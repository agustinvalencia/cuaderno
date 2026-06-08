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
//! Tool methods are async because `rmcp::ServerHandler` is async.
//! Each tool calls `vault.method()` directly — fine for stdio, which
//! handles one request at a time. The HTTP transport (later phase)
//! should wrap each call in `tokio::task::spawn_blocking` so the
//! event loop never stalls on disk I/O.

pub mod dto;
pub mod input;
mod lifecycle;
pub mod server;
mod util;

pub use server::CuadernoServer;
