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
//! every handler routes its domain calls through
//! `CuadernoServer::with_vault`, which runs the synchronous work on
//! tokio's blocking pool via `spawn_blocking` (GH #303) — one blocking
//! task per request, so async workers never block on disk/SQLite and
//! slow calls queue on the blocking pool instead of starving the
//! accept loop. For stdio this is belt-and-braces (one request at a
//! time); for the HTTP binary it's what keeps the runtime responsive
//! under concurrent requests. The transport-level concurrency cap in
//! `cdno-mcp-server` remains as backpressure, and its reconciliation
//! loop — the only long-running scan — also runs on `spawn_blocking`.

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
pub mod startup;
mod util;

pub use server::CuadernoServer;
pub use smoke::SmokeServer;
