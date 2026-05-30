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
//! Stub today: tools answer with "not yet implemented" until #46 and
//! #47 fill in the handlers. Wiring it up here means MCP clients
//! can already register the server, list the advertised tools, and
//! see the schemas — which is how Claude Desktop / Claude Code
//! discover capabilities at startup.

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

#[tokio::main]
async fn main() -> Result<()> {
    let root = vault_root_from_env()?;
    let vault = open_vault(&root)?;
    let server = CuadernoServer::new(Arc::new(vault));

    // `ServiceExt::serve(transport)` performs the MCP init handshake,
    // routes incoming `tools/call` requests through the
    // `#[tool_router]` table, and runs until the client disconnects.
    let running = server
        .serve(stdio())
        .await
        .context("failed to start MCP service over stdio")?;
    running
        .waiting()
        .await
        .context("MCP service exited with an error")?;
    Ok(())
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
            "no Cuaderno vault at {}; run `cdno init` first.",
            root.display()
        );
    }

    let config = VaultConfig::load(root)
        .with_context(|| format!("loading {}", root.join(paths::CONFIG_FILE).display()))?;
    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(root));
    let index: Arc<dyn VaultIndex> = Arc::new(
        SqliteIndex::open(root.join(paths::INDEX_DB))
            .with_context(|| format!("opening {}", root.join(paths::INDEX_DB).display()))?,
    );
    let (vault, _report) =
        Vault::new(store, index, config).context("constructing vault and reconciling index")?;
    Ok(vault)
}
