//! cdno-mcp: MCP (Model Context Protocol) server for Cuaderno vaults.
//!
//! Built on the official [`rmcp`] SDK (`modelcontextprotocol/rust-sdk`).
//! `rmcp` provides the JSON-RPC framing, the initialisation
//! handshake, the `tools/list` machinery, and the dispatch table —
//! we only define the typed tools and how they call into
//! [`cdno_domain::Vault`].
//!
//! # Layout
//!
//! - [`dto`] — JSON-Schema-friendly mirror types for domain summaries
//!   (`PortfolioSummary`, `CommitmentEntry`, …). Each DTO implements
//!   `From<DomainType>` so handlers convert in one explicit line at
//!   the layer boundary. Lives here (not in `cdno-domain`) so the
//!   domain crate stays free of the `schemars` dependency.
//! - [`server`] — the [`server::CuadernoServer`] struct holds an
//!   `Arc<Vault>` and exposes each design-§11 tool as a method
//!   annotated with rmcp's `#[tool]`. The `#[tool_router]` macro
//!   builds the dispatch table; the `ServerHandler` impl wires it
//!   into the MCP protocol surface.
//!
//! # Concurrency note
//!
//! `Vault` is synchronous (see `docs/implementation-plan.md` §4).
//! Tool methods are async because `rmcp::ServerHandler` is async, and
//! each tool calls `vault.method()` directly on the runtime worker —
//! **a recorded decision, not an oversight** (GH #303 tracks the
//! hardening). For stdio this is moot (one request at a time). For
//! the HTTP binary it means a blocking domain call occupies a worker
//! thread per in-flight request; acceptable for the single-operator
//! deployment because `cdno-mcp-server` caps concurrent requests at
//! the transport layer, and its reconciliation loop — the only
//! long-running scan — does run on `spawn_blocking`. Before any
//! multi-user or high-concurrency use, #303 moves the per-tool domain
//! calls onto the blocking pool too.

pub mod access;
pub mod bootstrap;
pub mod checkpoint;
mod context;
mod creation;
pub mod dto;
pub mod input;
mod lifecycle;
mod operations;
pub mod server;
pub mod smoke;
mod util;

pub use server::CuadernoServer;
pub use smoke::SmokeServer;
