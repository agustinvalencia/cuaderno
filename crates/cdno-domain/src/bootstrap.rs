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
//! order. This is deliberately the crate's **single impure module**
//! — the composition root that names `FsVaultStore` and
//! `SqliteIndex` concretely so no downstream crate has to; the
//! "domain depends on traits only" rule holds everywhere else.
//! Errors are typed ([`BootstrapError`]) per the workspace rule — no
//! `anyhow` in library code; binary callers wrap it at their
//! boundary.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cdno_core::classify::NoteScope;
use cdno_core::config::VaultConfig;
use cdno_core::error::{ConfigError, IndexError};
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
    /// The same index as [`index`](Self::index), concretely: the handle
    /// a long-lived caller checks and rebuilds in place (#526). `index`
    /// and this are one object, so a rebuild through it is seen through
    /// the vault too.
    pub sqlite: Arc<SqliteIndex>,
    /// What [`open_vault_checked`] found wrong with the index before it
    /// set the file aside and started from an empty one; `None` when it
    /// found nothing, or when the open did not check.
    pub damage_at_open: Option<Vec<String>>,
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
    open(root, false)
}

/// [`open_vault`] for a long-lived process: before the startup
/// reconciliation, run `PRAGMA quick_check` on the index, and if it
/// reports damage, rebuild it in place from the notes
/// ([`cdno_core::reconcile::rebuild`]), keeping a copy of the damaged file
/// as `index.db.previous` and carrying across the archival snapshots it
/// can still read. If that rebuild fails, the damaged file is set aside
/// instead and the open starts from an empty index, which the
/// reconciliation fills (the snapshots are then lost, as with
/// `cdno reindex`).
///
/// A damaged file can open and answer queries normally, so the open-time
/// self-heal, which only catches a file SQLite refuses outright, misses
/// it, and a restart then reopens the same damage (#526). The check costs
/// tens of milliseconds, which a CLI pays on every command and a server
/// pays once.
pub fn open_vault_checked(root: &Path) -> Result<OpenedVault, BootstrapError> {
    open(root, true)
}

fn open(root: &Path, check: bool) -> Result<OpenedVault, BootstrapError> {
    let cuaderno_dir = root.join(paths::CUADERNO_DIR);
    if !cuaderno_dir.is_dir() {
        return Err(BootstrapError::NotAVault {
            root: root.to_path_buf(),
        });
    }

    let config = VaultConfig::load(root)?;
    // A malformed glob is a config error, reported before the index opens.
    config.ignore_set()?;
    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(root));
    let index_path = root.join(paths::INDEX_DB);
    let mut sqlite = SqliteIndex::open(&index_path)?;
    let mut damage_at_open = None;
    if check {
        // A check that cannot run (a busy or unreadable file) is no reason
        // to refuse to start: before the check existed the server would
        // have started, and the sweep checks again.
        let problems = sqlite.quick_check().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "the index integrity check could not run at open");
            Vec::new()
        });
        if !problems.is_empty() {
            tracing::warn!(
                problems = ?problems,
                "the index failed its integrity check at open; rebuilding it from the notes \
                 (the damaged file is kept as index.db.previous)"
            );
            let scope = Vault::note_scope_of(&config)?;
            match cdno_core::reconcile::rebuild(&store, &sqlite, &scope) {
                Ok(rebuilt) => {
                    if !rebuilt.snapshots_lost.is_empty() {
                        tracing::warn!(
                            lost = ?rebuilt.snapshots_lost,
                            "some archival snapshots could not be carried into the rebuilt index"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "rebuilding the damaged index failed; setting it aside and starting \
                         from an empty one"
                    );
                    drop(sqlite);
                    SqliteIndex::set_aside(&index_path);
                    sqlite = SqliteIndex::open(&index_path)?;
                }
            }
            damage_at_open = Some(problems);
        }
    }
    let sqlite = Arc::new(sqlite);
    let index: Arc<dyn VaultIndex> = sqlite.clone();
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
        sqlite,
        damage_at_open,
    })
}
