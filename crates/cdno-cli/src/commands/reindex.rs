//! `cdno reindex` — rebuild the SQLite index from the markdown source
//! of truth.
//!
//! The index is a cache: every fact in it but one is derived from the
//! notes on disk, so rebuilding it is always safe. This is the explicit
//! recovery path for a stale or damaged index.
//!
//! **In place** (#999). The rebuild is [`cdno_core::reconcile::rebuild`],
//! the one the servers repair a damaged index with: a fresh index is built
//! beside the live one and copied over it through SQLite's backup API,
//! under the vault write lock. The file is never deleted or renamed, so
//! every process on this machine that has it open (an MCP server, `cdno
//! watch`) reads the rebuilt contents on its next query, and no commit
//! lands half before and half after. A process on another machine or
//! kernel sharing the file is not covered: SQLite's locks do not reach it.
//! A held vault lock makes the rebuild wait one lock timeout, then fail
//! with the lock's own error, with the live index as it was. A live file
//! that fails its integrity check first has a copy kept beside it as
//! `<index>.previous`.
//!
//! **Archival snapshots** (the one table the notes cannot regenerate, the
//! baseline `cdno lint` checks archived action notes against, and that
//! `cdno open` consults to warn before an archived note is edited) are
//! carried into the rebuilt index. `--drop-snapshots` leaves them behind,
//! the way out of a snapshot that can never verify again (#901).
//!
//! **A file too damaged to rebuild in place** (SQLite refuses to open it,
//! or to copy over it) is the one case that still needs a new file: under
//! the vault write lock it is moved aside to `<index>.previous` and a new
//! index is built from the notes. An MCP server that had the old file open
//! finds it moved and reopens the new one by itself (#1000); any other
//! process (`cdno watch`) stays on it until it restarts, and the output
//! says so. Its snapshots are lost with it.
//!
//! **The index location** is wherever [`bootstrap::index_location`]
//! resolves it — `CUADERNO_INDEX_PATH` when set (#998). An overridden file
//! is touched only when it is recognisably cuaderno's index, and one bound
//! to another vault root is refused unless `--rebind` is given. A rebind
//! does not rebuild in place: under this vault's write lock the file, if
//! it is still bound to that root, is deleted and a new one bound to this
//! vault is built, so none of its rows (its snapshots above all) or its
//! binding carry over, and a process still using it for the other root
//! keeps its own copy rather than starting to read this vault's notes. The
//! binding is a path, so the other root may be this same vault before a
//! move or rename: its snapshots are then dropped too, and the output is
//! worded to be true of both. A file bound to this vault by the time the
//! lock is held is rebuilt in place instead, and one whose binding can no
//! longer be read is refused.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use cdno_core::classify::NoteScope;
use cdno_core::error::{IndexError, StoreError};
use cdno_core::index::{SqliteIndex, VaultIndex, canonical_vault_root, previous_copy_path};
use cdno_core::paths::{self, IndexLocation};
use cdno_core::reconcile::{self, RebuildReport};
use cdno_core::store::{FsVaultStore, VaultStore};
use cdno_domain::Vault;

use crate::bootstrap;

/// How the index was rebuilt.
enum Rebuilt {
    /// Copied in place over the live file.
    InPlace(RebuildReport),
    /// The live file was too damaged to rebuild in place: moved aside to
    /// `previous`, and a new file built.
    SetAside { cause: String, previous: PathBuf },
    /// The file was bound to the vault root `from`: deleted, and a new file
    /// built for this vault.
    Rebound { from: String },
}

/// What [`discard_for_rebind`] did.
enum Rebind {
    /// The file bound to the other root was deleted (or was already gone).
    Discarded,
    /// The file was bound to this vault by the time the lock was held, so
    /// it is left for an ordinary in-place rebuild.
    NowBoundHere,
}

/// Test-only, undocumented: when set, `--rebind` prints
/// [`REBIND_LOCK_SIGNAL`] to stderr just before it takes the vault write
/// lock (`tests/reindex.rs` waits for it).
pub const REBIND_LOCK_SIGNAL_ENV: &str = "CDNO_TEST_SIGNAL_REBIND_LOCK";

/// The line printed for [`REBIND_LOCK_SIGNAL_ENV`].
pub const REBIND_LOCK_SIGNAL: &str = "cdno-test: rebind taking the vault write lock";

/// Appended to the message of a lock timeout.
const LOCK_HINT: &str = "; another cdno process is writing this vault; run it again";

pub fn run(root: &Path, rebind: bool, drop_snapshots: bool) -> Result<()> {
    // Confirm we're actually in a vault before touching anything.
    let cuaderno_dir = root.join(paths::CUADERNO_DIR);
    if !cuaderno_dir.is_dir() {
        anyhow::bail!(
            "no Cuaderno vault at {}; run `cdno init` to create one.",
            root.display()
        );
    }

    // Resolve before touching anything: a bad override is refused with
    // the index untouched.
    let location = bootstrap::index_location(root)?;
    let here = canonical_vault_root(root);
    let rebound_from = if location.overridden {
        // Never touch a file that is not cuaderno's index (a mistyped path).
        SqliteIndex::ensure_discardable(&location.path)?;
        match SqliteIndex::bound_vault_root(&location.path)? {
            Some(bound) if bound != here && !rebind => {
                return Err(IndexError::BoundToOtherVault {
                    index: location.path.display().to_string(),
                    bound,
                    root: here,
                }
                .into());
            }
            Some(bound) if bound != here => Some(bound),
            _ => None,
        }
    } else {
        None
    };

    let config = bootstrap::load_config(root)?;
    let scope = Vault::note_scope_of(&config).context("validating the vault config")?;
    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(root));
    // Rebuild files a killed rebuild left behind; nothing else removes them.
    SqliteIndex::clear_stale_rebuilds(&location.path);

    // A rebind whose file is bound to this vault by the time the lock is
    // held is an ordinary rebuild, in place: a process of this vault may
    // hold the file now.
    let rebound_from = match rebound_from {
        Some(from) => match discard_for_rebind(&store, &location, &here, &from)? {
            Rebind::Discarded => Some(from),
            Rebind::NowBoundHere => None,
        },
        None => None,
    };
    let (rebuilt, vault_report) = match rebound_from {
        Some(from) => {
            let (_vault, report) = bootstrap::open_vault(root).context("rebuilding the index")?;
            (Rebuilt::Rebound { from }, report)
        }
        None => match rebuild_or_set_aside(&store, &location, root, &scope, drop_snapshots)? {
            (Some(sqlite), rebuilt) => {
                // The rebuilt index is current: the vault's own startup
                // reconciliation finds nothing to do, and reports the notes
                // that still cannot be indexed.
                let index: Arc<dyn VaultIndex> = Arc::new(sqlite);
                let (_vault, report) = bootstrap::open_vault_on(store, index, config)
                    .context("opening the rebuilt index")?;
                (rebuilt, report)
            }
            (None, rebuilt) => {
                let (_vault, report) =
                    bootstrap::open_vault(root).context("rebuilding the index")?;
                (rebuilt, report)
            }
        },
    };

    // The counts: an in-place rebuild's own last pass, else the full pass
    // the reopen made into the new, empty file.
    let (notes, scanned, ignored, artefacts) = match &rebuilt {
        Rebuilt::InPlace(r) => (r.notes, r.scanned, r.ignored, r.artefacts),
        _ => (
            vault_report.added,
            vault_report.scanned,
            vault_report.ignored,
            vault_report.artefacts,
        ),
    };
    println!("Reindexed {notes} note(s) ({scanned} scanned).");
    // An override is invisible otherwise: name the file it resolved to.
    if location.overridden {
        println!(
            "Index: {} (from CUADERNO_INDEX_PATH).",
            location.path.display()
        );
    }
    match &rebuilt {
        Rebuilt::InPlace(report) => print_in_place(report, drop_snapshots),
        Rebuilt::SetAside { cause, previous } => {
            println!(
                "The index was too damaged to rebuild in place ({cause}); it was moved aside to {} \
                 and a new one was built from the notes. Its archival snapshots could not be \
                 carried, so `cdno lint` no longer checks the frozen prefix of archived action \
                 notes.",
                previous.display()
            );
            println!(
                "An MCP server that had the index open reopens the new one on its next sweep, or \
                 at the next tool call that meets the damage. Restart any other process that had \
                 it open (`cdno watch`): until it restarts it keeps using the old file."
            );
        }
        Rebuilt::Rebound { from } => {
            println!("Rebound the index from the vault at {from} to this vault.");
            // Worded to be true whether `from` is another vault or this one
            // before a move or rename: the binding is a path, so the two
            // look alike from here.
            println!(
                "The index recorded for {from} was discarded, archival snapshots included, and a \
                 new one was built for this vault; if {from} was this vault before it was moved \
                 or renamed, those were its own snapshots. A process still using the file for \
                 {from} keeps the old copy until it restarts."
            );
        }
    }
    // Make ignore-glob exclusions visible: a stray `**` that drops every
    // note from search/lint should never be silent. The files are on
    // disk untouched — removing the offending pattern and reindexing
    // brings them all back.
    if ignored > 0 {
        println!(
            "{ignored} file(s) excluded by `ignore` globs (still on disk; clear the glob and reindex to restore)."
        );
    }
    // Artefacts are excluded by design, not by configuration, but the
    // count is reported for the same reason: a file absent from the index
    // is absent from search and lint, so it should never be a surprise.
    if artefacts > 0 {
        println!(
            "{artefacts} markdown file(s) skipped as attachment artefacts (filed documents, reachable through their evidence stub)."
        );
    }
    Ok(())
}

/// The lines an in-place rebuild adds: nothing in the ordinary case.
fn print_in_place(report: &RebuildReport, drop_snapshots: bool) {
    let kept = report
        .previous_copy
        .as_ref()
        .map(|copy| {
            format!(
                ", and a copy of the old file was kept at {}",
                copy.display()
            )
        })
        .unwrap_or_default();
    if report.found_damage {
        println!("The index was damaged; it was rebuilt in place{kept}.");
    } else if let Some(e) = &report.check_error {
        println!(
            "The index's integrity check could not run before the rebuild ({e}), so whether it \
             was damaged is unknown; it was rebuilt in place{kept}."
        );
    }
    if drop_snapshots {
        if report.snapshots_dropped > 0 {
            println!(
                "Dropped {} archival snapshot(s): `cdno lint` no longer checks the frozen prefix \
                 of those archived action notes, and `cdno open` no longer warns before opening \
                 them.",
                report.snapshots_dropped
            );
        } else {
            println!("No archival snapshots to drop.");
        }
    }
    if let Some(e) = &report.snapshots_unreadable {
        eprintln!(
            "warning: the old index's archival snapshots could not be read ({e}), so none was \
             carried into the rebuilt index; `cdno lint` no longer checks the frozen prefix of \
             archived action notes."
        );
    }
    if !report.snapshots_lost.is_empty() {
        eprintln!(
            "warning: {} archival snapshot(s) could not be carried into the rebuilt index; \
             `cdno lint` no longer checks those archived notes' frozen prefix:",
            report.snapshots_lost.len()
        );
        for lost in &report.snapshots_lost {
            eprintln!("  {lost}");
        }
    }
}

/// Rebuild the index in place, or, when the file is too damaged for that,
/// move it aside under the vault write lock. Returns the open index after
/// an in-place rebuild, `None` when the file was moved aside (the caller
/// opens the new one).
///
/// Any failure other than damage (the vault lock held past its timeout, a
/// busy database, an I/O error) is returned with the index as it was.
fn rebuild_or_set_aside(
    store: &Arc<dyn VaultStore>,
    location: &IndexLocation,
    root: &Path,
    scope: &NoteScope,
    drop_snapshots: bool,
) -> Result<(Option<SqliteIndex>, Rebuilt)> {
    let unchanged = |e: IndexError| {
        let mut context = format!(
            "rebuilding the index at {}; it was left as it was",
            location.path.display()
        );
        if matches!(
            e,
            IndexError::Lock {
                source: StoreError::LockTimeout(_),
                ..
            }
        ) {
            context.push_str(LOCK_HINT);
        }
        anyhow::Error::new(e).context(context)
    };
    // Twice at most: a second round only when another process repaired
    // the file between the failure here and the lock.
    let mut last_cause = None;
    for _ in 0..2 {
        let cause = match SqliteIndex::open_location_unhealed(location, root) {
            Ok(sqlite) => {
                let rebuilt = if drop_snapshots {
                    reconcile::rebuild_without_snapshots(store, &sqlite, scope, true)
                } else {
                    reconcile::rebuild(store, &sqlite, scope, true)
                };
                match rebuilt {
                    Ok(report) => return Ok((Some(sqlite), Rebuilt::InPlace(report))),
                    // Too damaged to copy over: the one case for a new file.
                    Err(e) if e.is_corruption() => e,
                    Err(e) => return Err(unchanged(e)),
                }
            }
            // Too damaged to open.
            Err(e) if e.is_corruption() => e,
            Err(e) => return Err(unchanged(e)),
        };
        if set_aside_under_lock(store, location, root)? {
            return Ok((
                None,
                Rebuilt::SetAside {
                    cause: cause.to_string(),
                    previous: previous_copy_path(&location.path),
                },
            ));
        }
        last_cause = Some(cause);
    }
    // Both rounds met damage, and both times the file checked sound once
    // the lock was held: name the damage, not only the outcome.
    let cause = last_cause.map(|e| e.to_string()).unwrap_or_default();
    bail!(
        "the index at {} could not be rebuilt in place ({cause}), yet it checked sound under the \
         vault write lock, so it was left as it was: another process is repairing or replacing \
         it. Run `cdno reindex` again",
        location.path.display()
    )
}

/// The error for failing to take the vault write lock: `what`, plus the
/// hint when the failure is the lock's timeout.
fn lock_error(what: &str, e: StoreError) -> anyhow::Error {
    let context = match e {
        StoreError::LockTimeout(_) => format!("{what}{LOCK_HINT}"),
        _ => what.to_owned(),
    };
    anyhow::Error::new(e).context(context)
}

/// Move the damaged index aside, under the vault write lock, so the next
/// open starts from an empty file. Returns false, moving nothing, when
/// under the lock the file opens and checks sound: another process (a
/// server's start) repaired it meanwhile.
///
/// An overridden file passes the guards again under the lock, so one
/// replaced or rebound since `reindex` began is refused rather than moved.
fn set_aside_under_lock(
    store: &Arc<dyn VaultStore>,
    location: &IndexLocation,
    root: &Path,
) -> Result<bool> {
    let _lock = store.acquire_index_lock().map_err(|e| {
        lock_error(
            "taking the vault write lock to move the damaged index aside; nothing was moved",
            e,
        )
    })?;
    {
        let repaired = SqliteIndex::open_location_unhealed(location, root)
            .is_ok_and(|index| index.quick_check().is_ok_and(|p| p.is_empty()));
        if repaired {
            return Ok(false);
        }
    }
    if location.overridden {
        SqliteIndex::ensure_discardable(&location.path)?;
        let here = canonical_vault_root(root);
        if let Some(bound) = SqliteIndex::bound_vault_root(&location.path)?
            && bound != here
        {
            return Err(IndexError::BoundToOtherVault {
                index: location.path.display().to_string(),
                bound,
                root: here,
            }
            .into());
        }
    }
    SqliteIndex::set_aside(location)?;
    Ok(true)
}

/// For `--rebind`: under this vault's write lock, delete the overridden
/// index if it is still bound to the vault root `from`. The guards run
/// again under the lock. A file bound to this vault by then is left for an
/// in-place rebuild (a process of this vault may hold it); one bound to a
/// third root, or whose binding can no longer be read (replaced or damaged
/// since), is refused with nothing changed.
fn discard_for_rebind(
    store: &Arc<dyn VaultStore>,
    location: &IndexLocation,
    here: &str,
    from: &str,
) -> Result<Rebind> {
    // A test seam, not an interface: with the variable set, say on stderr
    // that the unlocked checks are done and the lock is next, so a test can
    // change the file in exactly that window. Unset, nothing happens.
    if std::env::var_os(REBIND_LOCK_SIGNAL_ENV).is_some() {
        eprintln!("{REBIND_LOCK_SIGNAL}");
    }
    let _lock = store.acquire_index_lock().map_err(|e| {
        lock_error(
            "taking the vault write lock to rebind the index; nothing was changed",
            e,
        )
    })?;
    if !location.path.exists() {
        // A `-wal` or `-shm` the other vault's file left must not end up
        // beside the new one, where SQLite would replay it.
        SqliteIndex::remove(&location.path);
        return Ok(Rebind::Discarded);
    }
    SqliteIndex::ensure_discardable(&location.path)?;
    match SqliteIndex::bound_vault_root(&location.path)? {
        Some(bound) if bound == from => {
            SqliteIndex::remove(&location.path);
            Ok(Rebind::Discarded)
        }
        Some(bound) if bound == here => Ok(Rebind::NowBoundHere),
        Some(bound) => Err(IndexError::BoundToOtherVault {
            index: location.path.display().to_string(),
            bound,
            root: here.to_owned(),
        }
        .into()),
        None => bail!(
            "the index file {} changed while `cdno reindex --rebind` waited for the vault write \
             lock: it no longer records the vault at {from}, and no binding can be read from it \
             (replaced, or damaged); nothing was changed. Run `cdno reindex --rebind` again",
            location.path.display()
        ),
    }
}
