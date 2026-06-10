//! Stdio entry point for the `cdno-mcp` binary. Reads the
//! `CUADERNO_VAULT_PATH` environment variable (or falls back to the
//! current working directory), opens the vault, and serves the
//! [`cdno_mcp::CuadernoServer`] over stdio — the transport Claude
//! Desktop and Claude Code use for local MCP servers.
//!
//! The file name (`stdio.rs`) describes the transport; the binary's
//! user-facing name (`cdno-mcp`) is set in `Cargo.toml`'s `[[bin]]`
//! table. A future `bin/server.rs` will host the HTTP transport
//! under a separate binary name.
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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use cdno_core::config::VaultConfig;
use cdno_core::index::{SqliteIndex, VaultIndex};
use cdno_core::paths;
use cdno_core::store::{FsVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_mcp::CuadernoServer;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let root = vault_root_from_env()?;
    tracing::info!(vault_root = %root.display(), "starting cdno-mcp");

    let vault = open_vault(&root).inspect_err(|e| {
        tracing::error!(error = %e, vault_root = %root.display(), "failed to open vault");
    })?;

    let server = CuadernoServer::new(Arc::new(vault));
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

/// Open a vault at `root`. Mirror of [`cdno_cli::bootstrap::open_vault`]
/// — duplicated here so cdno-mcp doesn't depend on the CLI crate. If
/// this drift becomes painful, lift both into a shared core helper.
fn open_vault(root: &Path) -> Result<Vault> {
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
    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(root));
    let index: Arc<dyn VaultIndex> = Arc::new(
        SqliteIndex::open(root.join(paths::INDEX_DB))
            .with_context(|| format!("opening {}", root.join(paths::INDEX_DB).display()))?,
    );
    let (vault, report) =
        Vault::new(store, index, config).context("constructing vault and reconciling index")?;
    tracing::debug!(
        scanned = report.scanned,
        added = report.added,
        updated = report.updated,
        removed = report.removed,
        errors = report.errors.len(),
        "reconciliation complete",
    );
    Ok(vault)
}
