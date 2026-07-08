//! Context (read-heavy) tool handlers. Part of the handler-group split: a separate
//! `#[tool_router(router = context_router)]` impl merged into the dispatch
//! table in `CuadernoServer::new`.

use std::str::FromStr;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorData};
use rmcp::{tool, tool_router};

use cdno_core::path::VaultPath;
use cdno_domain::SearchFilters;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{ProjectFrontmatter, QuestionDomain};

use crate::dto::{
    CommitmentEntryDto, DailyNoteViewDto, InboxItemDto, LintReportDto, MonthlyContextDto,
    MonthlyNoteViewDto, OrientationContextDto, PortfolioDetailDto, ProjectContextDto,
    ProjectListDto, ProjectListEntryDto, ProjectSlotsDto, QuestionSummaryDto, SearchResultDto,
    StewardshipTrackingDto, WEEKLY_LOGS_MAX, WeeklyContextDto, WeeklyNoteViewDto, cap_recent_logs,
};

use crate::input::*;

use crate::util::{
    into_mcp_error, invalid_argument, json_result, monday_of_iso_week, parse_period_into_from_date,
    parse_question_slug_from_wikilink,
};

use crate::server::CuadernoServer;

#[tool_router(router = context_router, vis = "pub")]
impl CuadernoServer {
    #[tool(
        description = "Today's orientation: commitments due soon, active projects with their top action, and lapsed stewardship habits. The `energy` field is reserved for client-side suggestion biasing; the server returns the raw context unfiltered."
    )]
    pub async fn get_orientation(
        &self,
        Parameters(_input): Parameters<GetOrientationInput>,
    ) -> Result<CallToolResult, ErrorData> {
        // `energy` is intentionally ignored at the server: the domain
        // returns the raw orientation context, and the client biases
        // the suggestion locally. Same separation the CLI uses (see
        // `commands/orient.rs::suggestion`).
        let today = chrono::Local::now().date_naive();
        let ctx = self
            .with_vault(move |vault| vault.orientation_context(today))
            .await?
            .map_err(into_mcp_error)?;
        json_result(OrientationContextDto::from(ctx))
    }

    #[tool(
        description = "This week's logs, completed actions, project state changes, and the upcoming two weeks of commitments. The ISO week (Mon-Sun) containing today is used; the returned `week_of` field carries the resolved Monday so clients render the window explicitly. The payload is bounded for token-cap safety: each `state_changes` entry carries only a ~200-char gist (marked with a trailing \u{2026}) of its before/after Current State bodies, and `logs` is capped to the 100 most-recent lines. The full detail stays one `get_project_context` / `read_daily_note` away. Stewardship status, called out in design \u{00a7}11 alongside this tool, is reachable separately through `get_stewardship_tracking`."
    )]
    pub async fn get_weekly_context(
        &self,
        Parameters(_input): Parameters<EmptyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = chrono::Local::now().date_naive();
        let monday = monday_of_iso_week(today);
        let sunday = monday + chrono::Duration::days(6);

        let (logs, completed, state_changes, commitments) = self
            .with_vault(move |vault| {
                let logs = vault.weekly_logs(today)?;
                let completed = vault.completed_actions_between(monday, sunday)?;
                let state_changes = vault.project_state_changes_between(monday, sunday)?;
                let commitments = vault.commitments(today, 14)?;
                Ok::<_, DomainError>((logs, completed, state_changes, commitments))
            })
            .await?
            .map_err(into_mcp_error)?;

        // Bound the two unbounded slices before serialising (GH #298):
        // `state_changes` bodies are truncated per-entry in the DTO's
        // `From` impl; `logs` is capped to the most-recent lines here.
        let logs = cap_recent_logs(logs.into_iter().map(Into::into).collect(), WEEKLY_LOGS_MAX);
        json_result(WeeklyContextDto {
            week_of: monday,
            logs,
            completed_actions: completed.into_iter().map(Into::into).collect(),
            state_changes: state_changes.into_iter().map(Into::into).collect(),
            commitments: commitments.into_iter().map(Into::into).collect(),
        })
    }

    #[tool(
        description = "Strategic monthly scan: the past 30 days' completed actions (wins patterns), every active question, the portfolio health table, active projects unchanged for >2 weeks (stuck-detection), every stewardship dashboard, a six-week commitments lookahead, and active-project slot allocation against the configured cap."
    )]
    pub async fn get_monthly_context(
        &self,
        Parameters(_input): Parameters<EmptyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = chrono::Local::now().date_naive();
        let since = today - chrono::Duration::days(30);

        let (
            completed,
            active_questions,
            portfolios,
            stuck,
            stewardships,
            commitments,
            active_count,
            cap,
        ) = self
            .with_vault(move |vault| {
                let completed = vault.completed_actions_between(since, today)?;
                let active_questions = vault.active_questions()?;
                let portfolios = vault.list_portfolios(today)?;
                // Design §11: "project stuck-check, unchanged >2 weeks" — 14 days.
                let stuck = vault.stuck_projects(today, 14)?;
                let stewardships = vault.list_stewardships(today)?;
                // Six-week (42-day) lookahead per design §11.
                let commitments = vault.commitments(today, 42)?;
                let active_count = vault.active_projects()?.len();
                let cap = vault.config().vault.max_active_projects;
                Ok::<_, DomainError>((
                    completed,
                    active_questions,
                    portfolios,
                    stuck,
                    stewardships,
                    commitments,
                    active_count,
                    cap,
                ))
            })
            .await?
            .map_err(into_mcp_error)?;

        json_result(MonthlyContextDto {
            since,
            completed_actions: completed.into_iter().map(Into::into).collect(),
            active_questions: active_questions.into_iter().map(Into::into).collect(),
            portfolios: portfolios.into_iter().map(Into::into).collect(),
            stuck_projects: stuck.into_iter().map(Into::into).collect(),
            stewardships: stewardships.into_iter().map(Into::into).collect(),
            commitments: commitments.into_iter().map(Into::into).collect(),
            slots: ProjectSlotsDto {
                active: active_count,
                cap,
            },
        })
    }

    #[tool(
        description = "List the vault's projects: active and parked, each with its slug and typed frontmatter, plus the slot budget (active count vs the configured cap). The lightweight enumeration tool -- use it to discover project slugs without pulling a full `get_monthly_context` or per-project `get_project_context`."
    )]
    pub async fn list_projects(
        &self,
        Parameters(_input): Parameters<EmptyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let (active, parked, cap) = self
            .with_vault(move |vault| {
                let active = vault.active_projects()?;
                let parked = vault.parked_projects()?;
                let cap = vault.config().vault.max_active_projects;
                Ok::<_, DomainError>((active, parked, cap))
            })
            .await?
            .map_err(into_mcp_error)?;
        let slots = ProjectSlotsDto {
            active: active.len(),
            cap,
        };
        json_result(ProjectListDto {
            active: active.into_iter().map(project_list_entry).collect(),
            parked: parked.into_iter().map(project_list_entry).collect(),
            slots,
        })
    }

    #[tool(
        description = "The four-source aggregated commitments timeline: project milestones with hard deadlines, stewardship periodic commitments, standalone commitment notes, and action notes with a self-imposed due date. `lookahead_weeks` (default 2) sets the forward window; overdue commitments are always included. Mirrors `cdno commitments --weeks N`."
    )]
    pub async fn get_commitments(
        &self,
        Parameters(input): Parameters<GetCommitmentsInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = chrono::Local::now().date_naive();
        let weeks = input.lookahead_weeks.unwrap_or(2);
        let lookahead_days = i64::from(weeks) * 7;
        let entries = self
            .with_vault(move |vault| vault.commitments(today, lookahead_days))
            .await?
            .map_err(into_mcp_error)?;
        let dtos: Vec<CommitmentEntryDto> = entries.into_iter().map(Into::into).collect();
        json_result(dtos)
    }

    #[tool(
        description = "Validate every indexed note and return a structured report: unknown note types, missing required fields, append-only violations, attachment-pairing problems (all `error`), broken wikilinks (`warning`; body links only -- frontmatter links like `project:`/`origin:` are out of scope), and malformed stewardship-dashboard bullets (`warning`; `## Active Habits` / `## Periodic Commitments` lines the canonical parsers reject). The programmatic backing for the `vault-lint` skill; `clean` is true when nothing was found."
    )]
    pub async fn lint(
        &self,
        Parameters(_input): Parameters<EmptyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = self
            .with_vault(|vault| vault.lint_all_notes())
            .await?
            .map_err(into_mcp_error)?;
        json_result(LintReportDto::from(report))
    }

    #[tool(
        description = "List uncategorised captures under `inbox/` awaiting triage (each: `slug` + `text`), oldest first. The read half of draining the inbox: route an item into a task/note with the usual create tool (e.g. `add_action`), then call `discard_inbox_item` with its slug to clear it."
    )]
    pub async fn triage_inbox(
        &self,
        Parameters(_input): Parameters<EmptyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let items = self
            .with_vault(|vault| vault.list_inbox())
            .await?
            .map_err(into_mcp_error)?;
        let dtos: Vec<InboxItemDto> = items.into_iter().map(Into::into).collect();
        json_result(dtos)
    }

    #[tool(
        description = "Full context for a single project: typed frontmatter, the full body of the project map, recent daily-log mentions (past 30 days, bare or qualified wikilinks), body backlinks grouped by source note type, and the resolved core_question summary when the project sets one. Resolves the slug against both `projects/` and `projects/_parked/`."
    )]
    pub async fn get_project_context(
        &self,
        Parameters(input): Parameters<ProjectSlugInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = chrono::Local::now().date_naive();
        let since = today - chrono::Duration::days(30);

        let project = input.project.clone();
        let (fm, body, mentions, backlinks, core_question) = self
            .with_vault(move |vault| {
                let (fm, body) = vault.get_project_full(&project)?;
                let mentions = vault.daily_log_mentions(&project, since)?;
                let backlinks = vault.project_backlinks(&project)?;

                // Resolve core_question when present. Parse the wikilink
                // target out of `"[[questions/<domain>/<slug>]]"`, look it
                // up in the full question list (active OR otherwise), and
                // include the summary. Quietly silent on any parse / lookup
                // failure — better to return None than to surface a "broken
                // wikilink" error from a read-only context query. Lint is
                // where that surfaces.
                let core_question = if let Some(link) = fm.core_question.as_deref() {
                    parse_question_slug_from_wikilink(link).and_then(|slug| {
                        vault
                            .list_questions()
                            .ok()
                            .and_then(|qs| qs.into_iter().find(|q| q.slug == slug))
                    })
                } else {
                    None
                };

                Ok::<_, DomainError>((fm, body, mentions, backlinks, core_question))
            })
            .await?
            .map_err(into_mcp_error)?;

        json_result(ProjectContextDto {
            slug: input.project,
            frontmatter: fm.into(),
            body_markdown: body,
            recent_mentions: mentions.into_iter().map(Into::into).collect(),
            backlinks: backlinks.into(),
            core_question: core_question.map(QuestionSummaryDto::from),
        })
    }

    #[tool(
        description = "A portfolio's frontmatter and every evidence note filed into it (vault path, created date, source, and origin wikilink)."
    )]
    pub async fn get_portfolio_contents(
        &self,
        Parameters(input): Parameters<PortfolioSlugInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let portfolio = input.portfolio.clone();
        let (fm, evidence) = self
            .with_vault(move |vault| {
                let fm = vault.get_portfolio(&portfolio)?;
                let evidence = vault.get_portfolio_contents(&portfolio)?;
                Ok::<_, DomainError>((fm, evidence))
            })
            .await?
            .map_err(into_mcp_error)?;
        json_result(PortfolioDetailDto::new(input.portfolio, fm, evidence))
    }

    #[tool(
        description = "Structured tracking data for a stewardship's activity (gym sessions, body measurements, swim sets, ...) for trend analysis. `activity` filters to the named activity slug per design \u{00a7}11. `period` is a lookback like `30d`, `4w`, `6m`, `1y`; defaults to `90d` when omitted. Calendar-aware: months and years subtract via chrono rather than rough day counts."
    )]
    pub async fn get_stewardship_tracking(
        &self,
        Parameters(input): Parameters<GetStewardshipTrackingInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = chrono::Local::now().date_naive();
        let from = match input.period.as_deref() {
            Some(p) => {
                parse_period_into_from_date(p, today).map_err(|e| invalid_argument("period", &e))?
            }
            None => today - chrono::Duration::days(90),
        };
        let stewardship = input.stewardship.clone();
        let activity = input.activity.clone();
        let entries = self
            .with_vault(move |vault| {
                vault.list_tracking(&stewardship, Some(&activity), from, today)
            })
            .await?
            .map_err(into_mcp_error)?;

        json_result(StewardshipTrackingDto {
            stewardship: input.stewardship,
            activity: Some(input.activity),
            from,
            to: today,
            entries: entries.into_iter().map(Into::into).collect(),
        })
    }

    #[tool(
        description = "Every question with status: active, sorted by (domain, slug). Filter by `domain` (`research` or `life`) to limit; omit for all."
    )]
    pub async fn get_active_questions(
        &self,
        Parameters(input): Parameters<GetActiveQuestionsInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let filter = match input.domain.as_deref() {
            Some(d) => Some(
                QuestionDomain::from_str(d)
                    .map_err(|e| invalid_argument("domain", &e.to_string()))?,
            ),
            None => None,
        };
        let mut active = self
            .with_vault(|vault| vault.active_questions())
            .await?
            .map_err(into_mcp_error)?;
        if let Some(d) = filter {
            active.retain(|q| q.domain == d);
        }
        let dtos: Vec<QuestionSummaryDto> = active.into_iter().map(Into::into).collect();
        json_result(dtos)
    }

    #[tool(
        description = "Read the daily note for a date (defaults to today). Returns `{ path, exists, markdown }`. A day with no note yet returns `exists: false` and empty `markdown` rather than erroring, so callers can check for pre-planned content (a written intention, a pre-filled agenda) before deciding what to write."
    )]
    pub async fn read_daily_note(
        &self,
        Parameters(input): Parameters<ReadDailyNoteInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let date = input
            .date
            .unwrap_or_else(|| chrono::Local::now().date_naive());
        let view = self
            .with_vault(move |vault| vault.read_daily_note(date))
            .await?
            .map_err(into_mcp_error)?;
        json_result(DailyNoteViewDto::from(view))
    }

    #[tool(
        description = "Read the weekly note for the ISO week containing `date` (any day in the week; defaults to this week). Returns `{ path, exists, markdown }`. A week with no note yet returns `exists: false` and empty `markdown` rather than erroring, so a skill can check whether the week is already started before composing. The note's sections are Wins, Challenges, One Improvement, and This Week's Goal (the week's anchoring goal — read it for the day's umbrella in a daily orientation)."
    )]
    pub async fn read_weekly_note(
        &self,
        Parameters(input): Parameters<ReadWeeklyNoteInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let date = input
            .date
            .unwrap_or_else(|| chrono::Local::now().date_naive());
        let view = self
            .with_vault(move |vault| vault.read_weekly_note(date))
            .await?
            .map_err(into_mcp_error)?;
        json_result(WeeklyNoteViewDto::from(view))
    }

    #[tool(
        description = "Read the monthly note for the calendar month containing `date` (any day in the month; defaults to this month). Returns `{ path, exists, markdown }`. A month with no note yet returns `exists: false` and empty `markdown` rather than erroring, so a skill can check whether the month is already started before composing. The note's review sections are Wins, Themes, and Next Month's Focus; a scaffolded `## Weeks` block links (never copies) the month's weekly notes so the weeks stay the source of truth."
    )]
    pub async fn read_monthly_note(
        &self,
        Parameters(input): Parameters<ReadMonthlyNoteInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let date = input
            .date
            .unwrap_or_else(|| chrono::Local::now().date_naive());
        let view = self
            .with_vault(move |vault| vault.read_monthly_note(date))
            .await?
            .map_err(into_mcp_error)?;
        json_result(MonthlyNoteViewDto::from(view))
    }

    #[tool(
        description = "Full-text search across all notes by content, ranked best-first. Optional filters: `note_type` (e.g. `project`, `evidence`, `daily`), a `from`/`to` ISO date window (matched against the note's date), and `portfolio`. Free-text `query` is matched case-insensitively with terms ANDed. Returns `{ path, note_type, title, snippet, score }` per hit — `snippet` brackets the matched terms; lower `score` is a better match."
    )]
    pub async fn search_notes(
        &self,
        Parameters(input): Parameters<SearchNotesInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let results = self
            .with_vault(move |vault| {
                // Any built-in or config-defined custom type name. Validate against the
                // registry so a typo is a clear INVALID_PARAMS rather than a silent
                // empty result (an LLM client has no tab-completion to catch it).
                if let Some(t) = input.note_type.as_deref()
                    && !vault.type_registry().is_known(t)
                {
                    return Err(invalid_argument(
                        "note_type",
                        &format!("unknown note type '{t}'"),
                    ));
                }
                let note_type_names = input.note_type.into_iter().collect();
                let filters = SearchFilters {
                    note_type_names,
                    date_from: input.from,
                    date_to: input.to,
                    portfolio: input.portfolio,
                };
                Ok(vault.search(&input.query, &filters, input.limit))
            })
            .await??
            .map_err(into_mcp_error)?;
        let dtos: Vec<SearchResultDto> = results.into_iter().map(Into::into).collect();
        json_result(dtos)
    }
}

/// Build a `list_projects` row from a domain `(path, frontmatter)`
/// pair: the slug is the file stem (`projects/<slug>.md` for active,
/// `projects/_parked/<slug>.md` for parked).
fn project_list_entry((path, fm): (VaultPath, ProjectFrontmatter)) -> ProjectListEntryDto {
    let slug = path
        .as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned();
    ProjectListEntryDto {
        slug,
        frontmatter: fm.into(),
    }
}
