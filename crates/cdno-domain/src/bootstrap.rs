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
use cdno_core::paths::{self, IndexLocation};
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
    /// rebuilt it; `None` when it found nothing, or when the open did not
    /// check.
    pub damage_at_open: Option<Vec<String>>,
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
    open(root, None, false)
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
    open(root, index_override, false)
}

/// [`open_vault_with_index`] for a long-lived process: before the startup
/// reconciliation, run `PRAGMA quick_check` on the index, and if it
/// reports damage, rebuild it in place from the notes
/// ([`cdno_core::reconcile::rebuild`]), keeping a copy of the damaged file
/// beside it as `<index>.previous` and carrying across the archival
/// snapshots it can still read. If that rebuild fails, the index is emptied
/// in place instead and the startup reconciliation fills it (the snapshots
/// are then lost, as with `cdno reindex`). It also clears rebuild files a
/// killed rebuild left behind.
///
/// The index is wherever the override puts it, and every file the repair
/// writes sits beside that file. The override's refusals come first and are
/// final: a file that is not cuaderno's index, or is bound to another vault,
/// fails the open exactly as in [`open_vault_with_index`], and is never
/// checked, rebuilt or set aside. A rebuilt overridden index stays bound to
/// this vault.
///
/// A damaged file can open and answer queries normally, so the open-time
/// self-heal, which only catches a file SQLite refuses outright, misses
/// it, and a restart then reopens the same damage (#526). The check costs
/// tens of milliseconds, which a CLI pays on every command and a server
/// pays once.
pub fn open_vault_checked(
    root: &Path,
    index_override: Option<&OsStr>,
) -> Result<OpenedVault, BootstrapError> {
    open(root, index_override, true)
}

/// The checked open's fallback when the rebuild failed: empty the index in
/// place, so the startup reconciliation fills it and any other process
/// holding the file sees the same empty-then-filled index. Renaming the
/// file away would strand those processes on the damaged one, so that is
/// only the last resort, when even the in-place copy is refused (a header
/// too damaged to write over), and it runs under the vault write lock.
///
/// Neither step ever takes an overridden file from another vault: a
/// [`IndexError::BoundToOtherVault`] from the emptying is returned as is,
/// and before an overridden file is set aside the open's guards run again,
/// under the lock, so a file replaced or rebound since the open is refused
/// rather than moved.
fn empty_in_place(
    sqlite: SqliteIndex,
    store: &Arc<dyn VaultStore>,
    location: &IndexLocation,
    root: &Path,
    cause: &IndexError,
) -> Result<SqliteIndex, BootstrapError> {
    let emptied = (|| {
        let empty = sqlite.open_fresh_beside()?;
        let _lock = store
            .acquire_write_lock()
            .map_err(|e| IndexError::Update(format!("acquiring the vault write lock: {e}")))?;
        sqlite.replace_contents_from(&empty)
    })();
    match emptied {
        Ok(()) => {
            tracing::warn!(
                error = %cause,
                "rebuilding the damaged index failed; emptied it in place, and the startup \
                 reconciliation refills it from the notes"
            );
            Ok(sqlite)
        }
        Err(e @ IndexError::BoundToOtherVault { .. }) => Err(e.into()),
        Err(e) => {
            tracing::warn!(
                error = %cause,
                emptying = %e,
                index_path = %location.path.display(),
                "rebuilding the damaged index failed and it could not be emptied in place; \
                 setting it aside and starting from an empty one (another process with it open \
                 keeps the old file until it restarts)"
            );
            let _lock = store.acquire_write_lock().ok();
            drop(sqlite);
            if location.overridden {
                SqliteIndex::ensure_discardable(&location.path)?;
                let canonical = cdno_core::index::canonical_vault_root(root);
                if let Some(bound) = SqliteIndex::bound_vault_root(&location.path)?
                    && bound != canonical
                {
                    return Err(IndexError::BoundToOtherVault {
                        index: location.path.display().to_string(),
                        bound,
                        root: canonical,
                    }
                    .into());
                }
            }
            SqliteIndex::set_aside(&location.path);
            Ok(SqliteIndex::open_location(location, root)?)
        }
    }
}

/// Remove the rebuild files a killed rebuild left beside the index
/// (`<index>.rebuild-<pid>-<n>`, and its `-wal` / `-shm`): nothing else
/// ever deletes them. Only names of exactly that shape, so a file of
/// someone else's that merely shares the prefix in an override's directory
/// is left alone, and only ones untouched for an hour, so a rebuild running
/// in another process right now keeps its file.
fn clear_stale_rebuilds(index_path: &Path) {
    let (Some(dir), Some(name)) = (index_path.parent(), index_path.file_name()) else {
        return;
    };
    let prefix = format!(
        "{}{}-",
        name.to_string_lossy(),
        cdno_core::index::REBUILD_SUFFIX
    );
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let Some(rest) = file_name.strip_prefix(&prefix) else {
            continue;
        };
        if !is_rebuild_tail(rest) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > std::time::Duration::from_secs(3600));
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Whether `rest` is what follows `<index>.rebuild-` in a rebuild file's
/// name: `<pid>-<n>`, optionally with `-wal` or `-shm`.
fn is_rebuild_tail(rest: &str) -> bool {
    let rest = rest
        .strip_suffix("-wal")
        .or_else(|| rest.strip_suffix("-shm"))
        .unwrap_or(rest);
    let digits =
        |p: Option<&str>| p.is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    let mut parts = rest.split('-');
    digits(parts.next()) && digits(parts.next()) && parts.next().is_none()
}

fn open(
    root: &Path,
    index_override: Option<&OsStr>,
    check: bool,
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
    // The override's refusals (not an index, bound to another vault)
    // happen here, before any check or repair can touch the file.
    let mut sqlite = SqliteIndex::open_location(&location, root)?;
    let mut damage_at_open = None;
    if check {
        clear_stale_rebuilds(&location.path);
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
                index_path = %location.path.display(),
                "the index failed its integrity check at open; rebuilding it from the notes \
                 (a copy of the damaged file is kept beside it, with `.previous` appended)"
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
                // Never empty or set aside a file another vault has bound
                // since the open: refuse, as the open itself would have.
                Err(e @ IndexError::BoundToOtherVault { .. }) => return Err(e.into()),
                Err(e) => {
                    sqlite = empty_in_place(sqlite, &store, &location, root, &e)?;
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
        index_path: location.path,
    })
}
