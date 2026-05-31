//! `CuadernoServer` — the rmcp [`ServerHandler`] that exposes the
//! design §11 tools to MCP clients (Claude Desktop, Claude Code, any
//! agent that speaks MCP).
//!
//! Status: 14 of 16 tools wired through to the domain. The two
//! remaining stubs (`get_project_context`, `get_stewardship_tracking`)
//! land one PR each as the next GH #142 follow-ups. Their bodies
//! return [`rmcp::ErrorData::internal_error`] via
//! `not_yet_implemented` so a client that calls one gets a clear
//! "not implemented" response rather than a panic.
//!
//! # Imports note
//!
//! `JsonSchema` comes from the top-level `schemars` crate (pinned to
//! the same major as rmcp's transitive version — the derive macro's
//! hygiene resolves `::schemars::...` paths, so the re-export at
//! `rmcp::schemars` alone isn't enough). `Parameters<T>` lives at
//! `rmcp::handler::server::wrapper::Parameters` — the canonical
//! tool-argument extractor; rmcp deserialises the incoming JSON
//! against `T`'s `JsonSchema` and hands the typed value to the
//! method body.

use std::str::FromStr;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, ErrorData, Implementation, ProtocolVersion, ServerCapabilities,
    ServerInfo,
};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{Context, EnergyLevel, QuestionDomain};

use crate::dto::{
    MonthlyContextDto, OrientationContextDto, PortfolioDetailDto, ProjectSlotsDto,
    QuestionSummaryDto, WeeklyContextDto, WriteResultDto,
};

// ---------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------
//
// Each tool's input lives here as a `derive(Deserialize, JsonSchema)`
// struct. Tools with no parameters take `Parameters<EmptyInput>` —
// rmcp generates an `object`-typed empty schema rather than `null`.
//
// Enum-typed inputs (energy, context, question domain) are typed as
// `String` here rather than the domain enum because the domain types
// do not derive `JsonSchema` (they live in `cdno-domain`, which has
// no schemars dependency). Handlers parse via `FromStr` when they
// land in #46 / #47.

/// Empty input for tools that take no parameters.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct EmptyInput {}

/// Input for [`CuadernoServer::get_orientation`]. `energy` biases the
/// suggested starting point per design §11.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetOrientationInput {
    /// Optional bias toward `"deep"`, `"medium"`, or `"light"`.
    pub energy: Option<String>,
}

/// Input for tools that take a single project slug.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectSlugInput {
    pub project: String,
}

/// Input for tools that take a single portfolio slug.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioSlugInput {
    pub portfolio: String,
}

/// Input for `get_stewardship_tracking` (design §11).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetStewardshipTrackingInput {
    pub stewardship: String,
    pub activity: String,
    /// `period` is a free-form lookback window (e.g. `"30d"`,
    /// `"6m"`); parsing lands with the handler in #46.
    pub period: Option<String>,
}

/// Input for `get_active_questions` (design §11). `domain` is
/// optional — when omitted, all domains are returned.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetActiveQuestionsInput {
    /// Optional filter: `"research"` or `"life"`. Omitted = all.
    pub domain: Option<String>,
}

/// Input for `append_to_log`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AppendToLogInput {
    pub text: String,
}

/// Input for `file_to_portfolio`. `origin` is the bare wikilink
/// target (e.g. `"projects/surrogate-model"`); the domain wraps it
/// per design §5.5.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileToPortfolioInput {
    pub portfolio: String,
    pub source: String,
    pub origin: String,
    #[serde(default)]
    pub content: String,
}

/// Input for `update_project_state`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateProjectStateInput {
    pub project: String,
    pub new_state: String,
}

/// Input for `add_action`. `with_note` flips between the inline
/// bullet (default) and the heavier action-note form (design §5.11).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddActionInput {
    pub project: String,
    pub title: String,
    /// One of `"deep"`, `"medium"`, `"light"`.
    pub energy: String,
    #[serde(default)]
    pub with_note: bool,
}

/// Input for the substring-match action verbs (`promote_action`,
/// `complete_action`). `query` is matched against the bullet text.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ActionQueryInput {
    pub project: String,
    pub query: String,
}

/// Input for `create_commitment`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateCommitmentInput {
    pub title: String,
    /// ISO `YYYY-MM-DD`.
    pub due: chrono::NaiveDate,
    /// One of the [`cdno_domain::frontmatter::Context`] variants
    /// (kebab-case: `work`, `household`, `personal`, …).
    pub context: String,
    pub project: Option<String>,
    pub stewardship: Option<String>,
}

/// Input for `complete_commitment` — commitment slug.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteCommitmentInput {
    pub commitment: String,
}

/// Input for `create_tracking_entry`. `routine` is the bare slug of
/// a routine doc (gym/swim templates only); domain wraps the
/// wikilink.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateTrackingEntryInput {
    pub stewardship: String,
    pub activity: String,
    pub routine: Option<String>,
    #[serde(default)]
    pub content: String,
}

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
        description = "(not yet implemented \u{2014} see GH #142) Full context for a single project: the project map, recent daily log entries mentioning it, linked portfolio summaries, and the linked question."
    )]
    async fn get_project_context(
        &self,
        Parameters(_input): Parameters<ProjectSlugInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("get_project_context"))
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
        description = "(not yet implemented \u{2014} see GH #142) Structured tracking data for a stewardship's activity (gym sessions, body measurements, ...) for trend analysis. `period` is a lookback like `30d`, `6m`."
    )]
    async fn get_stewardship_tracking(
        &self,
        Parameters(_input): Parameters<GetStewardshipTrackingInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("get_stewardship_tracking"))
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

/// Placeholder error returned by every stubbed tool method until
/// its handler lands. With #142a + #142b (weekly + monthly context)
/// shipped, the two remaining stubs are `get_project_context` and
/// `get_stewardship_tracking` — each lands as its own GH #142
/// follow-up PR. Includes the tool name so a client sees exactly
/// which surface isn't ready yet.
fn not_yet_implemented(tool_name: &str) -> ErrorData {
    ErrorData::internal_error(
        format!("tool '{tool_name}' is registered but not yet implemented"),
        None,
    )
}

/// Wrap a serialisable DTO as the single content item of a
/// successful tool result. Shared by every implemented handler so
/// the JSON-encoding step is one call site, not 16.
fn json_result<S: serde::Serialize>(value: S) -> Result<CallToolResult, ErrorData> {
    let content = Content::json(value)?;
    Ok(CallToolResult::success(vec![content]))
}

/// Translate a [`DomainError`] into an rmcp [`ErrorData`]. We surface
/// the domain's `Display` output as the JSON-RPC error message — it's
/// already human-readable (see `cdno-domain/src/error.rs`). All
/// variants land as `InternalError` for now; the JSON-RPC code-mapping
/// table (per design §5.2) is a follow-up if clients start
/// branching on the code.
fn into_mcp_error(e: DomainError) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

/// Build an InvalidParams error pointing at a specific input field.
/// Used by handlers that accept enum-typed strings (e.g. the
/// `domain` filter on `get_active_questions`) and need to reject a
/// value that doesn't parse.
fn invalid_argument(field: &str, reason: &str) -> ErrorData {
    ErrorData::invalid_params(format!("invalid '{field}': {reason}"), None)
}

/// Compute the Monday of the ISO-8601 week containing `date`. ISO
/// week (Mon-Sun) rather than locale week so behaviour is identical
/// across deployments. Duplicates the domain's internal helper of
/// the same name; kept here rather than re-exporting it because
/// each handler may want a different windowing strategy in future.
fn monday_of_iso_week(date: chrono::NaiveDate) -> chrono::NaiveDate {
    use chrono::Datelike;
    let days_since_monday = date.weekday().num_days_from_monday() as i64;
    date - chrono::Duration::days(days_since_monday)
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
