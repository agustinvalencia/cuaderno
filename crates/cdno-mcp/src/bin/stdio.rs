//! Stdio entry point for the `cdno-mcp` binary. Reads the
//! `CUADERNO_VAULT_PATH` environment variable (or falls back to the
//! current working directory), opens the vault, and serves the
//! [`cdno_mcp::CuadernoServer`] over stdio — the transport Claude
//! Desktop and Claude Code use for local MCP servers.
//!
//! The file name (`stdio.rs`) describes the transport; the binary's
//! user-facing name (`cdno-mcp`) is set in `Cargo.toml`'s `[[bin]]`
//! table. The HTTP transport lives in `bin/server.rs` as the
//! `cdno-mcp-server` binary (GH #60/#61).
//!
//! # Logging
//!
//! Structured logs go to **stderr**, never stdout (stdout is the
//! JSON-RPC channel — anything written there corrupts the
//! protocol). Filter verbosity at runtime via `RUST_LOG`, e.g.
//! `RUST_LOG=cdno_mcp=debug,cdno_domain=info` to debug a real
//! Claude session without rebuilding. Default level is `info` so a
//! freshly-launched server logs its vault path and any startup
//! failure without further configuration.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use cdno_mcp::CuadernoServer;
use cdno_mcp::bootstrap::open_vault;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let root = vault_root_from_env()?;
    tracing::info!(vault_root = %root.display(), "starting cdno-mcp");

    // A stdio session is short-lived, so only the `vault` is kept;
    // the store/index/ignore handles (used by the HTTP binary's
    // reconciliation loop) are dropped.
    let opened = open_vault(&root).inspect_err(|e| {
        tracing::error!(error = %e, vault_root = %root.display(), "failed to open vault");
    })?;

    let server = CuadernoServer::new(Arc::new(opened.vault));
    // Derive the count from the merged router rather than hardcoding it,
    // so the startup log can't drift out of sync as tools are added.
    tracing::info!(
        tools = server.advertised_tools().len(),
        "vault opened; serving cdno-mcp tools over stdio"
    );

    // `ServiceExt::serve(transport)` performs the MCP init handshake,
    // routes incoming `tools/call` requests through the
    // `#[tool_router]` table, and runs until the client disconnects.
    let running = server.serve(stdio()).await.inspect_err(|e| {
        tracing::error!(error = %e, "failed to start MCP service over stdio");
    })?;

    let result = running.waiting().await;
    if let Err(ref e) = result {
        tracing::error!(error = %e, "MCP service exited with an error");
    } else {
        tracing::info!("MCP client disconnected; shutting down");
    }
    result.context("MCP service exited with an error")?;
    Ok(())
}

/// Initialise the `tracing` subscriber: write to **stderr** (stdout
/// is the JSON-RPC channel and must not be written to from anywhere
/// else), default to `info`, allow `RUST_LOG` to override.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        // No ANSI colour codes — these go to log files / Claude
        // Desktop's MCP log pane and the escapes would be noise.
        .with_ansi(false)
        // Compact format is easier to scan in a log pane than the
        // full pretty form, but still includes timestamp + level.
        .compact()
        .try_init();
}

/// Resolve the vault root from `CUADERNO_VAULT_PATH`, falling back
/// to the current working directory.
fn vault_root_from_env() -> Result<PathBuf> {
    match std::env::var_os("CUADERNO_VAULT_PATH") {
        Some(p) => Ok(PathBuf::from(p)),
        None => {
            std::env::current_dir().context("could not determine the current working directory")
        }
    }
}
