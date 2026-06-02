//! Wire production dependencies (filesystem store, SQLite index,
//! TOML config) into a [`Vault`] and run startup reconciliation.
//!
//! Every subcommand that operates on an existing vault opens it
//! through here. `init` is the exception — it creates the vault
//! rather than opening one.

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

/// Resolve which vault root to operate on, in precedence order:
///
/// 1. `vault_flag` — an explicit `--vault <path>`, wins unconditionally.
/// 2. A vault discovered by walking up from `cwd` — so running *inside*
///    a vault always targets that vault, even when `env_vault_path`
///    points elsewhere. Discovery must beat the env fallback or a stray
///    `CUADERNO_VAULT_PATH` would silently misroute writes in a
///    multi-vault setup.
/// 3. `env_vault_path` — the `CUADERNO_VAULT_PATH` value, the "I'm
///    outside any vault" fallback. Empty / whitespace-only values are
///    treated as unset so an accidentally-exported blank var doesn't
///    resolve to `""`.
///
/// Returns `None` when nothing resolves; the caller renders the error.
/// Pure over its inputs — `main` supplies the real CWD and environment
/// — so the precedence policy is testable without touching either.
pub fn resolve_vault_root(
    vault_flag: Option<&Path>,
    cwd: &Path,
    env_vault_path: Option<&str>,
) -> Option<PathBuf> {
    if let Some(path) = vault_flag {
        return Some(path.to_path_buf());
    }
    if let Some(root) = discover_vault_root(cwd) {
        return Some(root);
    }
    if let Some(value) = env_vault_path
        && !value.trim().is_empty()
    {
        return Some(PathBuf::from(value));
    }
    None
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
