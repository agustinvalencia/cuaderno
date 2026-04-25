//! Wire production dependencies (filesystem store, SQLite index,
//! TOML config) into a [`Vault`] and run startup reconciliation.
//!
//! Every subcommand that operates on an existing vault opens it
//! through here. `init` is the exception — it creates the vault
//! rather than opening one. Until #22 lands additional subcommands,
//! the helpers below are unused at the binary entry point; the
//! `dead_code` suppression goes away as soon as a caller exists.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use cdno_core::config::VaultConfig;
use cdno_core::index::{SqliteIndex, VaultIndex};
use cdno_core::paths;
use cdno_core::reconcile::ReconciliationReport;
use cdno_core::store::{FsVaultStore, VaultStore};
use cdno_domain::Vault;

/// Walk upward from `start` looking for a `.cuaderno/` marker.
/// Returns the directory containing it, or `None` if no vault is found.
///
/// Mirrors the discovery semantics of `git`: a user can run `cdno`
/// from anywhere inside the vault and have it locate the root.
pub fn discover_vault_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(paths::CUADERNO_DIR).is_dir())
        .map(Path::to_path_buf)
}

/// Open a Cuaderno vault rooted at `root`: load config, open the
/// SQLite index, run startup reconciliation.
///
/// Errors if `root/.cuaderno/` is missing — that signals the user
/// pointed at a non-vault directory and should run `cdno init`.
pub fn open_vault(root: &Path) -> Result<(Vault, ReconciliationReport)> {
    let cuaderno_dir = root.join(paths::CUADERNO_DIR);
    if !cuaderno_dir.is_dir() {
        bail!(
            "no Cuaderno vault at {}; run `cdno init` to create one.",
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

    Vault::new(store, index, config).context("constructing vault and reconciling index")
}
