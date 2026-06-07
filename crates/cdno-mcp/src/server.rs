//! `CuadernoServer` — the rmcp [`ServerHandler`] that exposes the
//! cuaderno tools to MCP clients (Claude Desktop, Claude Code, any
//! agent that speaks MCP).
//!
//! Status: all 22 tools are wired through to the domain — the 16
//! design §11 tools, the two daily-note tools (`read_daily_note`,
//! `upsert_daily_section`, GH #158), and the four structural-creation
//! tools (`create_project`, `create_portfolio`, `create_question`,
//! `create_stewardship`, GH #162). The `not_yet_implemented`
//! placeholder has been retired with no stubs left.
//!
//! # Layout note
//!
//! Tool input structs live in [`crate::input`] and helpers in
//! `crate::util`; this module holds the server struct, the tool
//! handlers, and the `ServerHandler` wiring. `Parameters<T>` lives at
//! `rmcp::handler::server::wrapper::Parameters` — the canonical
//! tool-argument extractor; rmcp deserialises the incoming JSON
//! against the input type's `JsonSchema` and hands the typed value to
//! the method body.

use std::str::FromStr;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ErrorData, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};

use cdno_domain::frontmatter::{Context, EnergyLevel, QuestionDomain};
use cdno_domain::{DailySection, Vault};

use crate::dto::{
    DailyNoteViewDto, MonthlyContextDto, OrientationContextDto, PortfolioDetailDto,
    ProjectContextDto, ProjectSlotsDto, QuestionSummaryDto, StewardshipTrackingDto,
    WeeklyContextDto, WriteResultDto,
};
// Re-exported so the existing `cdno_mcp::server::*Input` paths (used by
// tests) keep resolving after the structs moved to `crate::input`.
pub use crate::input::*;
use crate::util::{
    into_mcp_error, invalid_argument, json_result, monday_of_iso_week, parse_period_into_from_date,
    parse_question_slug_from_wikilink,
};

// ---------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------

/// The MCP server. Holds an [`Arc<Vault>`] so it's cheaply cloneable
/// (rmcp's `ServerHandler` requires `Clone + Send + Sync`), and a
/// [`ToolRouter`] built by the `#[tool_router]` macro.
#[derive(Clone)]
pub struct CuadernoServer {
    #[allow(dead_code)] // populated by #46/#47 handlers
    vault: Arc<Vault>,
    // The `#[tool_router]` macro reads this via `Self::tool_router()`
    // and the `#[tool_handler]` `ServerHandler` impl dispatches
    // through it at runtime — dead-code analysis can't trace the
    // proc-macro-generated reads.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl CuadernoServer {
    pub fn new(vault: Arc<Vault>) -> Self {
        Self {
            vault,
            tool_router: Self::tool_router(),
        }
    }

    /// Sorted snapshot of every advertised tool. Public so tests
    /// (and any external introspection client wrapping this binary)
    /// can verify the catalogue without going through the MCP
    /// protocol. Mirrors what `tools/list` returns over the wire.
    pub fn advertised_tools(&self) -> Vec<rmcp::model::Tool> {
        let mut tools = self.tool_router.list_all();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    // -----------------------------------------------------------------
    // Context (read-heavy, used by skills)
    // -----------------------------------------------------------------

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

    // -----------------------------------------------------------------
    // Operations (write, used by skills and UI)
    // -----------------------------------------------------------------

    #[tool(
        description = "Append a single line to today's daily log entry, creating the daily note if it doesn't yet exist."
    )]
    pub async fn append_to_log(
        &self,
        Parameters(input): Parameters<AppendToLogInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .log_to_daily_note(at, &input.text)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Logged to {}", path),
        ))
    }

    #[tool(
        description = "Create an evidence note in the named portfolio. `origin` is a bare wikilink target (e.g. `projects/foo`); the server wraps it. `content` is optional — empty string is fine; the user can flesh out the note after."
    )]
    pub async fn file_to_portfolio(
        &self,
        Parameters(input): Parameters<FileToPortfolioInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .file_evidence(
                at,
                &input.portfolio,
                &input.source,
                &input.origin,
                &input.content,
            )
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Filed evidence at {}", path),
        ))
    }

    #[tool(
        description = "Rewrite a project's `## Current State` section, auto-logging the previous state to today's daily entry in the same atomic transaction. No-op (returns the path) when `new_state` matches the existing state — silent so logging 'was X, now X' doesn't fire."
    )]
    pub async fn update_project_state(
        &self,
        Parameters(input): Parameters<UpdateProjectStateInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .update_project_state(at, &input.project, &input.new_state)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Updated state on {}", path),
        ))
    }

    #[tool(
        description = "Append a next-action bullet to a project. With `with_note: true`, also creates an action note (design §5.11) and rewrites the bullet to wikilink it. `energy` is one of `deep`, `medium`, `light`."
    )]
    pub async fn add_action(
        &self,
        Parameters(input): Parameters<AddActionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let energy = EnergyLevel::from_str(&input.energy)
            .map_err(|e| invalid_argument("energy", &e.to_string()))?;
        let path = if input.with_note {
            self.vault
                .add_action_with_note(at, &input.project, &input.title, energy)
                .map_err(into_mcp_error)?
        } else {
            self.vault
                .add_action(at, &input.project, &input.title, energy)
                .map_err(into_mcp_error)?
        };
        let label = if input.with_note {
            "Added action with note at"
        } else {
            "Added action bullet to"
        };
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("{label} {}", path),
        ))
    }

    #[tool(
        description = "Promote an existing bullet to an action note: matches the bullet on the project by substring `query`, creates the note from the template, and rewrites the bullet to wikilink it. Errors with `INTERNAL_ERROR` on ambiguous matches (multiple bullets contain `query`)."
    )]
    pub async fn promote_action(
        &self,
        Parameters(input): Parameters<ActionQueryInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .promote_action(at, &input.project, &input.query)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Promoted action note at {}", path),
        ))
    }

    #[tool(
        description = "Complete an action: matches the bullet on the project by substring `query`, removes the bullet, logs the completion to today's daily, and (if an action note is attached) archives it to `actions/_done/<year>/`."
    )]
    pub async fn complete_action(
        &self,
        Parameters(input): Parameters<ActionQueryInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .complete_action(at, &input.project, &input.query)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Completed action on {}", path),
        ))
    }

    #[tool(
        description = "Create a standalone commitment note with a due date and life context. `project` and `stewardship` are reserved on the input schema for the eventual originating-source link but ignored today \u{2014} the domain currently writes both as null per design \u{00a7}5.9 (originating commitments are tracked inline at their source: project milestones, stewardship periodic commitments)."
    )]
    pub async fn create_commitment(
        &self,
        Parameters(input): Parameters<CreateCommitmentInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let context = Context::from_str(&input.context)
            .map_err(|e| invalid_argument("context", &e.to_string()))?;
        // input.project and input.stewardship are deliberately unused
        // today; see tool description.
        let path = self
            .vault
            .create_commitment(at, &input.title, input.due, context)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created commitment at {}", path),
        ))
    }

    #[tool(
        description = "Mark an active commitment as completed: stamps the `status` and `completed` frontmatter fields, moves the file to `commitments/_done/<year>/`, and logs to today's daily entry. All in one atomic transaction."
    )]
    pub async fn complete_commitment(
        &self,
        Parameters(input): Parameters<CompleteCommitmentInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .complete_commitment(at, &input.commitment)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Completed commitment, archived to {}", path),
        ))
    }

    #[tool(
        description = "Scaffold a tracking note under an expanded stewardship. Built-in templates for `gym`, `body`, and `swim`; generic fallback for anything else. `routine` is the bare slug of a routine doc (e.g. `upper-body-a`); the server wraps it into the gym template's `routine:` wikilink. Returns the new path for the user to flesh out."
    )]
    pub async fn create_tracking_entry(
        &self,
        Parameters(input): Parameters<CreateTrackingEntryInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .add_tracking_entry(
                at,
                &input.stewardship,
                &input.activity,
                input.routine.as_deref(),
                &input.content,
            )
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Tracked at {}", path),
        ))
    }

    #[tool(
        description = "Create-or-replace a planning section of the daily note (defaults to today). `section` is one of `Standup`, `Intention`, `Agenda` (case-insensitive); any other value is rejected as an invalid argument. The append-only history sections (`## Logs`, `## Notes`) are NOT writable here — they only grow via `append_to_log`. Overwrites the section if present, creates it (and the daily note) if not. An empty `content` clears the section to just its heading."
    )]
    pub async fn upsert_daily_section(
        &self,
        Parameters(input): Parameters<UpsertDailySectionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let date = input
            .date
            .unwrap_or_else(|| chrono::Local::now().date_naive());
        let section = DailySection::from_str(&input.section)
            .map_err(|reason| invalid_argument("section", &reason))?;
        let path = self
            .vault
            .upsert_daily_section(date, section, &input.content)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Updated {} on {}", section.heading(), path),
        ))
    }

    #[tool(
        description = "Create a new project map. Below the active-project cap (default 5) the project is created active; at or above the cap it's created parked (`projects/_parked/<slug>`) so you can capture it without parking another first — the cap is enforced on activation, not creation. `context` is a kebab-case Context (`work`, `household`, `personal`, …). `core_question` is an optional bare wikilink target (e.g. `questions/research/foo`) linking the project to the question it answers."
    )]
    pub async fn create_project(
        &self,
        Parameters(input): Parameters<CreateProjectInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = chrono::Local::now().date_naive();
        let context = Context::from_str(&input.context)
            .map_err(|e| invalid_argument("context", &e.to_string()))?;
        let path = self
            .vault
            .create_project(today, &input.title, context, input.core_question.as_deref())
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created project at {}", path),
        ))
    }

    #[tool(
        description = "Create a portfolio (evidence folder + `_index.md`) for a question or topic. `project` optionally links it to a project slug."
    )]
    pub async fn create_portfolio(
        &self,
        Parameters(input): Parameters<CreatePortfolioInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .create_portfolio(at, &input.question, input.project.as_deref())
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created portfolio at {}", path),
        ))
    }

    #[tool(
        description = "Create a research or life question note. `domain` is `research` or `life`."
    )]
    pub async fn create_question(
        &self,
        Parameters(input): Parameters<CreateQuestionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let domain = QuestionDomain::from_str(&input.domain)
            .map_err(|e| invalid_argument("domain", &e.to_string()))?;
        let path = self
            .vault
            .create_question(at, domain, &input.text)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created question at {}", path),
        ))
    }

    #[tool(
        description = "Create a stewardship. With `expanded: true` it's a folder stewardship (`stewardships/<slug>/_index.md` with a lazy `tracking/`); otherwise a flat file. `context` is a kebab-case Context."
    )]
    pub async fn create_stewardship(
        &self,
        Parameters(input): Parameters<CreateStewardshipInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let context = Context::from_str(&input.context)
            .map_err(|e| invalid_argument("context", &e.to_string()))?;
        let path = if input.expanded {
            self.vault
                .create_stewardship_expanded(at, &input.name, context)
                .map_err(into_mcp_error)?
        } else {
            self.vault
                .create_stewardship_flat(at, &input.name, context)
                .map_err(into_mcp_error)?
        };
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created stewardship at {}", path),
        ))
    }
}

#[tool_handler]
impl ServerHandler for CuadernoServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::default()
            .with_protocol_version(ProtocolVersion::default())
            .with_server_info(Implementation::new("cdno-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Cuaderno MCP server. Tools are grouped into context-gathering reads \
                (get_orientation, get_*_context, queries) and write operations \
                (append_to_log, update_project_state, the create/complete pairs). \
                See docs/design.md §11 for the full surface.",
            )
            // ServerInfo::default already enables an empty capability
            // set; flip the `tools` flag on so clients know we serve
            // tools (the `#[tool_router]` machinery populates the
            // actual tool list at runtime).
            .with_capabilities(ServerCapabilities::builder().enable_tools().build())
    }
}

// `ServerInfo` doesn't expose a public `with_capabilities` builder,
// so the impl above goes through this small extension trait. Keeping
// it crate-local rather than reaching directly into the public
// `InitializeResult` fields (which are public despite the
// `non_exhaustive` attr on `Implementation`).
trait ServerInfoExt {
    fn with_capabilities(self, capabilities: ServerCapabilities) -> Self;
}

impl ServerInfoExt for ServerInfo {
    fn with_capabilities(mut self, capabilities: ServerCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}
