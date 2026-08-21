//! `CuadernoServer` — the rmcp [`ServerHandler`] that exposes the
//! cuaderno tools to MCP clients (Claude Desktop, Claude Code, any
//! agent that speaks MCP).
//!
//! Status: all 44 tools are wired through to the domain — the 16
//! design §11 tools, the two daily-note tools (GH #158), the two
//! weekly-note tools (`read_weekly_note`, `upsert_weekly_section`), the
//! two monthly-note tools (`read_monthly_note`,
//! `upsert_monthly_section`, GH #228), the
//! four structural-creation tools (GH #162), the four lifecycle tools
//! (`park_project`, `activate_project`, `set_question_status`,
//! `add_periodic_commitment`, GH #166), `search_notes` (#172),
//! `link_portfolio_to_question` (#200), the four read-parity tools
//! (`list_projects`, `get_commitments`, `lint`, `capture`, GH #204),
//! and the four milestone/waiting-on tools (`add_milestone`,
//! `complete_milestone`, `add_waiting_on`, `resolve_waiting_on`,
//! GH #213), and `create_custom_note` (GH #296). The authoritative
//! catalogue is the `tests/server.rs`
//! sorted-set assertion. Handlers are split by group
//! across sibling modules — `context.rs`, `operations.rs`,
//! `creation.rs`, `lifecycle.rs` — each a `#[tool_router]` merged into
//! the dispatch table in `new()`.
//!
//! # Layout note
//!
//! Tool input structs live in [`crate::input`], helpers in
//! `crate::util`, and the tool handlers in the per-group modules; this
//! module holds the server struct, `new()`, and the `ServerHandler`
//! wiring. `Parameters<T>` lives at
//! `rmcp::handler::server::wrapper::Parameters` — the canonical
//! tool-argument extractor; rmcp deserialises the incoming JSON
//! against the input type's `JsonSchema` and hands the typed value to
//! the method body.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool_handler};

use cdno_domain::Vault;

// Re-exported so the existing `cdno_mcp::server::*Input` paths (used by
// tests) keep resolving after the structs moved to `crate::input`.
pub use crate::input::*;

// ---------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------

/// The MCP server. Holds an [`Arc<Vault>`] so it's cheaply cloneable
/// (rmcp's `ServerHandler` requires `Clone + Send + Sync`), and the
/// merged [`ToolRouter`] built in [`CuadernoServer::new`].
#[derive(Clone)]
pub struct CuadernoServer {
    // Private: the per-group handler impls in sibling modules
    // (`context`/`operations`/`creation`/`lifecycle`) reach the vault
    // exclusively through [`CuadernoServer::with_vault`] (GH #303).
    vault: Arc<Vault>,
    // Merged dispatch table (the four per-group `#[tool_router]`s).
    // `#[tool_handler(router = self.tool_router)]` reads it at runtime;
    // dead-code analysis can't trace the proc-macro-generated reads.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl CuadernoServer {
    pub fn new(vault: Arc<Vault>) -> Self {
        // Tool handlers are split into per-group `#[tool_router]` impls
        // (context / operations / creation in sibling modules, lifecycle
        // in `lifecycle.rs`); `new` merges their routers into the
        // dispatch table.
        let mut tool_router = Self::context_router();
        tool_router.merge(Self::operations_router());
        tool_router.merge(Self::creation_router());
        tool_router.merge(Self::lifecycle_router());
        Self { vault, tool_router }
    }

    /// Read-only variant: only the context-gathering read tools
    /// (`context_router` — orientation, the `get_*_context` family,
    /// reads and search). No operations / creation / lifecycle tools
    /// are advertised or dispatchable, so a client of this server
    /// cannot mutate the vault at all. Used by `cdno-mcp-server
    /// --read-only` for scoped remote deployments and the initial
    /// exposure soak (GH #61).
    pub fn read_only(vault: Arc<Vault>) -> Self {
        Self {
            vault,
            tool_router: Self::context_router(),
        }
    }

    /// Sorted snapshot of every advertised tool. Public so tests (and
    /// any external introspection client wrapping this binary) can
    /// verify the catalogue without going through the MCP protocol.
    /// Mirrors what `tools/list` returns over the wire.
    pub fn advertised_tools(&self) -> Vec<rmcp::model::Tool> {
        let mut tools = self.tool_router.list_all();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    /// Run a synchronous domain call on tokio's blocking pool.
    ///
    /// Tool handlers are async only because rmcp requires it; the
    /// domain is deliberately synchronous (implementation-plan §4).
    /// Routing every vault call through here keeps blocking disk/
    /// SQLite work off the async workers (GH #303 — flagged by the
    /// PR #304/#306 reviews), so slow calls queue on the blocking
    /// pool instead of starving the accept loop. The closure returns
    /// whatever the call site needs (usually a `Result<T, DomainError>`
    /// that the caller then maps with `into_mcp_error`).
    pub(crate) async fn with_vault<R>(
        &self,
        f: impl FnOnce(&Vault) -> R + Send + 'static,
    ) -> Result<R, rmcp::model::ErrorData>
    where
        R: Send + 'static,
    {
        let vault = Arc::clone(&self.vault);
        tokio::task::spawn_blocking(move || f(&vault))
            .await
            .map_err(|e| {
                // A JoinError here almost always means the closure
                // panicked. Containing it as an error response (rather
                // than unwinding the runtime) is deliberate — but the
                // JoinError's Display embeds the panic payload, which
                // must not reach the client (PR #307 audit note). Log
                // the detail server-side; return a generic message.
                tracing::error!(error = %e, "tool handler panicked on the blocking pool");
                rmcp::model::ErrorData::internal_error(
                    "internal error while executing the tool".to_string(),
                    None,
                )
            })
    }
}

// `router = self.tool_router` so the wire dispatch uses the MERGED
// router built in `new()` (all four groups), not a static
// `Self::tool_router()` default.
#[tool_handler(router = self.tool_router)]
impl ServerHandler for CuadernoServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::default()
            .with_protocol_version(ProtocolVersion::default())
            .with_server_info(Implementation::new("cdno-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                // This is the only place a method can be stated to an agent that
                // arrives with no skill loaded. Tool descriptions carry mechanics —
                // argument shapes, side effects, caps — and cost tokens on every
                // listing; the *judgement* about which note type a thing is belongs
                // here, where it is paid once per session. The previous version
                // pointed at `docs/design.md`, which an agent over MCP cannot read.
                "Cuaderno is a vault manager for the Research Logbook Method (RLM). \
                Choosing the right note type matters more than calling the tool \
                correctly, so read this before creating anything.\n\n\
                The method runs two tracks over one vault. INQUIRY is open-ended: a \
                chronological log feeds evidence portfolios, which answer important \
                questions. OPERATIONS is delivery: project maps carry next actions, \
                stewardships hold perpetual responsibilities, and a commitments \
                register holds dated promises. The daily log feeds both.\n\n\
                The distinctions that are easy to get wrong:\n\
                - PROJECTS END, STEWARDSHIPS DO NOT. Health, finances, household, a \
                recurring service are stewardships. Active projects are capped \
                (default 5) to force prioritisation, so filing a perpetual \
                responsibility as a project permanently consumes a slot that can \
                never be freed by finishing.\n\
                - A QUESTION is open-ended and sits above projects — one question may \
                spawn several over time and outlives them. A PROJECT is bounded, with \
                a deliverable and usually a deadline. Either can exist without the \
                other.\n\
                - A PORTFOLIO is a dossier accumulating evidence for one question over \
                months. Do not organise inside it; file evidence as it arises. If you \
                are delivering an outcome rather than accumulating evidence, that is a \
                project.\n\
                - An ACTION is a bullet on a project map by default (add_action). \
                Promote it to its own note (promote_action) only once it becomes a \
                multi-day investigation with its own evidence. Three next actions per \
                project is a ceiling, not a backlog.\n\
                - A COMMITMENT is something someone else is counting on, with a date. \
                It is not a to-do list.\n\
                - The journal is APPEND-ONLY and is the source of truth. Never rewrite \
                a past entry; add a new one. When a mutable section changes, its \
                previous value is logged for you — that history is how a project's \
                evolution is traced.\n\n\
                Energy (deep/medium/light) tags an action's cognitive cost so \
                get_orientation can match work to how the person actually feels.\n\n\
                Tools split into context-gathering reads (get_orientation, \
                get_*_context, queries) and writes (append_to_log, \
                update_project_state, the create/complete pairs). Call \
                list_note_types before creating notes — it reports the vault's types \
                and schemas, including user-defined ones.",
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
