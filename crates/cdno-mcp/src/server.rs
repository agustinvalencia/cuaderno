//! `CuadernoServer` — the rmcp [`ServerHandler`] that exposes the
//! design §11 tools to MCP clients (Claude Desktop, Claude Code, any
//! agent that speaks MCP).
//!
//! Tools are stubs in this PR (#45). #46 fills in the
//! context-gathering tools (`get_orientation`, `get_*_context`,
//! queries); #47 fills in the operation tools (`append_to_log`,
//! `update_project_state`, the create/complete pairs). The stubs
//! return [`rmcp::ErrorData::internal_error`] so a client that calls
//! one before its handler lands gets a clear "not implemented"
//! response rather than a panic.
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

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ErrorData, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use cdno_domain::Vault;

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
        description = "Today's orientation: commitments due soon, active projects with their top action, lapsed stewardship habits, and a suggested starting point. Bias the suggestion with `energy`."
    )]
    async fn get_orientation(
        &self,
        Parameters(_input): Parameters<GetOrientationInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("get_orientation"))
    }

    #[tool(
        description = "This week's wins, completed actions, project state changes, stewardship status, and commitments for the next two weeks."
    )]
    async fn get_weekly_context(
        &self,
        Parameters(_input): Parameters<EmptyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("get_weekly_context"))
    }

    #[tool(
        description = "Strategic monthly scan: wins patterns, active questions, portfolio health, project stuck-detection, stewardship overview, and a six-week commitments lookahead."
    )]
    async fn get_monthly_context(
        &self,
        Parameters(_input): Parameters<EmptyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("get_monthly_context"))
    }

    #[tool(
        description = "Full context for a single project: the project map, recent daily log entries mentioning it, linked portfolio summaries, and the linked question."
    )]
    async fn get_project_context(
        &self,
        Parameters(_input): Parameters<ProjectSlugInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("get_project_context"))
    }

    #[tool(
        description = "Every evidence note filed into a portfolio, with dates, sources, and full bodies for deep consultation."
    )]
    async fn get_portfolio_contents(
        &self,
        Parameters(_input): Parameters<PortfolioSlugInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("get_portfolio_contents"))
    }

    #[tool(
        description = "Structured tracking data for a stewardship's activity (gym sessions, body measurements, ...) for trend analysis. `period` is a lookback like `30d`, `6m`."
    )]
    async fn get_stewardship_tracking(
        &self,
        Parameters(_input): Parameters<GetStewardshipTrackingInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("get_stewardship_tracking"))
    }

    #[tool(
        description = "Active questions, optionally filtered by domain (`research` or `life`). Grouped by domain when no filter applied."
    )]
    async fn get_active_questions(
        &self,
        Parameters(_input): Parameters<GetActiveQuestionsInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("get_active_questions"))
    }

    // -----------------------------------------------------------------
    // Operations (write, used by skills and UI)
    // -----------------------------------------------------------------

    #[tool(
        description = "Append a single line to today's daily log entry, creating the daily note if it doesn't yet exist."
    )]
    async fn append_to_log(
        &self,
        Parameters(_input): Parameters<AppendToLogInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("append_to_log"))
    }

    #[tool(
        description = "Create an evidence note in the named portfolio. `origin` is a bare wikilink target (e.g. `projects/foo`); the server wraps it."
    )]
    async fn file_to_portfolio(
        &self,
        Parameters(_input): Parameters<FileToPortfolioInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("file_to_portfolio"))
    }

    #[tool(
        description = "Rewrite a project's `## Current State` section, auto-logging the previous state to today's daily entry in the same atomic transaction."
    )]
    async fn update_project_state(
        &self,
        Parameters(_input): Parameters<UpdateProjectStateInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("update_project_state"))
    }

    #[tool(
        description = "Append a next-action bullet to a project. With `with_note: true`, also creates an action note (design §5.11) and rewrites the bullet to wikilink it."
    )]
    async fn add_action(
        &self,
        Parameters(_input): Parameters<AddActionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("add_action"))
    }

    #[tool(
        description = "Promote an existing bullet to an action note: matches the bullet by substring `query`, creates the note from the template, and rewrites the bullet to wikilink it."
    )]
    async fn promote_action(
        &self,
        Parameters(_input): Parameters<ActionQueryInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("promote_action"))
    }

    #[tool(
        description = "Complete an action: removes the bullet, logs the completion, and (if a note is attached) archives it to `actions/_done/<year>/`."
    )]
    async fn complete_action(
        &self,
        Parameters(_input): Parameters<ActionQueryInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("complete_action"))
    }

    #[tool(description = "Create a standalone commitment note with a due date and life context.")]
    async fn create_commitment(
        &self,
        Parameters(_input): Parameters<CreateCommitmentInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("create_commitment"))
    }

    #[tool(
        description = "Mark a commitment as completed: stamps the frontmatter, moves the file to `commitments/_done/<year>/`, and logs to the daily entry."
    )]
    async fn complete_commitment(
        &self,
        Parameters(_input): Parameters<CompleteCommitmentInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("complete_commitment"))
    }

    #[tool(
        description = "Scaffold a tracking note under an expanded stewardship. Built-in templates for `gym`, `body`, and `swim`; generic fallback for anything else. Returns the path for editing."
    )]
    async fn create_tracking_entry(
        &self,
        Parameters(_input): Parameters<CreateTrackingEntryInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Err(not_yet_implemented("create_tracking_entry"))
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

/// Placeholder error returned by every stubbed tool method until its
/// handler lands in #46 (context) or #47 (operations). Includes the
/// tool name so a client sees exactly which surface isn't ready yet.
fn not_yet_implemented(tool_name: &str) -> ErrorData {
    ErrorData::internal_error(
        format!("tool '{tool_name}' is registered but not yet implemented"),
        None,
    )
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
