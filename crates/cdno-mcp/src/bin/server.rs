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
//! This binary implements **no authentication itself**. The remote
//! deployment terminates OAuth 2.1 at an identity-aware proxy
//! (Cloudflare Access with Managed OAuth), and origin-side validation
//! of the proxy-injected identity JWT arrives with GH #302. Until
//! that middleware exists, this binary **refuses to bind anything but
//! loopback** — it must be impossible to expose an unauthenticated
//! vault listener by accident. Three deliberate consequences:
//!
//! - `--bind` defaults to `127.0.0.1:8787` and non-loopback values
//!   are rejected at startup (the GH #302 middleware will lift this
//!   when — and only when — JWT validation is configured).
//! - `--smoke` serves [`cdno_mcp::SmokeServer`], which holds **no
//!   vault handle**: infra bring-up is proven with the real binary
//!   and zero vault exposure.
//! - `--read-only` serves only the context-gathering read tools, for
//!   the initial exposure soak and permanently scoped deployments.
//!
//! rmcp's own `allowed_hosts` check (DNS-rebinding protection) stays
//! at its loopback default unless `--allowed-host` extends it.
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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use tracing_subscriber::EnvFilter;

use cdno_core::config::{IgnoreSet, VaultConfig};
use cdno_core::index::{SqliteIndex, VaultIndex};
use cdno_core::paths;
use cdno_core::reconcile::reconcile;
use cdno_core::store::{FsVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_mcp::{CuadernoServer, SmokeServer};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

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
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = ServeArgs::parse();

    // Safety interlock (see module docs): a bare unauthenticated
    // listener must be impossible to expose by accident. GH #302
    // replaces this hard refusal with "refused unless JWT validation
    // is configured".
    if !args.bind.ip().is_loopback() {
        bail!(
            "refusing to bind non-loopback address {}: cdno-mcp-server has no \
             origin authentication yet (GH #302). Bind a loopback address and put \
             an authenticating proxy in front, or wait for the Access-JWT middleware.",
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
    let cancel = http_config.cancellation_token.clone();

    let router = if args.smoke {
        tracing::info!("smoke mode: serving the echo tool only; the vault is not opened");
        mcp_router(SmokeServer::new(), http_config)
    } else {
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

    let listener = tokio::net::TcpListener::bind(args.bind)
        .await
        .with_context(|| format!("binding {}", args.bind))?;
    tracing::info!(bind = %args.bind, "listening (endpoint: /mcp)");

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            // ctrl-c (or container SIGINT) → cancel rmcp's sessions,
            // then let axum drain in-flight requests.
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutdown signal received");
            cancel.cancel();
        })
        .await
        .context("HTTP server exited with an error")?;

    tracing::info!("cdno-mcp-server stopped");
    Ok(())
}

/// Mount an MCP handler as a Streamable HTTP tower service at `/mcp`.
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
    axum::Router::new().route_service("/mcp", service)
}

/// Everything `open_vault` produced. The store/index/ignore handles
/// exist so the reconciliation loop can re-run the pass that
/// `Vault::new` performs once at open — `Vault` deliberately does not
/// re-expose them.
struct OpenedVault {
    vault: Vault,
    store: Arc<dyn VaultStore>,
    index: Arc<dyn VaultIndex>,
    ignore: Arc<IgnoreSet>,
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

/// Open a vault at `root`. Mirror of the stdio binary's `open_vault`
/// (itself a mirror of `cdno_cli::bootstrap::open_vault`), except it
/// also hands back the store/index/ignore handles for the
/// reconciliation loop.
fn open_vault(root: &Path) -> Result<OpenedVault> {
    let cuaderno_dir = root.join(paths::CUADERNO_DIR);
    if !cuaderno_dir.is_dir() {
        bail!(
            "no Cuaderno vault at {} (looked for `.cuaderno/`).\n\
             Hint: run `cdno init {}` to scaffold one, or set \
             `CUADERNO_VAULT_PATH` to point at an existing vault.",
            root.display(),
            root.display(),
        );
    }

    let config = VaultConfig::load(root)
        .with_context(|| format!("loading {}", root.join(paths::CONFIG_FILE).display()))?;
    let ignore = Arc::new(config.ignore_set().context("compiling the ignore set")?);
    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(root));
    let index: Arc<dyn VaultIndex> = Arc::new(
        SqliteIndex::open(root.join(paths::INDEX_DB))
            .with_context(|| format!("opening {}", root.join(paths::INDEX_DB).display()))?,
    );
    let (vault, report) = Vault::new(store.clone(), index.clone(), config)
        .context("constructing vault and reconciling index")?;
    tracing::debug!(
        scanned = report.scanned,
        added = report.added,
        updated = report.updated,
        removed = report.removed,
        errors = report.errors.len(),
        "startup reconciliation complete",
    );
    Ok(OpenedVault {
        vault,
        store,
        index,
        ignore,
    })
}

/// Periodic index reconciliation — the correctness backstop for a
/// long-running server whose markdown is mutated by other writers
/// (CLI, editors, sync). Runs the synchronous pass on the blocking
/// pool so tool requests keep flowing while it scans.
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
