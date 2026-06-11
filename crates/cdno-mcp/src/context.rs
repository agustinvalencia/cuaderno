//! Context (read-heavy) tool handlers. Part of the handler-group split: a separate
//! `#[tool_router(router = context_router)]` impl merged into the dispatch
//! table in `CuadernoServer::new`.

use std::str::FromStr;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorData};
use rmcp::{tool, tool_router};

use cdno_domain::SearchFilters;
use cdno_domain::frontmatter::QuestionDomain;
use cdno_domain::note_type::NoteType;

use crate::dto::{
    DailyNoteViewDto, MonthlyContextDto, OrientationContextDto, PortfolioDetailDto,
    ProjectContextDto, ProjectSlotsDto, QuestionSummaryDto, SearchResultDto,
    StewardshipTrackingDto, WeeklyContextDto,
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
            .vault
            .orientation_context(today)
            .map_err(into_mcp_error)?;
        json_result(OrientationContextDto::from(ctx))
    }

    #[tool(
        description = "This week's logs, completed actions, project state changes, and the upcoming two weeks of commitments. The ISO week (Mon-Sun) containing today is used; the returned `week_of` field carries the resolved Monday so clients render the window explicitly. Stewardship status, called out in design \u{00a7}11 alongside this tool, is reachable separately through `get_stewardship_tracking`."
    )]
    pub async fn get_weekly_context(
        &self,
        Parameters(_input): Parameters<EmptyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = chrono::Local::now().date_naive();
        let monday = monday_of_iso_week(today);
        let sunday = monday + chrono::Duration::days(6);

        let logs = self.vault.weekly_logs(today).map_err(into_mcp_error)?;
        let completed = self
            .vault
            .completed_actions_between(monday, sunday)
            .map_err(into_mcp_error)?;
        let state_changes = self
            .vault
            .project_state_changes_between(monday, sunday)
            .map_err(into_mcp_error)?;
        let commitments = self.vault.commitments(today, 14).map_err(into_mcp_error)?;

        json_result(WeeklyContextDto {
            week_of: monday,
            logs: logs.into_iter().map(Into::into).collect(),
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

        let completed = self
            .vault
            .completed_actions_between(since, today)
            .map_err(into_mcp_error)?;
        let active_questions = self.vault.active_questions().map_err(into_mcp_error)?;
        let portfolios = self.vault.list_portfolios(today).map_err(into_mcp_error)?;
        // Design §11: "project stuck-check, unchanged >2 weeks" — 14 days.
        let stuck = self
            .vault
            .stuck_projects(today, 14)
            .map_err(into_mcp_error)?;
        let stewardships = self
            .vault
            .list_stewardships(today)
            .map_err(into_mcp_error)?;
        // Six-week (42-day) lookahead per design §11.
        let commitments = self.vault.commitments(today, 42).map_err(into_mcp_error)?;
        let active_count = self.vault.active_projects().map_err(into_mcp_error)?.len();
        let cap = self.vault.config().vault.max_active_projects;

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
        description = "Full context for a single project: typed frontmatter, the full body of the project map, recent daily-log mentions (past 30 days, bare or qualified wikilinks), body backlinks grouped by source note type, and the resolved core_question summary when the project sets one. Resolves the slug against both `projects/` and `projects/_parked/`."
    )]
    pub async fn get_project_context(
        &self,
        Parameters(input): Parameters<ProjectSlugInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = chrono::Local::now().date_naive();
        let since = today - chrono::Duration::days(30);

        let (fm, body) = self
            .vault
            .get_project_full(&input.project)
            .map_err(into_mcp_error)?;
        let mentions = self
            .vault
            .daily_log_mentions(&input.project, since)
            .map_err(into_mcp_error)?;
        let backlinks = self
            .vault
            .project_backlinks(&input.project)
            .map_err(into_mcp_error)?;

        // Resolve core_question when present. Parse the wikilink
        // target out of `"[[questions/<domain>/<slug>]]"`, look it
        // up in the full question list (active OR otherwise), and
        // include the summary. Quietly silent on any parse / lookup
        // failure — better to return None than to surface a "broken
        // wikilink" error from a read-only context query. Lint is
        // where that surfaces.
        let core_question = if let Some(link) = fm.core_question.as_deref() {
            parse_question_slug_from_wikilink(link)
                .and_then(|slug| {
                    self.vault
                        .list_questions()
                        .ok()
                        .and_then(|qs| qs.into_iter().find(|q| q.slug == slug))
                })
                .map(QuestionSummaryDto::from)
        } else {
            None
        };

        json_result(ProjectContextDto {
            slug: input.project,
            frontmatter: fm.into(),
            body_markdown: body,
            recent_mentions: mentions.into_iter().map(Into::into).collect(),
            backlinks: backlinks.into(),
            core_question,
        })
    }

    #[tool(
        description = "A portfolio's frontmatter and every evidence note filed into it (vault path, created date, source, and origin wikilink)."
    )]
    pub async fn get_portfolio_contents(
        &self,
        Parameters(input): Parameters<PortfolioSlugInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let fm = self
            .vault
            .get_portfolio(&input.portfolio)
            .map_err(into_mcp_error)?;
        let evidence = self
            .vault
            .get_portfolio_contents(&input.portfolio)
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
        let entries = self
            .vault
            .list_tracking(&input.stewardship, Some(&input.activity), from, today)
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
        let mut active = self.vault.active_questions().map_err(into_mcp_error)?;
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
        let view = self.vault.read_daily_note(date).map_err(into_mcp_error)?;
        json_result(DailyNoteViewDto::from(view))
    }

    #[tool(
        description = "Full-text search across all notes by content, ranked best-first. Optional filters: `note_type` (e.g. `project`, `evidence`, `daily`), a `from`/`to` ISO date window (matched against the note's date), and `portfolio`. Free-text `query` is matched case-insensitively with terms ANDed. Returns `{ path, note_type, title, snippet, score }` per hit — `snippet` brackets the matched terms; lower `score` is a better match."
    )]
    pub async fn search_notes(
        &self,
        Parameters(input): Parameters<SearchNotesInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let note_types = match input.note_type.as_deref() {
            Some(t) => vec![
                NoteType::from_str(t).map_err(|e| invalid_argument("note_type", &e.to_string()))?,
            ],
            None => Vec::new(),
        };
        let filters = SearchFilters {
            note_types,
            date_from: input.from,
            date_to: input.to,
            portfolio: input.portfolio,
        };
        let results = self
            .vault
            .search(&input.query, &filters, input.limit)
            .map_err(into_mcp_error)?;
        let dtos: Vec<SearchResultDto> = results.into_iter().map(Into::into).collect();
        json_result(dtos)
    }
}
