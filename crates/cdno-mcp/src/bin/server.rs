//! HTTP entry point for the `cdno-mcp-server` binary (GH #60/#61).
//!
//! Serves the same [`cdno_mcp::CuadernoServer`] as the stdio binary,
//! but over the MCP **Streamable HTTP** transport (rmcp's
//! `StreamableHttpService` mounted on axum at `/mcp`) so a remote
//! client — Claude's connector infrastructure, which reaches the
//! server from Anthropic's cloud for every surface including mobile —
//! can call the vault tools.
//!
//! # Security model (read before changing any default)
//!
//! This binary issues **no OAuth of its own**. The remote deployment
//! terminates OAuth 2.1 at an identity-aware proxy (Cloudflare Access
//! with Managed OAuth); the binary's job is origin-side validation of
//! the proxy-injected identity JWT (`Cf-Access-Jwt-Assertion`, GH
//! #302, [`cdno_mcp::access`]) — defence in depth so the tunnel is
//! never trusted alone. The deliberate consequences:
//!
//! - `--bind` defaults to `127.0.0.1:8787`, and non-loopback values
//!   are rejected at startup **unless** JWT validation is configured
//!   (`CDNO_ACCESS_TEAM_URL` + `CDNO_ACCESS_AUD`, which fail closed:
//!   the server won't start if the team JWKS can't be fetched).
//! - `--smoke` serves [`cdno_mcp::SmokeServer`], which holds **no
//!   vault handle**: infra bring-up is proven with the real binary
//!   and zero vault exposure.
//! - `--read-only` serves only the context-gathering read tools, for
//!   the initial exposure soak and permanently scoped deployments.
//!
//! **Be precise about what the interlock guarantees** (2026-07-05
//! security review): the enforced property is *"this process only
//! accepts connections arriving on its own loopback interface"* — it
//! is **not** "impossible to expose unauthenticated". Anything that
//! bridges the loopback port outward (an ad-hoc `cloudflared tunnel
//! --url http://localhost:8787` with no Access policy, an SSH
//! forward, in-container port games) exposes the vault regardless,
//! and the process cannot observe that. The operator contract is:
//! never bridge this port without the authenticating proxy in front;
//! GH #302's origin JWT check is the backstop that makes the bridge
//! itself safe. A startup warning restates this whenever the vault
//! (not `--smoke`) is being served.
//!
//! Transport guardrails carried by this binary regardless of auth:
//! rmcp's `allowed_hosts` check (DNS-rebinding protection; extended,
//! never replaced, by `--allowed-host`), a request-body size cap
//! (rmcp reads the raw body, so axum's extractor-gated default limit
//! does not apply), and an in-flight concurrency bound (tool handlers
//! run blocking domain calls on runtime workers until GH #303).
//!
//! # Index freshness
//!
//! Unlike a stdio session, this process is long-running while other
//! writers (the CLI, editors, sync tools) mutate the markdown
//! underneath it. Markdown is the source of truth and the index is a
//! cache, so the server re-runs the reconciliation pass on an
//! interval (`--reconcile-interval-secs`, default 300, 0 disables) as
//! the correctness backstop; a file watcher (GH #49) can later reduce
//! the latency but never replaces this loop.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use tracing_subscriber::EnvFilter;

use cdno_core::config::IgnoreSet;
use cdno_core::index::VaultIndex;
use cdno_core::reconcile::reconcile;
use cdno_core::store::VaultStore;
use cdno_mcp::bootstrap::open_vault;
use cdno_mcp::{CuadernoServer, SmokeServer};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

/// Request-body cap for `/mcp`. Tool arguments are JSON text — the
/// largest legitimate payloads are evidence-note bodies filed via
/// `file_to_portfolio`, comfortably under this. Anything bigger is
/// hostile or broken. (Enforced with `RequestBodyLimitLayer`, which
/// wraps the body itself; see the module docs for why axum's default
/// limit doesn't cover rmcp's raw-body path.)
const MAX_BODY_BYTES: usize = 1024 * 1024;

/// In-flight request bound. Tool handlers currently execute blocking
/// domain calls on runtime worker threads (GH #303), all ultimately
/// serialised on one SQLite connection — beyond a handful in flight,
/// extra requests only queue harder and buffer more bodies. Eight is
/// generous for a single-operator server.
const MAX_IN_FLIGHT: usize = 8;

/// Streamable HTTP MCP server for a Cuaderno vault.
///
/// Remote counterpart of the stdio `cdno-mcp` binary. Designed to run
/// behind an OAuth-terminating proxy; see the module docs for why it
/// refuses non-loopback binds until origin auth (GH #302) lands.
#[derive(Parser, Debug)]
#[command(name = "cdno-mcp-server", version, about)]
struct ServeArgs {
    /// Vault root. Falls back to the current working directory.
    #[arg(long, env = "CUADERNO_VAULT_PATH")]
    vault: Option<PathBuf>,

    /// Address to listen on. Non-loopback addresses are refused until
    /// origin-auth middleware exists (GH #302).
    #[arg(long, env = "CDNO_MCP_BIND", default_value = "127.0.0.1:8787")]
    bind: SocketAddr,

    /// Extra `Host` header values to accept, on top of rmcp's
    /// loopback defaults (DNS-rebinding protection). A public
    /// deployment adds its hostname here, e.g. `mcp.example.com`.
    #[arg(
        long = "allowed-host",
        env = "CDNO_MCP_ALLOWED_HOSTS",
        value_delimiter = ','
    )]
    allowed_hosts: Vec<String>,

    /// Serve the no-vault smoke-test handler (a single `echo` tool)
    /// instead of the vault tools. The vault is never opened.
    #[arg(long, conflicts_with = "read_only")]
    smoke: bool,

    /// Advertise only the context-gathering read tools; every
    /// mutating tool is absent from the dispatch table entirely.
    #[arg(long)]
    read_only: bool,

    /// Seconds between index-reconciliation passes (the correctness
    /// backstop against out-of-band edits). 0 disables the loop.
    #[arg(long, env = "CDNO_MCP_RECONCILE_INTERVAL_SECS", default_value_t = 300)]
    reconcile_interval_secs: u64,

    /// Seconds between git checkpoints of the vault (commit-if-dirty;
    /// the recoverability layer for remote writes — GH #303). 0
    /// disables. No-op with a warning when the vault is not a git
    /// repository or `git` is not on PATH.
    #[arg(
        long,
        env = "CDNO_MCP_GIT_CHECKPOINT_INTERVAL_SECS",
        default_value_t = 60
    )]
    git_checkpoint_interval_secs: u64,

    /// Cloudflare Access team URL (e.g.
    /// `https://<team>.cloudflareaccess.com`) — the JWT issuer and
    /// the JWKS host. Setting this (with `--access-aud`) activates
    /// origin JWT validation and lifts the non-loopback interlock.
    #[arg(long, env = "CDNO_ACCESS_TEAM_URL", requires = "access_aud")]
    access_team_url: Option<String>,

    /// The Access application's AUD tag (expected `aud` claim).
    #[arg(long, env = "CDNO_ACCESS_AUD", requires = "access_team_url")]
    access_aud: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = ServeArgs::parse();

    // Origin authentication (GH #302): both CDNO_ACCESS_* values set
    // → build the verifier (fail-closed: construction performs the
    // initial JWKS fetch and errors if it can't). clap's `requires`
    // pairing rejects setting only one of the two.
    let verifier = match (&args.access_team_url, &args.access_aud) {
        (Some(team_url), Some(aud)) => Some(
            cdno_mcp::access::JwtVerifier::new(team_url, aud)
                .await
                .context("initialising Access JWT verification (is the team URL reachable?)")?,
        ),
        _ => None,
    };

    // Safety interlock (see module docs): a bare unauthenticated
    // listener must be impossible to expose by accident. With the
    // GH #302 verifier active every request is authenticated at the
    // origin, so non-loopback binds (e.g. 0.0.0.0 inside a container)
    // become legitimate.
    //
    // NOTE (future-proofing, from the 2026-07-05 security review):
    // this validates `args.bind` — the address WE are about to bind —
    // which is sound only because this binary always creates its own
    // listener below. If socket activation / fd passing is ever
    // added, the check must move to the *actual* socket's local
    // address or it is silently bypassed.
    if verifier.is_none() && !args.bind.ip().is_loopback() {
        bail!(
            "refusing to bind non-loopback address {}: origin authentication is not \
             configured. Set CDNO_ACCESS_TEAM_URL and CDNO_ACCESS_AUD (GH #302) to \
             enable Access-JWT validation, or bind a loopback address behind an \
             authenticating proxy.",
            args.bind
        );
    }

    // rmcp validates the `Host` header against loopback names by
    // default; extend (never replace) that list so the interlock above
    // and the rebinding protection stay coherent.
    let mut http_config = StreamableHttpServerConfig::default()
        // Stateless JSON mode: every request is self-contained and the
        // response is plain `application/json` — no SSE framing to
        // shepherd through a tunnel, nothing session-shaped to expire.
        .with_stateful_mode(false)
        .with_json_response(true);
    http_config
        .allowed_hosts
        .extend(args.allowed_hosts.iter().cloned());

    let router = if args.smoke {
        tracing::info!("smoke mode: serving the echo tool only; the vault is not opened");
        mcp_router(SmokeServer::new(), http_config)
    } else {
        if verifier.is_none() {
            // The interlock only guards OUR bind address; restate the
            // operator contract whenever real vault data is served
            // unauthenticated (see module docs — an unauthenticated
            // tunnel to this port would expose the vault and we
            // cannot detect it).
            tracing::warn!(
                "serving vault tools on loopback WITHOUT origin authentication (GH #302): \
                 never bridge this port (tunnel, SSH forward, container publish) without \
                 an authenticating proxy in front"
            );
        }

        let root = vault_root(args.vault.clone())?;
        tracing::info!(vault_root = %root.display(), "starting cdno-mcp-server");

        let opened = open_vault(&root).inspect_err(|e| {
            tracing::error!(error = %e, vault_root = %root.display(), "failed to open vault");
        })?;

        if args.reconcile_interval_secs > 0 {
            spawn_reconcile_loop(
                opened.store.clone(),
                opened.index.clone(),
                opened.ignore.clone(),
                Duration::from_secs(args.reconcile_interval_secs),
            );
        } else {
            tracing::warn!(
                "periodic reconciliation disabled (--reconcile-interval-secs 0); \
                 out-of-band edits will not appear in the index until restart"
            );
        }

        if args.git_checkpoint_interval_secs > 0 {
            spawn_git_checkpoint_loop(
                root.clone(),
                Duration::from_secs(args.git_checkpoint_interval_secs),
            );
        } else {
            tracing::warn!(
                "git checkpoints disabled (--git-checkpoint-interval-secs 0); \
                 remote writes will have no commit-level recovery trail"
            );
        }

        let vault = Arc::new(opened.vault);
        let server = if args.read_only {
            tracing::info!("read-only mode: mutating tools are not registered");
            CuadernoServer::read_only(vault)
        } else {
            CuadernoServer::new(vault)
        };
        tracing::info!(
            tools = server.advertised_tools().len(),
            read_only = args.read_only,
            "vault opened; serving cdno-mcp tools over streamable HTTP"
        );
        mcp_router(server, http_config)
    };

    // Auth outermost: an unauthenticated request is rejected before
    // it can consume body-buffer or concurrency budget, and before
    // rmcp ever parses it. (`.layer` wraps everything added earlier.)
    let router = match &verifier {
        Some(v) => router.layer(axum::middleware::from_fn_with_state(
            v.clone(),
            cdno_mcp::access::require_access_jwt,
        )),
        None => router,
    };

    let listener = tokio::net::TcpListener::bind(args.bind)
        .await
        .with_context(|| format!("binding {}", args.bind))?;
    tracing::info!(bind = %args.bind, auth = verifier.is_some(), "listening (endpoint: /mcp)");

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            // ctrl-c (or container SIGINT) → stop accepting and let
            // axum drain in-flight requests to completion. We
            // deliberately do NOT cancel rmcp's CancellationToken:
            // in stateless mode there are no sessions to tear down,
            // and cancelling mid-request would turn every in-flight
            // call into a 500 instead of letting it finish (PR #304
            // review, correctness finding 4).
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutdown signal received; draining in-flight requests");
        })
        .await
        .context("HTTP server exited with an error")?;

    tracing::info!("cdno-mcp-server stopped");
    Ok(())
}

/// Mount an MCP handler as a Streamable HTTP tower service at `/mcp`,
/// wrapped in the transport guardrails (body cap, concurrency bound —
/// see the constants above).
///
/// The handler is `Clone` (both [`CuadernoServer`] and [`SmokeServer`]
/// are cheap to clone); in stateless mode the factory is invoked per
/// request.
fn mcp_router<S>(handler: S, config: StreamableHttpServerConfig) -> axum::Router
where
    S: rmcp::Service<rmcp::RoleServer> + Clone + Send + 'static,
{
    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(NeverSessionManager::default()),
        config,
    );
    axum::Router::new().route_service("/mcp", service).layer(
        tower::ServiceBuilder::new()
            .layer(tower::limit::ConcurrencyLimitLayer::new(MAX_IN_FLIGHT))
            .layer(tower_http::limit::RequestBodyLimitLayer::new(
                MAX_BODY_BYTES,
            )),
    )
}

/// Resolve the vault root: `--vault` / `CUADERNO_VAULT_PATH`, else cwd.
fn vault_root(flag: Option<PathBuf>) -> Result<PathBuf> {
    match flag {
        Some(p) => Ok(p),
        None => {
            std::env::current_dir().context("could not determine the current working directory")
        }
    }
}

/// Periodic index reconciliation — the correctness backstop for a
/// long-running server whose markdown is mutated by other writers
/// (CLI, editors, sync). Runs the synchronous pass on the blocking
/// pool so tool requests keep flowing while it scans.
///
/// Known benign race (PR #304 review, correctness finding 2): the
/// pass reads file state *before* taking the per-note write lock, so
/// a tool-call write landing in that window can be shadowed by a
/// stale index row — which the *next* pass heals (mtime mismatch).
/// Worst case is stale search results for one interval; markdown is
/// never touched by reconciliation, so no data is at risk.
fn spawn_reconcile_loop(
    store: Arc<dyn VaultStore>,
    index: Arc<dyn VaultIndex>,
    ignore: Arc<IgnoreSet>,
    every: Duration,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick fires immediately; skip it — `Vault::new`
        // just reconciled at open.
        interval.tick().await;
        loop {
            interval.tick().await;
            let (s, i, g) = (store.clone(), index.clone(), ignore.clone());
            match tokio::task::spawn_blocking(move || reconcile(&s, &i, &g)).await {
                Ok(Ok(report)) => {
                    let changed = report.added + report.updated + report.removed;
                    if changed > 0 || !report.errors.is_empty() {
                        tracing::info!(
                            scanned = report.scanned,
                            added = report.added,
                            updated = report.updated,
                            removed = report.removed,
                            errors = report.errors.len(),
                            "periodic reconciliation applied out-of-band changes"
                        );
                    } else {
                        tracing::debug!(
                            scanned = report.scanned,
                            "periodic reconciliation: no drift"
                        );
                    }
                }
                Ok(Err(e)) => tracing::warn!(error = %e, "periodic reconciliation failed"),
                Err(e) => tracing::warn!(error = %e, "periodic reconciliation task panicked"),
            }
        }
    });
}

/// Periodic git checkpoint of the vault — the recoverability layer
/// for remote writes (GH #303): every mutation a prompt-injected or
/// buggy session could make becomes diffable and revertible, which
/// is the only meaningful damage limit once write tools are exposed.
///
/// Deliberately a commit-if-dirty *sweep*, not a per-tool-call hook:
/// it needs zero changes to the 42 handlers, it also captures
/// out-of-band edits (CLI, editors, sync) into the audit trail, and
/// attribution still exists in-content because every cdno write
/// already logs a line to the daily note. Runs `git` as a
/// subprocess on the blocking pool; identity is forced per-commit so
/// no global git config is required in the container.
///
/// First pass runs immediately (captures drift that accumulated
/// while the server was down); disabled with a warning when the
/// vault is not a git repo or `git` is unavailable.
fn spawn_git_checkpoint_loop(root: std::path::PathBuf, every: Duration) {
    if !root.join(".git").exists() {
        tracing::warn!(
            vault_root = %root.display(),
            "vault is not a git repository — checkpoints disabled; \
             remote writes will have NO commit-level recovery trail"
        );
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let repo = root.clone();
            let done = tokio::task::spawn_blocking(move || git_checkpoint(&repo)).await;
            match done {
                Ok(Ok(Some(summary))) => tracing::info!(%summary, "git checkpoint committed"),
                Ok(Ok(None)) => tracing::debug!("git checkpoint: vault clean"),
                Ok(Err(e)) => {
                    // Missing binary / repo corruption: warn and stop
                    // rather than log-spamming every tick — the
                    // operator must intervene either way.
                    tracing::warn!(error = %e, "git checkpoint failed; stopping checkpoint loop");
                    return;
                }
                Err(e) => tracing::warn!(error = %e, "git checkpoint task panicked"),
            }
        }
    });
}

/// One checkpoint pass: commit everything if the tree is dirty.
/// Returns the one-line commit summary, or `None` when clean.
fn git_checkpoint(root: &std::path::Path) -> anyhow::Result<Option<String>> {
    let git = |args: &[&str]| -> anyhow::Result<std::process::Output> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .context("running git (is it installed in this environment?)")?;
        Ok(out)
    };

    let status = git(&["status", "--porcelain"])?;
    anyhow::ensure!(
        status.status.success(),
        "git status failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    if status.stdout.is_empty() {
        return Ok(None);
    }
    let dirty_paths = status.stdout.iter().filter(|&&b| b == b'\n').count();

    let add = git(&["add", "-A"])?;
    anyhow::ensure!(
        add.status.success(),
        "git add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let message = format!("cdno-mcp checkpoint ({dirty_paths} path(s))");
    let commit = git(&[
        "-c",
        "user.name=cdno-mcp",
        "-c",
        "user.email=cdno-mcp@localhost",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        &message,
    ])?;
    anyhow::ensure!(
        commit.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );
    Ok(Some(message))
}

/// Same tracing setup as the stdio binary: stderr, `info` default,
/// `RUST_LOG` override. stdout carries nothing here, but containers
/// conventionally read logs from stderr and consistency with the
/// stdio binary keeps operational muscle memory intact.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .compact()
        .try_init();
}
