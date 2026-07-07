//! Shared vault-opening for long-lived consumers.
//!
//! Lifted from `cdno-mcp`'s bootstrap when the Tauri app became its
//! fourth would-be copy (the tripwire that module documented). The
//! CLI's copy intentionally stays where it is: its surrounding
//! *resolution* semantics (upward vault discovery, `--vault`/env
//! precedence) are CLI-specific UX; this function starts where
//! resolution ends — with a concrete root path.
//!
//! Lives in cdno-domain, not cdno-core, because it constructs a
//! [`Vault`] and the domain crate sits above core in the dependency
//! order. Errors are typed ([`BootstrapError`]) per the workspace
//! rule — no `anyhow` in library code; binary callers wrap it at
//! their boundary.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cdno_core::config::{IgnoreSet, VaultConfig};
use cdno_core::error::{ConfigError, IndexError};
use cdno_core::index::{SqliteIndex, VaultIndex};
use cdno_core::paths;
use cdno_core::reconcile::ReconciliationReport;
use cdno_core::store::{FsVaultStore, VaultStore};

use crate::error::DomainError;
use crate::vault::Vault;

/// Everything [`open_vault`] produced. The store/index/ignore handles
/// exist so a long-running caller (an HTTP server's reconciliation
/// loop, the desktop app's watcher thread) can re-run the pass that
/// [`Vault::new`] performs once at open — `Vault` deliberately does
/// not re-expose them. Short-lived callers just take `vault` and
/// drop the rest.
pub struct OpenedVault {
    pub vault: Vault,
    pub store: Arc<dyn VaultStore>,
    pub index: Arc<dyn VaultIndex>,
    pub ignore: Arc<IgnoreSet>,
    pub report: ReconciliationReport,
}

/// Opening a vault failed before, or during, the startup
/// reconciliation.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error(
        "no Cuaderno vault at {} (looked for `.cuaderno/`). \
         Hint: run `cdno init {}` to scaffold one, or set \
         `CUADERNO_VAULT_PATH` to point at an existing vault.",
        .root.display(),
        .root.display()
    )]
    NotAVault { root: PathBuf },

    #[error("loading vault config: {0}")]
    Config(#[from] ConfigError),

    #[error("opening the index database: {0}")]
    Index(#[from] IndexError),

    #[error("constructing the vault and reconciling the index: {0}")]
    Domain(#[from] DomainError),
}

/// Open a vault at `root`: check the `.cuaderno/` marker, load config,
/// wire the filesystem store + SQLite index, and run the startup
/// reconciliation via [`Vault::new`].
pub fn open_vault(root: &Path) -> Result<OpenedVault, BootstrapError> {
    let cuaderno_dir = root.join(paths::CUADERNO_DIR);
    if !cuaderno_dir.is_dir() {
        return Err(BootstrapError::NotAVault {
            root: root.to_path_buf(),
        });
    }

    let config = VaultConfig::load(root)?;
    let ignore = Arc::new(config.ignore_set()?);
    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(root));
    let index: Arc<dyn VaultIndex> = Arc::new(SqliteIndex::open(root.join(paths::INDEX_DB))?);
    let (vault, report) = Vault::new(store.clone(), index.clone(), config)?;
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
        report,
    })
}
