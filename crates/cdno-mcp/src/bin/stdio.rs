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
//! often hours, while notes change under it outside cuaderno (an editor, a
//! file sync, `git pull`). So it runs the HTTP server's sweep, through the
//! same [`IndexHealth::start`](cdno_mcp::index_health::IndexHealth::start)
//! (#1002): every `CDNO_MCP_RECONCILE_INTERVAL_SECS` seconds (300 by
//! default, `0` turns it off) it reconciles the index with the notes, then
//! checks the index file and repairs damage, with the recurrence stop of
//! #526. The variable is the HTTP server's own; a client's config passes
//! environment more readily than flags. Each session runs its own sweep.
//!
//! # Threads
//!
//! A desktop client may spawn several of these, so the runtime is built by
//! hand with a small, fixed budget ([`WORKER_THREADS`],
//! [`MAX_BLOCKING_THREADS`]) instead of `#[tokio::main]`'s worker per core
//! and 512 blocking threads, and at most [`MAX_TOOL_CALLS`] tool calls run
//! their blocking work at once, so the stdin reader, a stdout write and a
//! sweep always find a thread.
//!
//! # Shutdown
//!
//! When the client disconnects (stdin closes), in this order:
//!
//! 1. The read that meets end of input stops the sweep loop at once
//!    ([`EofSignal`]), so no new sweep starts while the rest drains. A
//!    sweep already running carries on to step 4.
//! 2. rmcp sends the replies of handlers still running, for up to 5 s (its
//!    own drain), then `waiting()` returns. With nothing in flight this is
//!    immediate.
//! 3. The server waits for every tool call's blocking work to return
//!    ([`ToolCalls::idle`](cdno_mcp::tool_calls::ToolCalls::idle)), whether
//!    or not rmcp still awaits it: a write cut inside its commit, between
//!    two file writes, would leave the notes half-changed (a note moved
//!    without its log line, a rename that rewrote only some references),
//!    which no reconcile repairs. This cannot hang as an unbounded wait for
//!    every blocking task could: a call gives up on the vault lock after
//!    5 s, each git probe a rename makes is killed after 2 s (its output
//!    drained for at most 1 s more), and then it does only its own work,
//!    normally milliseconds. No call arrives after
//!    end of input. [`TOOL_CALL_DRAIN_LIMIT`] is a backstop for a call that
//!    hangs regardless; past it the server logs an error and exits.
//! 4. The runtime shuts down, waiting at most [`SWEEP_GRACE`] for what is
//!    left: a sweep, or the index-only work of `index_status` or
//!    `rebuild_index`. Cut short, that loses nothing: its index writes are
//!    SQLite transactions, which roll back, the OS releases the vault lock
//!    with the process, and the next open reconciles.
//!
//! So an idle server exits at once; one with only a sweep running exits
//! within 5 s; one with a write running exits once the write has returned
//! (plus up to 5 s for a sweep), and never later than 5 + 120 + 5 s.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use cdno_mcp::CuadernoServer;
use cdno_mcp::bootstrap::open_vault_checked;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tokio::io::{AsyncRead, ReadBuf};
use tracing_subscriber::EnvFilter;

/// Runtime worker threads. The async side of a stdio server is one
/// client's JSON-RPC stream; the domain work runs on the blocking pool.
/// Two keep one stalled task from stalling the stream.
const WORKER_THREADS: usize = 2;

/// How many tool calls run their blocking work at once; the others wait
/// their turn. A client rarely sends more than one or two at a time.
const MAX_TOOL_CALLS: usize = 4;

/// Ceiling on the blocking pool. Beyond it work queues rather than
/// becoming threads.
const MAX_BLOCKING_THREADS: usize = 8;

/// Every user of the blocking pool, at its most: [`MAX_TOOL_CALLS`], the
/// stdin reader (tokio reads stdin on a blocking thread, one read at a
/// time, for the life of the process), a stdout write (one at a time), and
/// one sweep (the loop runs one at a time). With a smaller pool, a burst
/// of tool calls waiting on the vault lock would queue the next stdin read
/// and every reply behind them.
const _: () = assert!(
    MAX_BLOCKING_THREADS >= MAX_TOOL_CALLS + 3,
    "the blocking pool must hold every tool call plus the stdin reader, a stdout write and a sweep"
);

/// The backstop on step 3 of the shutdown: how long a disconnected server
/// waits for tool calls still running before it exits regardless. Far
/// above what a call takes (see the module docs); reaching it means a call
/// hung.
const TOOL_CALL_DRAIN_LIMIT: Duration = Duration::from_secs(120);

/// Step 4 of the shutdown: how long the runtime waits for a sweep (or
/// index-only tool work) still running before the process exits anyway.
/// It starts after rmcp's drain and the tool calls' drain have finished.
const SWEEP_GRACE: Duration = Duration::from_secs(5);

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
    // on the vault lock note after note, would otherwise hold the process
    // after the client has gone. Tool calls have drained by now.
    runtime.shutdown_timeout(SWEEP_GRACE);
    result
}

async fn serve() -> Result<()> {
    let root = vault_root_from_env()?;
    tracing::info!(vault_root = %root.display(), "starting cdno-mcp");
    // Read before the vault is opened, so a bad value stops the server
    // before it touches anything.
    let interval_secs = cdno_mcp::index_health::sweep_interval_secs_from_env()
        .inspect_err(|e| tracing::error!(error = %e, "invalid sweep interval"))
        .map_err(|e| anyhow!(e))?;

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
    let server = CuadernoServer::new(Arc::new(opened.vault), root.clone())
        .with_index_health(health)
        .with_tool_call_limit(MAX_TOOL_CALLS);
    let tool_calls = server.tool_calls();
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
    let (stdin, stdout) = stdio();
    let stdin = EofSignal {
        inner: stdin,
        on_eof: Some(sweeps.abort_handle()),
    };
    let running = server.serve((stdin, stdout)).await.inspect_err(|e| {
        tracing::error!(error = %e, "failed to start MCP service over stdio");
    })?;

    let result = running.waiting().await;
    // Normally stopped already, at end of input (step 1); this covers the
    // service ending for any other reason.
    sweeps.abort();
    if let Err(ref e) = result {
        tracing::error!(error = %e, "MCP service exited with an error");
    } else {
        tracing::info!("MCP client disconnected; shutting down");
    }

    // Step 3: the writes still running finish before the runtime goes.
    let in_flight = tool_calls.in_flight();
    if in_flight > 0 {
        tracing::info!(in_flight, "waiting for tool calls still running to finish");
    }
    if tokio::time::timeout(TOOL_CALL_DRAIN_LIMIT, tool_calls.idle())
        .await
        .is_err()
    {
        tracing::error!(
            in_flight = tool_calls.in_flight(),
            limit_secs = TOOL_CALL_DRAIN_LIMIT.as_secs(),
            "tool calls still running after the drain limit; exiting regardless, so a write \
             among them may be incomplete: run `cdno lint`"
        );
    }
    result.context("MCP service exited with an error")?;
    Ok(())
}

/// Stdin, stopping the sweep loop as soon as a read meets the end of input
/// (or fails): the first moment the disconnect can be seen, before rmcp,
/// which sees the same end of input next, begins its drain.
struct EofSignal<R> {
    inner: R,
    on_eof: Option<tokio::task::AbortHandle>,
}

impl<R: AsyncRead + Unpin> AsyncRead for EofSignal<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let polled = Pin::new(&mut self.inner).poll_read(cx, buf);
        let ended = match &polled {
            Poll::Ready(Ok(())) => buf.filled().len() == before && buf.remaining() > 0,
            Poll::Ready(Err(_)) => true,
            Poll::Pending => false,
        };
        if ended && let Some(sweeps) = self.on_eof.take() {
            tracing::info!("stdin closed; the sweep loop is stopped");
            sweeps.abort();
        }
        polled
    }
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
