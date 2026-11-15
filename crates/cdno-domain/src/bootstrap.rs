//! Shared vault-opening for long-lived consumers.
//!
//! Lifted from `cdno-mcp`'s bootstrap when the Tauri app became its
//! fourth would-be copy (the tripwire that module documented). The
//! CLI's copy intentionally stays where it is: its surrounding
//! *resolution* semantics (upward vault discovery, `--vault`/env
//! precedence) are CLI-specific UX; this function starts where
//! resolution ends — with a concrete root path, and, for the index, the
//! raw override value the binary read (`CUADERNO_INDEX_PATH` or
//! `--index-path`). Nothing here reads the process environment.
//!
//! Lives in cdno-domain, not cdno-core, because it constructs a
//! [`Vault`] and the domain crate sits above core in the dependency
//! order. This is deliberately the crate's **single impure module**
//! — the composition root that names `FsVaultStore` and
//! `SqliteIndex` concretely so no downstream crate has to; the
//! "domain depends on traits only" rule holds everywhere else.
//! Errors are typed ([`BootstrapError`]) per the workspace rule — no
//! `anyhow` in library code; binary callers wrap it at their
//! boundary.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cdno_core::classify::NoteScope;
use cdno_core::config::VaultConfig;
use cdno_core::error::{ConfigError, IndexError, IndexPathError};
use cdno_core::index::{SqliteIndex, VaultIndex};
use cdno_core::paths;
use cdno_core::reconcile::ReconciliationReport;
use cdno_core::store::{FsVaultStore, VaultStore};

use crate::error::DomainError;
use crate::vault::Vault;

/// Everything [`open_vault`] produced. The store/index/scope handles
/// exist so a long-running caller (an HTTP server's reconciliation
/// loop, the desktop app's watcher thread) can re-run the pass that
/// [`Vault::new`] performs once at open — `Vault` deliberately does
/// not re-expose them. Short-lived callers just take `vault` and
/// drop the rest.
pub struct OpenedVault {
    pub vault: Vault,
    pub store: Arc<dyn VaultStore>,
    pub index: Arc<dyn VaultIndex>,
    pub scope: Arc<NoteScope>,
    pub report: ReconciliationReport,
    /// The index database file this open used, after the per-machine
    /// override was resolved ([`paths::resolve_index_path`]). Kept so
    /// a long-running caller can name it in logs and keep it out of a
    /// git checkpoint when it lies inside the vault.
    pub index_path: PathBuf,
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

    #[error("{0}")]
    IndexPath(#[from] IndexPathError),

    #[error("opening the index database: {0}")]
    Index(#[from] IndexError),

    #[error("constructing the vault and reconciling the index: {0}")]
    Domain(#[from] DomainError),
}

/// Open a vault at `root`: check the `.cuaderno/` marker, load config,
/// wire the filesystem store + SQLite index, and run the startup
/// reconciliation via [`Vault::new`].
///
/// The index is at its default location, `<root>/.cuaderno/index.db`; a
/// binary that honours an override calls [`open_vault_with_index`].
pub fn open_vault(root: &Path) -> Result<OpenedVault, BootstrapError> {
    open_vault_with_index(root, None)
}

/// [`open_vault`] with the index override the binary read
/// (`cdno-mcp-server --index-path`, which falls back to
/// `CUADERNO_INDEX_PATH`; the stdio server reads the variable through
/// [`paths::index_override_from_env`]). `None` or a blank value means the
/// default location; the rest of the policy is
/// [`paths::resolve_index_path`], and an overridden index is opened bound
/// to this vault ([`SqliteIndex::open_location`]).
pub fn open_vault_with_index(
    root: &Path,
    index_override: Option<&OsStr>,
) -> Result<OpenedVault, BootstrapError> {
    let cuaderno_dir = root.join(paths::CUADERNO_DIR);
    if !cuaderno_dir.is_dir() {
        return Err(BootstrapError::NotAVault {
            root: root.to_path_buf(),
        });
    }

    let location = paths::resolve_index_path(root, index_override)?;
    let config = VaultConfig::load(root)?;
    // A malformed glob is a config error, reported before the index opens.
    config.ignore_set()?;
    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(root));
    let index: Arc<dyn VaultIndex> = Arc::new(SqliteIndex::open_location(&location, root)?);
    let (vault, report) = Vault::new(store.clone(), index.clone(), config)?;
    let scope = Arc::new(vault.note_scope()?);
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
        scope,
        report,
        index_path: location.path,
    })
}
