//! Stdio entry point for the `cdno-mcp` binary. Reads the
//! `CUADERNO_VAULT_PATH` environment variable (or falls back to the
//! current working directory), opens the vault, and serves the
//! [`cdno_mcp::CuadernoServer`] over stdio — the transport Claude
//! Desktop and Claude Code use for local MCP servers. The index is
//! opened at `CUADERNO_INDEX_PATH` when that is set (#998), else at
//! `.cuaderno/index.db`.
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
//!
//! # Index freshness
//!
//! A stdio server lives as long as the client session that spawned it,
//! often hours, while other writers (the CLI, an editor, file sync, `git
//! pull`) change the notes under it. So it runs the HTTP server's sweep,
//! through the same [`IndexHealth::start`](cdno_mcp::index_health::IndexHealth::start)
//! (#1002): every `CDNO_MCP_RECONCILE_INTERVAL_SECS` seconds (300 by
//! default, `0` turns it off) it reconciles the index with the notes, then
//! checks the index file and repairs damage, with the recurrence stop of
//! #526. The variable is the HTTP server's own; a client's config passes
//! environment more readily than flags.
//!
//! # Threads and shutdown
//!
//! A desktop client may spawn several of these, so the runtime is built by
//! hand with a small, fixed budget ([`WORKER_THREADS`],
//! [`MAX_BLOCKING_THREADS`]) instead of `#[tokio::main]`'s worker per core
//! and 512 blocking threads. When the client disconnects (stdin closes)
//! the sweep loop is stopped, and the runtime is shut down with a bounded
//! wait ([`SHUTDOWN_GRACE`]): a sweep or tool call that is running gets
//! that long to finish, and the process then exits whether or not it has.
//! A sweep cut short loses nothing: its index writes are SQLite
//! transactions, which roll back, the OS releases the vault lock with the
//! process, and the next open reconciles. A tool call still running after
//! the grace (a client that hung up mid-call, on a call that takes
//! seconds) is cut short as if the process had been killed, which a
//! client may do anyway; its reply could not reach anyone.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use cdno_mcp::CuadernoServer;
use cdno_mcp::bootstrap::open_vault_checked;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

/// Runtime worker threads. The async side of a stdio server is one
/// client's JSON-RPC stream; the domain work runs on the blocking pool.
/// Two keep one stalled task from stalling the stream.
const WORKER_THREADS: usize = 2;

/// Ceiling on the blocking pool: the stdin reader (which holds one thread
/// for the life of the process), stdout writes, one sweep, and a few tool
/// calls a client sent at once. Beyond that work queues rather than
/// becoming threads.
const MAX_BLOCKING_THREADS: usize = 8;

/// The blocking pool must hold the stdin reader, a sweep and at least two
/// tool calls, or a sweep could keep every tool call waiting.
const _: () = assert!(
    MAX_BLOCKING_THREADS >= 4,
    "the blocking pool must hold the stdin reader, a sweep and tool calls"
);

/// How long a disconnected server waits for a running sweep or tool call
/// before it exits anyway.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

fn build_runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(WORKER_THREADS)
        .max_blocking_threads(MAX_BLOCKING_THREADS)
        .enable_all()
        .build()
        .context("building the tokio runtime")
}

fn main() -> Result<()> {
    init_tracing();
    let runtime = build_runtime()?;
    let result = runtime.block_on(serve());
    // Bounded, unlike dropping the runtime, which waits for every blocking
    // task however long it runs: a sweep over a large vault, or one waiting
    // on the vault lock, would otherwise hold the process after the client
    // has gone.
    runtime.shutdown_timeout(SHUTDOWN_GRACE);
    result
}

async fn serve() -> Result<()> {
    let root = vault_root_from_env()?;
    tracing::info!(vault_root = %root.display(), "starting cdno-mcp");
    // Read before the vault is opened, so a bad value stops the server
    // before it touches anything.
    let interval_secs =
        cdno_mcp::index_health::sweep_interval_secs_from_env().map_err(|e| anyhow!(e))?;

    // Checked at open, like the HTTP server, at the index location the
    // override names (#998). The store/index/scope handles go to the
    // index-health supervisor, which runs the sweep loop.
    let index_override = cdno_core::paths::index_override_from_env();
    let opened = open_vault_checked(&root, index_override.as_deref()).inspect_err(|e| {
        tracing::error!(error = %e, vault_root = %root.display(), "failed to open vault");
    })?;
    tracing::info!(index_path = %opened.index_path.display(), "index opened");

    // A vault on a newer minor method version is said so once, at start
    // (RFC 0006 §8.4).
    cdno_mcp::startup::log_method_notice(&opened.vault);

    // The HTTP server's loop (#1002): sweeps on the interval, and a tool
    // call that meets a damaged index starts a repair (#526).
    let health = Arc::new(cdno_mcp::index_health::IndexHealth::from_opened(&opened));
    let sweeps = health.start(interval_secs);
    let server =
        CuadernoServer::new(Arc::new(opened.vault), root.clone()).with_index_health(health);
    // Derive the count from the merged router rather than hardcoding it,
    // so the startup log can't fall out of sync as tools are added.
    tracing::info!(
        tools = server.advertised_tools().len(),
        "vault opened; serving cdno-mcp tools over stdio"
    );
    // See #309: timestamps are written in process-local time; log the
    // resolved offset so a UTC fallback is visible from the boot logs.
    cdno_mcp::startup::log_local_time();

    // `ServiceExt::serve(transport)` performs the MCP init handshake,
    // routes incoming `tools/call` requests through the
    // `#[tool_router]` table, and runs until the client disconnects.
    let running = server.serve(stdio()).await.inspect_err(|e| {
        tracing::error!(error = %e, "failed to start MCP service over stdio");
    })?;

    let result = running.waiting().await;
    // No sweep starts after the client has gone; one already running is
    // left to the shutdown grace in `main`.
    sweeps.abort();
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
