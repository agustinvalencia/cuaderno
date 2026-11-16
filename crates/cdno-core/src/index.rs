//! SQLite-backed vault index.
//!
//! The index is a cache — filesystem state is authoritative, reconciliation
//! rebuilds the index on any inconsistency. See `SCHEMA.md` for the table
//! shapes and rationale.
//!
//! This module owns connection setup, migrations, the [`VaultIndex`] trait,
//! and a concrete [`SqliteIndex`] implementation.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value as JsonValue;

use crate::error::IndexError;
use crate::path::VaultPath;

/// Ordered list of embedded migrations: (version, description, sql).
///
/// Every schema change is a new entry appended here. Migrations are
/// applied strictly in order; never edit a migration that has shipped.
const MIGRATIONS: &[(u32, &str, &str)] = &[
    (
        1,
        "Initial schema: notes, deadlines, links, note_tags, milestones, archived_action_snapshots",
        include_str!("../migrations/001_initial.sql"),
    ),
    (
        2,
        "FTS5 full-text search over note title + body",
        include_str!("../migrations/002_fts5_search.sql"),
    ),
    (
        3,
        "links.via_alias: the row resolved through a note alias",
        include_str!("../migrations/003_links_via_alias.sql"),
    ),
    (
        4,
        "archived_action_snapshots: masked_hash, mask_version and masked_size",
        include_str!("../migrations/004_snapshot_masked_hash.sql"),
    ),
    (
        5,
        "index_meta: key/value facts about the index itself (the bound vault root)",
        include_str!("../migrations/005_index_meta.sql"),
    ),
];

/// How long a connection waits on another writer before `SQLITE_BUSY`:
/// the main connection and the read-only probe alike.
const BUSY_TIMEOUT: Duration = Duration::from_millis(5000);

/// The first 16 bytes of every SQLite database file.
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// The `index_meta` key under which an overridden index records the
/// canonical root of the vault it belongs to (#998).
const VAULT_ROOT_KEY: &str = "vault_root";

/// The form of a vault root an overridden index records and compares:
/// canonicalised, so a vault reached through a symlink or a relative path
/// is still the same vault, else the path as given.
pub fn canonical_vault_root(root: &Path) -> String {
    root.canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// SQLite-backed implementation of the vault index.
///
/// Single [`Mutex<Connection>`] for now. This serialises reads and
/// writes through one connection, which is fine for CLI invocations
/// and simple MCP/Tauri access patterns. Upgrade path: swap for an
/// `r2d2`/`r2d2_sqlite` pool when read concurrency becomes a
/// measurable bottleneck — e.g. an agentic flow where multiple
/// workers read vault state while one writes. SQLite itself always
/// serialises writers, so a pool helps readers only; don't add the
/// dep until there's evidence. The connection behind the mutex can be
/// replaced ([`reopen`](Self::reopen), #1000) when the files it holds are
/// no longer the ones at its path, so holders of the index never see the
/// swap; a pool would have to swap every connection the same way, and
/// close them all before the first reopen of the same file.
pub struct SqliteIndex {
    conn: Mutex<Connection>,
    /// Where the database file lives, so [`open_fresh_beside`](Self::open_fresh_beside)
    /// can build a fresh index beside it.
    path: PathBuf,
    /// The canonical vault root an overridden index is bound to (#998), as
    /// [`open_location`](Self::open_location) checked it; `None` at the
    /// default location, which is never bound. A rebuild carries it: the
    /// fresh index is bound to the same root before it is copied in, so a
    /// rebuilt overridden index is still bound to this vault, and the copy
    /// refuses a live file another vault has bound since.
    binding: Option<String>,
    /// The files the connection holds, as they were when it opened them,
    /// for [`check_files`](Self::check_files) to compare with what the path
    /// names now (#1000). Locked only while the connection is, and after it.
    held: Mutex<HeldFiles>,
    /// Set while the mutex holds a placeholder, so every query fails with
    /// [`IndexError::Detached`] rather than an error about the placeholder.
    detached: std::sync::atomic::AtomicBool,
}

/// A file's identity on disk: the device and inode a descriptor to it
/// refers to, which a path that has been unlinked, renamed over or
/// recreated no longer shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileId {
    dev: u64,
    ino: u64,
}

/// What a `stat` of a path found.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stat {
    Present(FileId),
    /// Nothing is at the path (`NotFound`).
    Absent,
    /// The `stat` failed some other way (`EACCES`, `EIO` on a network
    /// mount): nothing is known, and nothing may be concluded from it.
    Failed(String),
}

/// The identity of the file at `path` now. Unix only: off unix the check
/// is not made at all ([`SqliteIndex::check_files`]).
#[cfg(unix)]
fn stat_file(path: &Path) -> Stat {
    use std::os::unix::fs::MetadataExt;
    match fs::metadata(path) {
        Ok(m) => Stat::Present(FileId {
            dev: m.dev(),
            ino: m.ino(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Stat::Absent,
        Err(e) => Stat::Failed(format!("{}: {e}", path.display())),
    }
}

/// How a file was found when the connection's files were recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recorded {
    Known(FileId),
    /// Not there: in WAL mode both sidecars exist once the open has read,
    /// so one missing then had already gone, and is reported.
    Missing,
    /// The `stat` failed: recorded at the first check that can see it.
    Unknown,
}

impl Recorded {
    fn of(path: &Path) -> Self {
        #[cfg(unix)]
        {
            match stat_file(path) {
                Stat::Present(id) => Self::Known(id),
                Stat::Absent => Self::Missing,
                Stat::Failed(_) => Self::Unknown,
            }
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Self::Unknown
        }
    }
}

/// What a connection holds: the database file and, in WAL mode, its
/// write-ahead log and shared-memory file, recorded right after the open
/// (when the connection has all three open and keeps them open until it
/// closes), under the name SQLite itself resolved. That name has its
/// symlinks resolved at the open, so a symlink retargeted later is not
/// seen as a move: the files compared are the ones SQLite opened.
#[derive(Debug, Clone)]
struct HeldFiles {
    db_path: PathBuf,
    db: Recorded,
    wal: Recorded,
    shm: Recorded,
    /// Set when a reopen closed the connection and could not open the file
    /// again: the mutex then holds a placeholder, and every query fails
    /// with [`IndexError::Detached`] until a reopen succeeds.
    detached: Option<String>,
    /// The snapshot rows a reopen read from the old connection before it
    /// closed it and then failed to open the file: kept with the detached
    /// state for the reopen that succeeds to hand on.
    salvaged: Option<Salvaged>,
}

/// Archival snapshot rows read from an old connection, or why they could not
/// be.
pub type Salvaged = Result<Vec<(VaultPath, ArchivalSnapshot)>, String>;

impl HeldFiles {
    fn of(conn: &Connection, path: &Path) -> Self {
        let db_path = conn
            .path()
            .filter(|p| !p.is_empty())
            .map_or_else(|| path.to_path_buf(), PathBuf::from);
        Self {
            db: Recorded::of(&db_path),
            wal: Recorded::of(&sibling(&db_path, "-wal")),
            shm: Recorded::of(&sibling(&db_path, "-shm")),
            db_path,
            detached: None,
            salvaged: None,
        }
    }
}

/// What [`SqliteIndex::check_files`] found: whether the files the
/// connection reads and writes are still the ones its path names (#1000).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileCheck {
    /// They are.
    Held,
    /// The database file at the path is not the one the connection has
    /// open: it was deleted, moved aside or replaced. The connection reads
    /// and writes a file no other process opening the path sees. Nearly
    /// always an act on this machine (`cdno reindex`'s set-aside, `--rebind`,
    /// a restore, `git clean -x`, a sync tool).
    DatabaseMoved(String),
    /// The database file is the one the connection has open, but its
    /// write-ahead log or shared-memory file was deleted or replaced at the
    /// path: the connection commits into, and reads through, orphans. The
    /// signature of a process on another kernel closing as the last
    /// connection, or of an `rm` or a sync tool.
    SidecarsMoved(String),
    /// A reopen closed the connection and could not open the file again,
    /// so nothing is held and every query fails until a reopen succeeds.
    Detached(String),
    /// A `stat` failed other than with not-found, so nothing is known this
    /// time; not a reason to reopen.
    Unchecked(String),
}

impl FileCheck {
    /// True when the connection's files are known to be the ones at the
    /// path.
    pub fn is_held(&self) -> bool {
        matches!(self, Self::Held)
    }

    /// True when the connection is known not to hold the files at the path,
    /// so it should be reopened. `Unchecked` is not.
    pub fn needs_reopen(&self) -> bool {
        matches!(
            self,
            Self::DatabaseMoved(_) | Self::SidecarsMoved(_) | Self::Detached(_)
        )
    }

    /// What was found, in a sentence, or `None` when the files are held.
    pub fn finding(&self) -> Option<&str> {
        match self {
            Self::Held => None,
            Self::DatabaseMoved(s)
            | Self::SidecarsMoved(s)
            | Self::Detached(s)
            | Self::Unchecked(s) => Some(s),
        }
    }

    /// A short name for the finding: `held`, `database_moved`,
    /// `sidecars_moved`, `detached` or `unchecked`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::DatabaseMoved(_) => "database_moved",
            Self::SidecarsMoved(_) => "sidecars_moved",
            Self::Detached(_) => "detached",
            Self::Unchecked(_) => "unchecked",
        }
    }
}

impl std::fmt::Display for FileCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            self.finding()
                .unwrap_or("the index files are the ones at its path"),
        )
    }
}

/// What a [`SqliteIndex::reopen`] did besides opening the file at the path.
#[derive(Debug, Default)]
pub struct Reopened {
    /// The archival snapshot rows the old connection could still read
    /// before it was closed, for the caller to carry into the reopened
    /// index once it is reconciled: they are the one table the notes cannot
    /// rebuild. `None` when there was no old connection to read (the index
    /// was detached); `Some(Err)` when reading them failed, and they are
    /// lost.
    pub salvaged: Option<Result<Vec<(VaultPath, ArchivalSnapshot)>, String>>,
    /// The old file's `-wal` and `-shm`, left at the path when only the
    /// database file was replaced, and moved aside to these paths
    /// ([`STALE_SUFFIX`]) before the new file was opened.
    pub stale_sidecars_moved_to: Vec<PathBuf>,
}

/// Suffix the old database's sidecars are moved aside under when a restore
/// replaced only the main file: `<index>.stale-wal`, `<index>.stale-shm`.
/// No database is named `<index>.stale`, so SQLite never pairs them with
/// one. They are the old file's log, kept only for diagnosis, replaced at
/// the next such set-aside and safe to delete at any time; at an overridden
/// location an existing one is replaced only when it is recognisably
/// SQLite's.
pub const STALE_SUFFIX: &str = ".stale";

/// Whether SQLite itself reports the database file `conn` has open as
/// moved: `SQLITE_FCNTL_HAS_MOVED`, which compares the open descriptor's
/// inode with the file the path names now (true when the path names none).
/// Only the unix VFS implements it.
fn sqlite_says_moved(conn: &Connection) -> bool {
    let mut moved: std::os::raw::c_int = 0;
    // SAFETY: the handle is valid for the borrow of `conn`, the schema
    // name is NUL-terminated, and HAS_MOVED writes one `int` through the
    // pointer, which points at a live `c_int`.
    let rc = unsafe {
        rusqlite::ffi::sqlite3_file_control(
            conn.handle(),
            c"main".as_ptr(),
            rusqlite::ffi::SQLITE_FCNTL_HAS_MOVED,
            (&mut moved as *mut std::os::raw::c_int).cast(),
        )
    };
    rc == rusqlite::ffi::SQLITE_OK && moved != 0
}

/// The files `held` records, compared with what the paths name now.
#[cfg(unix)]
fn compare_held(conn: &Connection, held: &mut HeldFiles) -> FileCheck {
    let path = held.db_path.clone();
    let now = stat_file(&path);
    if let Stat::Failed(e) = &now {
        return FileCheck::Unchecked(format!("could not check the index files: {e}"));
    }
    let moved = match (held.db, &now) {
        (Recorded::Known(was), Stat::Present(id)) => was != *id,
        (Recorded::Known(_) | Recorded::Missing, Stat::Absent) => true,
        _ => false,
    };
    if moved || sqlite_says_moved(conn) {
        return FileCheck::DatabaseMoved(if now == Stat::Absent {
            format!(
                "the index database file {} was deleted or moved away",
                path.display()
            )
        } else {
            format!(
                "the index database file {} was replaced by another file",
                path.display()
            )
        });
    }
    if let (Recorded::Unknown, Stat::Present(id)) = (held.db, &now) {
        held.db = Recorded::Known(*id);
    }
    for (suffix, recorded) in [("-wal", &mut held.wal), ("-shm", &mut held.shm)] {
        let sidecar = sibling(&path, suffix);
        let now = stat_file(&sidecar);
        let shown = sidecar.display();
        match (*recorded, now) {
            (_, Stat::Failed(e)) => {
                return FileCheck::Unchecked(format!("could not check the index files: {e}"));
            }
            (Recorded::Known(was), Stat::Present(id)) if was == id => {}
            (Recorded::Known(_), Stat::Present(_)) => {
                return FileCheck::SidecarsMoved(format!(
                    "{shown} was replaced by another file while this connection had it open"
                ));
            }
            (Recorded::Known(_) | Recorded::Unknown, Stat::Absent) => {
                return FileCheck::SidecarsMoved(format!(
                    "{shown} was deleted while this connection had it open"
                ));
            }
            (Recorded::Missing, _) => {
                return FileCheck::SidecarsMoved(format!(
                    "{shown} was already missing when this connection opened the index"
                ));
            }
            (Recorded::Unknown, Stat::Present(id)) => *recorded = Recorded::Known(id),
        }
    }
    FileCheck::Held
}

/// Whether the `-wal` or `-shm` at the path are still the ones `held`
/// records: after only the database file was replaced, they belong to the
/// old file, and a new connection would replay the old log onto the new
/// file.
#[cfg(unix)]
fn held_sidecars_at_path(held: &HeldFiles) -> bool {
    [("-wal", held.wal), ("-shm", held.shm)]
        .into_iter()
        .any(|(suffix, recorded)| match recorded {
            Recorded::Known(was) => {
                stat_file(&sibling(&held.db_path, suffix)) == Stat::Present(was)
            }
            _ => false,
        })
}

#[cfg(not(unix))]
fn held_sidecars_at_path(_held: &HeldFiles) -> bool {
    false
}

/// Whether the database file at the path is not the one `held` records
/// (deleted, moved or replaced); an error when a `stat` failed and nothing
/// can be told. A record that was not known is judged by SQLite's own
/// check.
#[cfg(unix)]
fn database_differs(conn: &Connection, held: &HeldFiles) -> Result<bool, String> {
    match (held.db, stat_file(&held.db_path)) {
        (_, Stat::Failed(e)) => Err(e),
        (Recorded::Known(was), Stat::Present(id)) => Ok(was != id || sqlite_says_moved(conn)),
        (Recorded::Known(_), Stat::Absent) => Ok(true),
        (_, now) => Ok(now == Stat::Absent || sqlite_says_moved(conn)),
    }
}

#[cfg(not(unix))]
fn database_differs(_conn: &Connection, _held: &HeldFiles) -> Result<bool, String> {
    Ok(false)
}

/// Whether an existing `<index>.stale-wal` / `.stale-shm` at an overridden
/// location may be replaced: absent, empty, or recognisably SQLite's (the
/// write-ahead log's magic number, or the wal-index header's version). The
/// same care [`SqliteIndex::ensure_discardable`] takes with `.previous`, so
/// a mistyped override never deletes another application's file.
fn is_discardable_sidecar(path: &Path, suffix: &str) -> bool {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => return e.kind() == std::io::ErrorKind::NotFound,
    };
    let Some(head) = bytes.get(..4) else {
        return bytes.is_empty();
    };
    let head = [head[0], head[1], head[2], head[3]];
    match suffix {
        "-wal" => matches!(u32::from_be_bytes(head), 0x377f_0682 | 0x377f_0683),
        _ => u32::from_ne_bytes(head) == 3_007_000,
    }
}

/// Whether the index file at `path` reads as a sound cuaderno index through
/// a read-only probe ([`SqliteIndex::open_probe`]): one that migrates,
/// binds and writes nothing, and whose close deletes nothing. For a caller
/// that must know whether another process repaired a file before it moves
/// it aside.
pub fn probe_reads_sound(path: &Path) -> bool {
    let Ok(conn) = SqliteIndex::open_probe(path) else {
        return false;
    };
    forbid_checkpoint_on_close(&conn);
    let ok = conn
        .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
        .is_ok_and(|r| r == "ok");
    ok && has_cuaderno_mark_on(&conn).unwrap_or(false)
}

/// Make a connection's close touch no file by name. The last connection
/// to a WAL database normally checkpoints on close and then deletes `-wal`
/// and `-shm` AT THE PATH: when the database file has moved, SQLite skips
/// that itself (`SQLITE_FCNTL_HAS_MOVED`), but when only the sidecars were
/// deleted under it, it would checkpoint its orphaned log over pages
/// another process may have changed since and then delete whatever
/// sidecars the path names now, another connection's.
/// `SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE` skips both: the close checkpoints
/// nothing and deletes nothing (both measured in the core
/// `index_reopen_tests`).
fn forbid_checkpoint_on_close(conn: &Connection) {
    let _ = conn.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        true,
    );
}

/// Close a connection whose files may no longer be its own without
/// touching any file by name ([`forbid_checkpoint_on_close`]). A close that
/// fails (it cannot, with no statement outstanding under the mutex) leaks
/// the connection rather than panicking in `Drop`.
fn close_detached(conn: Connection) {
    forbid_checkpoint_on_close(&conn);
    if let Err((conn, _)) = conn.close() {
        std::mem::forget(conn);
    }
}

/// On drop, a connection that is not known to hold the files at its path
/// closes without checkpointing or deleting anything ([`close_detached`]),
/// so a server shutting down on an orphan never writes its log over the
/// shared file or deletes the sidecars the path names now. A connection
/// that holds its files closes as SQLite's default, checkpointing and
/// removing its sidecars when it is the last, as every CLI command expects.
impl Drop for SqliteIndex {
    fn drop(&mut self) {
        if !self.check_files().is_held() {
            let conn = self
                .conn
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            forbid_checkpoint_on_close(conn);
        }
    }
}

impl std::fmt::Debug for SqliteIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The connection doesn't implement Debug usefully, so surface
        // just the type name rather than the full lock state.
        f.debug_struct("SqliteIndex").finish_non_exhaustive()
    }
}

impl SqliteIndex {
    /// Acquire the connection guard, **recovering from poisoning**.
    ///
    /// In a long-running host (`cdno-mcp-server`) a panic on one
    /// request while the guard is held would otherwise poison the
    /// mutex and turn every subsequent index call into a panic — a
    /// permanent all-500s zombie that no health check restarts. The
    /// connection state itself is sound after such a panic: rusqlite
    /// rolls back any transaction whose guard is dropped mid-panic,
    /// so the poison flag is the only residue and recovering it is
    /// safe. (Short-lived stdio/CLI processes never noticed — they
    /// exit and respawn.)
    fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, IndexError> {
        let conn = self.lock_conn_raw();
        if self.detached.load(std::sync::atomic::Ordering::Acquire) {
            return Err(IndexError::Detached);
        }
        Ok(conn)
    }

    /// The connection guard whatever it holds, for the file check and the
    /// reopen, which must reach the placeholder too.
    fn lock_conn_raw(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Open or create the index at `path`, creating missing parent
    /// directories and applying any pending migrations.
    ///
    /// A file SQLite refuses to open (not a database, or a schema too
    /// damaged to read) is deleted with its sidecars and a fresh one created
    /// in its place, for the caller's reconciliation to fill (#206). That
    /// self-heal takes no lock and cannot rebuild in place, since there is
    /// no connection to copy into: a process that already has the file
    /// open stays on the deleted file until it reopens (a server finds it
    /// moved on its next sweep, [`check_files`](Self::check_files)) or, for
    /// one that never checks (`cdno watch`), restarts. The removal takes no
    /// lock (there is no store here to take one from), so it leaves a window
    /// of microseconds: a server whose check lands between the sidecars'
    /// removal and the main file's sees only the sidecars gone and reopens on
    /// the doomed file, recreating sidecars at the path for it. The server's
    /// health loop checks the files again right after every reopen and
    /// reopens once more when the database moved meanwhile; the opener here,
    /// on a new empty file, discards a log left beside it. `cdno reindex` does not
    /// use it: it opens through
    /// [`open_location_unhealed`](Self::open_location_unhealed) and moves
    /// such a file aside under the vault write lock (#999).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, IndexError> {
        Self::open_healing(path.as_ref(), true)
    }

    /// [`open`](Self::open), with the self-heal only when `heal` is set.
    fn open_healing(path: &Path, heal: bool) -> Result<Self, IndexError> {
        // Ensure the containing directory exists so rusqlite can create
        // the file on first open. Silently a no-op if already present.
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| IndexError::Open {
                path: path.display().to_string(),
                source: e,
            })?;
        }

        match Self::open_connection(path) {
            Ok(index) => Ok(index),
            // The index is a cache (markdown is the source of truth), so
            // a corrupt file is safe to discard and rebuild (#206). Only
            // an *existing* file can be corrupt — a fresh-create failure
            // is a real environment problem, not a recoverable cache.
            // The caller's reconciliation repopulates the empty index.
            Err(err) if heal && path.exists() && is_recoverable_corruption(&err) => {
                remove_index_files(path);
                Self::open_connection(path)
            }
            Err(err) => Err(err),
        }
    }

    /// Open + configure + migrate a connection at `path`. The fallible
    /// core of [`open`](Self::open), split out so `open` can retry it on
    /// a fresh file after discarding a corrupt one.
    fn open_connection(path: &Path) -> Result<Self, IndexError> {
        Self::open_connection_sized(path, None)
    }

    /// [`open_connection`](Self::open_connection), setting the page size
    /// first when one is given. It only takes effect on a new, empty file.
    fn open_connection_sized(path: &Path, page_size: Option<u32>) -> Result<Self, IndexError> {
        let conn = open_configured(path, page_size)?;
        Ok(Self {
            held: Mutex::new(HeldFiles::of(&conn, path)),
            detached: std::sync::atomic::AtomicBool::new(false),
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
            binding: None,
        })
    }

    fn lock_held(&self) -> std::sync::MutexGuard<'_, HeldFiles> {
        self.held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Whether the files this index's connection reads and writes are still
    /// the ones its path names (#1000): the database file (its device and
    /// inode, and SQLite's own `SQLITE_FCNTL_HAS_MOVED`), and the `-wal`
    /// and `-shm` a WAL-mode connection keeps open from its open to its
    /// close. A few `stat` calls, so cheap enough for every sweep and every
    /// corruption-shaped error. A `stat` that fails other than with
    /// not-found gives [`FileCheck::Unchecked`], never a move.
    ///
    /// What it catches: the database deleted, moved aside or replaced at the
    /// path (`cdno reindex`'s set-aside, `reindex --rebind`, the open's
    /// self-heal, a restore), and the sidecars deleted or replaced under the
    /// connection: by a process on another kernel, whose locks this one
    /// cannot see, closing as the last connection, or by anything that is
    /// not SQLite (`rm`, `git clean -x`, a sync tool).
    ///
    /// What does not trip it: SQLite on one machine while this connection
    /// is open. A checkpoint, even `TRUNCATE`, keeps the log's inode, and no
    /// other SQLite connection on this kernel can take the exclusive lock
    /// that deleting the log needs while this one holds its shared lock.
    ///
    /// Off unix nothing is checked and this always answers
    /// [`FileCheck::Held`]: the identities come from `stat`'s device and
    /// inode, and only SQLite's unix VFS implements `HAS_MOVED`.
    pub fn check_files(&self) -> FileCheck {
        let conn = self.lock_conn_raw();
        let mut held = self.lock_held();
        if let Some(error) = &held.detached {
            return FileCheck::Detached(format!(
                "the index connection was closed and could not be opened again: {error}"
            ));
        }
        #[cfg(unix)]
        {
            compare_held(&conn, &mut held)
        }
        #[cfg(not(unix))]
        {
            let _ = (&conn, &mut held);
            FileCheck::Held
        }
    }

    /// Replace this index's connection with a fresh one at the same path,
    /// behind the same mutex, so every holder of the index carries on with
    /// no change: what to do when [`check_files`](Self::check_files) finds
    /// the files gone (#1000). The new connection is opened as at
    /// [`open_location`](Self::open_location), with its migrations, and an
    /// overridden index keeps its guards and its binding: a file at the path
    /// that is not cuaderno's index, or is bound to another vault, is
    /// refused with nothing changed, and an unbound one is bound to this
    /// vault. There is no self-heal: a file that will not open is returned
    /// as the error, for the caller to set aside under the vault write lock.
    ///
    /// Nothing is in flight on the old connection meanwhile: every
    /// statement and transaction runs under the mutex, which this holds for
    /// the swap. The old connection is closed without checkpointing and
    /// without deleting any file ([`close_detached`]). What it had committed
    /// only to its own orphaned files is dropped: the caller's
    /// reconciliation writes the notes' rows again, but archival snapshots
    /// are not derivable from the notes, so the old connection's snapshot
    /// rows are read before it closes and returned
    /// ([`Reopened::salvaged`]) for the caller to carry over.
    ///
    /// The order differs with what moved:
    ///
    /// - The database file at the path is another file and the sidecars at
    ///   the path are not the old connection's: the new connection is opened
    ///   first, and the old one is kept when that fails.
    /// - The database file is another file but the `-wal` or `-shm` at the
    ///   path is still the old connection's (a restore that renamed only the
    ///   main file over the path): a new connection would replay the old log
    ///   onto the new file and read the old rows. Refused with
    ///   [`IndexError::StaleSidecars`], nothing changed; the caller takes
    ///   the vault write lock and calls
    ///   [`reopen_setting_aside_stale_sidecars`](Self::reopen_setting_aside_stale_sidecars).
    /// - The same database file, only the sidecars went: the old connection
    ///   is closed first, since SQLite shares one shared-memory mapping per
    ///   database inode between the connections of a process, so a new
    ///   connection opened beside the old one would read through the old,
    ///   orphaned `-shm`.
    ///
    /// When the old connection was closed first and the open then fails, the
    /// index is left detached ([`FileCheck::Detached`]): every query fails
    /// with [`IndexError::Detached`] until a reopen succeeds.
    ///
    /// The process must hold no other connection to the file meanwhile, for
    /// the same reason; a server holds one.
    pub fn reopen(&self) -> Result<Reopened, IndexError> {
        self.reopen_inner(false)
    }

    /// [`reopen`](Self::reopen), moving the old file's sidecars aside to
    /// `<index>.stale-wal` / `<index>.stale-shm` when they are still at the
    /// path. The caller must hold the vault write lock: another process
    /// opening the path in between would read through them.
    pub fn reopen_setting_aside_stale_sidecars(&self) -> Result<Reopened, IndexError> {
        self.reopen_inner(true)
    }

    fn reopen_inner(&self, set_aside_stale: bool) -> Result<Reopened, IndexError> {
        let mut conn = self.lock_conn_raw();
        let mut held = self.lock_held();
        let mut reopened = Reopened::default();
        let detached = held.detached.is_some();
        // Decided from the files, not from the detached flag: after a failed
        // set-aside of stale sidecars the index is detached and the old
        // sidecars may still be at the path. A `stat` that fails decides
        // nothing, so nothing is closed or opened.
        let differs = database_differs(&conn, &held).map_err(|e| {
            IndexError::Update(format!(
                "could not check the index files, so the index was not reopened: {e}"
            ))
        })?;
        // Only when the database differs: a same-file reopen must never move
        // the database's own sidecars.
        let stale = differs && held_sidecars_at_path(&held);
        if stale && !set_aside_stale {
            return Err(IndexError::StaleSidecars(format!(
                "the database file {} was replaced but its -wal and -shm were left at the \
                 path, the old file's; opening it would replay the old log onto the new file",
                held.db_path.display()
            )));
        }
        if stale && self.binding.is_some() {
            for suffix in ["-wal", "-shm"] {
                let to = sibling(&held.db_path, &format!("{STALE_SUFFIX}{suffix}"));
                if !is_discardable_sidecar(&to, suffix) {
                    return Err(IndexError::NotAnIndex {
                        path: to.display().to_string(),
                    });
                }
            }
        }
        let same_file = !detached && !differs;
        // Read before the old connection closes; a detached index hands on
        // what the reopen that detached it read, so the reopen that finally
        // succeeds carries it.
        reopened.salvaged = if detached {
            held.salvaged.take()
        } else {
            Some(snapshots_on(&conn).map_err(|e| e.to_string()))
        };
        if same_file || stale {
            let placeholder = Connection::open_in_memory()?;
            close_detached(std::mem::replace(&mut *conn, placeholder));
            self.detached
                .store(true, std::sync::atomic::Ordering::Release);
            held.detached = Some("the index is being reopened".to_owned());
            if stale {
                for suffix in ["-wal", "-shm"] {
                    let from = sibling(&held.db_path, suffix);
                    let to = sibling(&held.db_path, &format!("{STALE_SUFFIX}{suffix}"));
                    let _ = fs::remove_file(&to);
                    match fs::rename(&from, &to) {
                        Ok(()) => reopened.stale_sidecars_moved_to.push(to),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => {
                            let e = IndexError::Open {
                                path: from.display().to_string(),
                                source: e,
                            };
                            held.detached = Some(e.to_string());
                            held.salvaged = reopened.salvaged.take();
                            return Err(e);
                        }
                    }
                }
            }
            match self.open_for_reopen() {
                Ok(fresh) => *conn = fresh,
                Err(e) => {
                    held.detached = Some(e.to_string());
                    held.salvaged = reopened.salvaged.take();
                    return Err(e);
                }
            }
        } else {
            let fresh = match self.open_for_reopen() {
                Ok(fresh) => fresh,
                Err(e) => {
                    if detached {
                        held.salvaged = reopened.salvaged.take();
                    }
                    return Err(e);
                }
            };
            close_detached(std::mem::replace(&mut *conn, fresh));
        }
        *held = HeldFiles::of(&conn, &self.path);
        self.detached
            .store(false, std::sync::atomic::Ordering::Release);
        Ok(reopened)
    }

    /// A new connection at this index's path for [`reopen`](Self::reopen),
    /// through the guards and binding rules of the open.
    fn open_for_reopen(&self) -> Result<Connection, IndexError> {
        let Some(root) = &self.binding else {
            return open_configured(&self.path, None);
        };
        Self::ensure_discardable(&self.path)?;
        let bound = Self::bound_vault_root(&self.path)?;
        if let Some(bound) = &bound
            && bound != root
        {
            return Err(IndexError::BoundToOtherVault {
                index: self.path.display().to_string(),
                bound: bound.clone(),
                root: root.clone(),
            });
        }
        let mut conn = open_configured(&self.path, None)?;
        if bound.is_none() {
            match bind_on(&mut conn, &self.path, root) {
                Ok(()) => {}
                Err(err) if err.is_corruption() => {}
                Err(err) => return Err(err),
            }
        }
        Ok(conn)
    }

    /// Where this index lives, as [`open_location`](Self::open_location)
    /// was given it: overridden when it is bound to a vault.
    pub fn location(&self) -> crate::paths::IndexLocation {
        crate::paths::IndexLocation {
            path: self.path.clone(),
            overridden: self.binding.is_some(),
        }
    }

    /// Open the index at `location` for the vault at `vault_root`.
    ///
    /// At the default location this is [`open`](Self::open), unchanged: the
    /// file is cuaderno's own, and the vault may legitimately be reached
    /// under two paths. An overridden location (#998) adds two guards:
    ///
    /// - **A file that is not recognisably cuaderno's index is never opened
    ///   read-write or discarded** ([`ensure_discardable`](Self::ensure_discardable)),
    ///   so a mistyped override cannot have another application's database
    ///   migrated, reconciled into, or deleted by the self-heal.
    /// - **The index is bound to one vault.** An open for a root other than
    ///   the recorded one is refused, read-only, before migrations or
    ///   reconciliation touch it. An index that records no root (new, or
    ///   from before migration 005) is bound on first use, atomically
    ///   ([`bind_vault_root`](Self::bind_vault_root)); one already bound to
    ///   this root is opened with no write for the binding. `cdno reindex
    ///   --rebind`, which deletes and recreates the file, is the way to give
    ///   it to another vault.
    ///
    /// A first binding that fails because the file is damaged does not fail
    /// the open: the file passed both guards, the binding is tried again at
    /// every open while none is recorded, and a rebuild
    /// ([`crate::reconcile::rebuild`]) writes it into the fresh index it
    /// copies in. Failing here would stop a long-running server from ever
    /// reaching the check that repairs the file (#526).
    pub fn open_location(
        location: &crate::paths::IndexLocation,
        vault_root: &Path,
    ) -> Result<Self, IndexError> {
        Self::open_location_healing(location, vault_root, true)
    }

    /// [`open_location`](Self::open_location) without the self-heal: a file
    /// SQLite refuses to open is returned as the error
    /// ([`IndexError::is_corruption`]) and left where it is, rather than
    /// deleted. For a caller that decides itself what to do with such a
    /// file, under the vault write lock: `cdno reindex`, which moves it
    /// aside there (#999).
    pub fn open_location_unhealed(
        location: &crate::paths::IndexLocation,
        vault_root: &Path,
    ) -> Result<Self, IndexError> {
        Self::open_location_healing(location, vault_root, false)
    }

    fn open_location_healing(
        location: &crate::paths::IndexLocation,
        vault_root: &Path,
        heal: bool,
    ) -> Result<Self, IndexError> {
        if !location.overridden {
            return Self::open_healing(&location.path, heal);
        }
        let path = location.path.as_path();
        Self::ensure_discardable(path)?;
        let root = canonical_vault_root(vault_root);
        let bound = Self::bound_vault_root(path)?;
        if let Some(bound) = &bound
            && *bound != root
        {
            return Err(IndexError::BoundToOtherVault {
                index: path.display().to_string(),
                bound: bound.clone(),
                root,
            });
        }
        let mut index = Self::open_healing(path, heal)?;
        if bound.is_none() {
            match index.bind_vault_root(path, &root) {
                Ok(()) => {}
                Err(err) if err.is_corruption() => {}
                Err(err) => return Err(err),
            }
        }
        index.binding = Some(root);
        Ok(index)
    }

    /// The canonical vault root this index was opened bound to, or `None`
    /// at the default location.
    pub fn binding(&self) -> Option<&str> {
        self.binding.as_deref()
    }

    /// The vault root recorded in this index's own `index_meta`, read
    /// through its connection; `None` when none is recorded.
    fn recorded_vault_root(&self) -> Result<Option<String>, IndexError> {
        let conn = self.lock_conn()?;
        conn.query_row(
            "SELECT value FROM index_meta WHERE key = ?1",
            params![VAULT_ROOT_KEY],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(IndexError::from)
    }

    /// Bind this index to the vault at `root` (a canonical root, as
    /// [`canonical_vault_root`] gives) unless it is bound already, and
    /// refuse when it is bound to another vault. Check and bind are one
    /// immediate transaction (`INSERT OR IGNORE`, then read back), so of two
    /// processes first-binding one file for different vaults, the second
    /// is refused rather than overwriting the first. `path` only names the
    /// file in the error.
    pub fn bind_vault_root(&self, path: &Path, root: &str) -> Result<(), IndexError> {
        bind_on(&mut *self.lock_conn()?, path, root)
    }

    /// A read-only connection for probing a file before deciding whether
    /// to open it for real, with the main connection's busy timeout so a
    /// writer elsewhere is waited for rather than mistaken for damage.
    ///
    /// It never creates a file beside `path`. A plain read-only connection
    /// to a WAL-mode database creates its `-wal` and `-shm` when they are
    /// missing, so when there is no `-wal` the file is opened `immutable`
    /// instead: no locks, no WAL, no shared memory, just the main file,
    /// which then holds every committed page. When a `-wal` exists, its
    /// pages may be the only copy of recent commits, so the probe reads
    /// through it; the sidecars are then the database's own, already there.
    fn open_probe(path: &Path) -> rusqlite::Result<Connection> {
        let flags =
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = if sibling(path, "-wal").exists() {
            Connection::open_with_flags(path, flags)?
        } else {
            Connection::open_with_flags(
                immutable_uri(path),
                flags | rusqlite::OpenFlags::SQLITE_OPEN_URI,
            )?
        };
        conn.busy_timeout(BUSY_TIMEOUT)?;
        Ok(conn)
    }

    /// The vault root an overridden index at `path` is bound to, read
    /// without writing: `None` when there is no file, it is empty, it
    /// records no root (an older schema, or an index never bound), or it is
    /// too damaged to read, which the self-heal then replaces.
    pub fn bound_vault_root(path: &Path) -> Result<Option<String>, IndexError> {
        match fs::metadata(path) {
            Ok(meta) if meta.len() > 0 => {}
            _ => return Ok(None),
        }
        let conn = match Self::open_probe(path) {
            Ok(conn) => conn,
            Err(err) if is_corruption(&err) => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let read = conn
            .query_row(
                "SELECT value FROM index_meta WHERE key = ?1",
                params![VAULT_ROOT_KEY],
                |r| r.get::<_, String>(0),
            )
            .optional();
        match read {
            Ok(value) => Ok(value),
            // No `index_meta` table yet: never bound.
            Err(rusqlite::Error::SqliteFailure(_, Some(msg))) if msg.contains("no such table") => {
                Ok(None)
            }
            Err(err) if is_corruption(&err) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Refuse to go on when an overridden `path` names an existing file that
    /// is not recognisably cuaderno's index. Called before anything opens
    /// it read-write, and before the self-heal or `cdno reindex` could
    /// delete it, so a mistyped override never alters or destroys another
    /// file.
    ///
    /// Discardable: no file, an empty file, or a file with the SQLite header
    /// whose read-only probe finds cuaderno's mark: the `schema_migrations`
    /// row for version 1 carrying migration 1's full description, which
    /// every cuaderno index has at every schema version. Any other SQLite
    /// database (another application's) is refused, whatever its tables are
    /// called. The probe creates no file beside it
    /// ([`open_probe`](Self::open_probe)).
    ///
    /// A file with the header whose schema the probe cannot read ("not a
    /// database", "malformed") stays discardable, so a damaged index still
    /// heals. That leaves a way to lose a foreign file: an override naming
    /// another application's database that reads as unreadable to this
    /// probe. A damaged one is already unreadable to its owner. Two known
    /// healthy cases read that way too: an encrypted database that keeps a
    /// plaintext header (as SQLCipher can be configured to), and a file
    /// whose header write version (byte 18) is above 2, which this SQLite
    /// does not understand. Both are accepted risks for a mistyped path
    /// that must also name such a file, because the alternative, refusing
    /// every unreadable file, would turn the self-heal off for exactly the
    /// indexes it exists for.
    pub fn ensure_discardable(path: &Path) -> Result<(), IndexError> {
        let not_an_index = || IndexError::NotAnIndex {
            path: path.display().to_string(),
        };
        let mut head = [0u8; 16];
        let read = match fs::File::open(path) {
            Ok(mut file) => {
                use std::io::Read;
                let mut filled = 0;
                loop {
                    match file.read(&mut head[filled..]) {
                        Ok(0) => break,
                        Ok(n) => {
                            filled += n;
                            if filled == head.len() {
                                break;
                            }
                        }
                        Err(e) => {
                            return Err(IndexError::Open {
                                path: path.display().to_string(),
                                source: e,
                            });
                        }
                    }
                }
                filled
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                return Err(IndexError::Open {
                    path: path.display().to_string(),
                    source: e,
                });
            }
        };
        if read == 0 {
            return Ok(());
        }
        if read < head.len() || &head != SQLITE_HEADER {
            return Err(not_an_index());
        }
        match has_cuaderno_mark(path) {
            Ok(true) => Ok(()),
            Ok(false) => Err(not_an_index()),
            Err(err) if is_corruption(&err) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    /// Delete the index file and its WAL/SHM sidecars. The index is a
    /// rebuildable cache, so the next `open` + reconcile rebuilds it from
    /// the markdown source of truth. A process that has the file open keeps
    /// the deleted one, so this is only for a file no process of this vault
    /// can be using: `cdno reindex --rebind` uses it on a file bound to
    /// another vault, which every open for this vault refuses. A rebuild of
    /// this vault's index copies in place instead
    /// ([`crate::reconcile::rebuild`]).
    pub fn remove(path: impl AsRef<Path>) {
        remove_index_files(path.as_ref());
    }

    /// Whether [`quick_check`](Self::quick_check) finds nothing: a yes/no
    /// for callers that do not need the problem lines. The servers call
    /// `quick_check` itself; see there for when it runs.
    pub fn check_integrity(&self) -> Result<bool, IndexError> {
        Ok(self.quick_check()?.is_empty())
    }

    /// The problems `PRAGMA quick_check` reports, at most
    /// [`CHECK_PROBLEM_LIMIT`] of them; empty when the file is sound.
    ///
    /// The cheap subset of [`integrity_check`](Self::integrity_check)
    /// that still catches the common corruption modes. Not run by
    /// [`open`](Self::open): a CLI command is a fresh start each time and
    /// would pay tens of milliseconds for it. A long-lived process opens
    /// once and can afford it, so the MCP servers run it at start
    /// (`cdno_domain::bootstrap::open_vault_checked`) and after every
    /// reconciliation sweep, and rebuild the index
    /// ([`crate::reconcile::rebuild`]) when it fails.
    ///
    /// Page-level damage can leave every ordinary query working while
    /// writes to the damaged pages fail, so this check, not query
    /// errors, is the dependable signal. A check that itself fails with
    /// a corruption code reports that error as its one problem rather
    /// than failing: the answer to "is it damaged?" is then yes.
    pub fn quick_check(&self) -> Result<Vec<String>, IndexError> {
        self.run_check("quick_check")
    }

    /// [`quick_check`](Self::quick_check)'s thorough sibling, `PRAGMA
    /// integrity_check`: it also compares every index with its table, so
    /// it finds rows missing from an index that `quick_check` skips. It
    /// is slower (it reads every index), so it runs on demand, not on
    /// every sweep.
    pub fn integrity_check(&self) -> Result<Vec<String>, IndexError> {
        self.run_check("integrity_check")
    }

    fn run_check(&self, pragma: &str) -> Result<Vec<String>, IndexError> {
        let conn = self.lock_conn()?;
        let rows = conn
            .prepare(&format!("PRAGMA {pragma}({CHECK_PROBLEM_LIMIT})"))
            .and_then(|mut stmt| {
                stmt.query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<String>>>()
            });
        match rows.map_err(IndexError::from) {
            Ok(rows) if rows.len() == 1 && rows[0] == "ok" => {
                // A sound file with no schema (an empty database) checks
                // as "ok" too, and reads as an empty vault (#520).
                let has_schema: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations')",
                    [],
                    |r| r.get(0),
                )?;
                if has_schema {
                    Ok(Vec::new())
                } else {
                    Ok(vec!["the index has no schema".to_string()])
                }
            }
            Ok(rows) => Ok(rows),
            Err(e) if e.is_corruption() => Ok(vec![e.to_string()]),
            Err(e) => Err(e),
        }
    }

    /// A new, empty index beside this one, under a name no other process
    /// or rebuild uses (`<index>.rebuild-<16 hex digits>`, next to the
    /// database file wherever that is), to be populated and then copied in
    /// with [`replace_contents_from`](Self::replace_contents_from). Its
    /// files are removed when the returned handle drops.
    ///
    /// An overridden index's fresh copy is bound to the same vault root
    /// before anything else is written to it, so the copy carries the
    /// binding in; it never inherits a binding from the live file, whose
    /// `index_meta` may be among the damage.
    ///
    /// The name is unique because two processes can find the same damage
    /// at once (two servers starting, a sweep beside a session's start);
    /// with one shared name each would delete or rename the other's
    /// half-built file. It is random rather than derived from the process
    /// id, since two servers in two containers on one vault are commonly
    /// both process 1. A name already taken is never reused: the main file
    /// is created exclusively, and a clash draws another name.
    pub fn open_fresh_beside(&self) -> Result<FreshIndex, IndexError> {
        let path = loop {
            let candidate = sibling(&self.path, &format!("{REBUILD_SUFFIX}-{}", random_tag()));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(_) => break candidate,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(IndexError::Open {
                        path: candidate.display().to_string(),
                        source: e,
                    });
                }
            }
        };
        // The backup copy refuses a WAL destination whose page size differs
        // from the source's, so the fresh file takes the live one's (read
        // from the header, which survives most damage). A header too
        // damaged to read leaves SQLite's default.
        let page_size: Option<u32> = self
            .lock_conn()
            .ok()
            .and_then(|c| c.query_row("PRAGMA page_size", [], |r| r.get(0)).ok());
        let opened = Self::open_connection_sized(&path, page_size).and_then(|mut index| {
            if let Some(root) = &self.binding {
                index.bind_vault_root(&path, root)?;
                index.binding = Some(root.clone());
            }
            Ok(index)
        });
        match opened {
            Ok(index) => Ok(FreshIndex {
                index: Some(Arc::new(index)),
                path,
                checked: std::sync::atomic::AtomicBool::new(false),
            }),
            Err(e) => {
                remove_index_files(&path);
                Err(e)
            }
        }
    }

    /// Replace everything this index holds with the contents of `fresh`,
    /// in place, through SQLite's online backup API.
    ///
    /// The file is never renamed or reopened: its pages are overwritten
    /// under SQLite's own write lock, so every connection to it on this
    /// machine, in this process and in any other (a second server, `cdno
    /// watch`, a CLI command), reads the new contents on its next query. A
    /// rename would repair only the process that did it and leave the
    /// others on the damaged file. Damaged pages do not stop the copy: it
    /// writes every page of the destination and reads none of its b-trees.
    ///
    /// This contains a fault on one machine (a torn write, a crash, a bad
    /// page). It is no protection for an index file shared between two
    /// machines or kernels: SQLite's locks do not cross that boundary, so
    /// the other side's writes keep damaging it after each copy. Each
    /// machine needs its own index ([`crate::paths::INDEX_PATH_ENV`]).
    ///
    /// Keeping a copy of the damaged file for diagnosis is the caller's
    /// choice, before it takes any lock: [`keep_previous_copy`](Self::keep_previous_copy).
    ///
    /// `fresh` must pass `quick_check` first, or nothing is touched; a
    /// check it already passed through [`FreshIndex::quick_check`] counts,
    /// so a caller that checked it before taking a lock is not made to
    /// repeat the scan under the lock. In [`crate::reconcile::rebuild`]
    /// that check passed before the last, locked pass and the snapshot
    /// carry wrote to the fresh index, so those last writes are copied in
    /// unchecked. That is accepted: they are ordinary transactions on a
    /// file this process created moments ago and nothing else writes, the
    /// same writes every commit makes unchecked, while a second scan would
    /// hold the vault lock, and every writer waiting on it, for the length
    /// of a whole-file read. For an
    /// overridden index, `fresh` must also be bound to this index's vault
    /// root ([`open_fresh_beside`](Self::open_fresh_beside) does that), and
    /// a live file that now records another vault's root is refused with
    /// [`IndexError::BoundToOtherVault`]: something rebound it since this
    /// process opened it, and overwriting it would take it back. A live
    /// binding too damaged to read does not stop the copy. A failed copy
    /// leaves the live file as it was, since the copy is one write
    /// transaction. The caller decides what else must hold still
    /// meanwhile: [`crate::reconcile::rebuild`] holds the vault write lock,
    /// so no commit by a process on this machine lands half before and half
    /// after.
    pub fn replace_contents_from(&self, fresh: &FreshIndex) -> Result<(), IndexError> {
        if !fresh.checked.load(std::sync::atomic::Ordering::Acquire) {
            let problems = fresh.quick_check()?;
            if !problems.is_empty() {
                return Err(IndexError::Update(format!(
                    "the rebuilt index failed its own check, so it was not copied in: {}",
                    problems.join("; ")
                )));
            }
        }
        if let Some(root) = &self.binding {
            let carried = fresh.sqlite().recorded_vault_root()?;
            if carried.as_deref() != Some(root.as_str()) {
                return Err(IndexError::Update(format!(
                    "the rebuilt index is not bound to the vault at {root}, so it was not \
                     copied in"
                )));
            }
            if let Ok(Some(live)) = self.recorded_vault_root()
                && live != *root
            {
                return Err(IndexError::BoundToOtherVault {
                    index: self.path.display().to_string(),
                    bound: live,
                    root: root.clone(),
                });
            }
        }
        let source = fresh.sqlite().lock_conn()?;
        let mut live = self.lock_conn()?;

        let backup = rusqlite::backup::Backup::new(&source, &mut live)?;
        let deadline = std::time::Instant::now() + REPLACE_TIMEOUT;
        loop {
            // -1: every page in one step, so the copy is one transaction.
            match backup.step(-1)? {
                rusqlite::backup::StepResult::Done => break,
                rusqlite::backup::StepResult::More => {}
                rusqlite::backup::StepResult::Busy | rusqlite::backup::StepResult::Locked => {
                    if std::time::Instant::now() >= deadline {
                        return Err(IndexError::Update(format!(
                            "the index stayed busy for {REPLACE_TIMEOUT:?}; nothing was replaced"
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Copy the index file and its WAL byte for byte to `<index>.previous`
    /// (and `.previous-wal`), replacing an older copy, for diagnosis. Best
    /// effort: a failure must not stop a repair. A copy taken while
    /// another process writes can be torn, which is acceptable for a file
    /// nothing reads back. The copy is of the WAL at the path, which is not
    /// what this process sees if its sidecars were unlinked under it.
    ///
    /// At an overridden location the name `<index>.previous` sits in a
    /// directory cuaderno does not own, so an existing file there is only
    /// replaced when it is recognisably a cuaderno index
    /// ([`ensure_discardable`](Self::ensure_discardable)); otherwise no copy
    /// is kept.
    ///
    /// Returns the copy's path when the database file was copied, `None`
    /// when no copy was kept.
    pub fn keep_previous_copy(&self) -> Option<PathBuf> {
        let previous = sibling(&self.path, PREVIOUS_SUFFIX);
        if self.binding.is_some() && Self::ensure_discardable(&previous).is_err() {
            return None;
        }
        keep_previous_copy(&self.path).then_some(previous)
    }

    /// Remove the rebuild files a killed rebuild left beside the index at
    /// `index_path` ([`open_fresh_beside`](Self::open_fresh_beside)'s
    /// `<index>.rebuild-<tag>`, with its `-wal` and `-shm`): nothing else
    /// ever deletes them. Only names of exactly that shape, so a file of
    /// someone else's that merely shares the prefix in an override's
    /// directory is left alone, and only ones untouched for an hour, so a
    /// rebuild running in another process right now keeps its files.
    pub fn clear_stale_rebuilds(index_path: &Path) {
        let (Some(dir), Some(name)) = (index_path.parent(), index_path.file_name()) else {
            return;
        };
        let name = name.to_string_lossy();
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if !is_rebuild_file_name(&name, &entry.file_name().to_string_lossy()) {
                continue;
            }
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > Duration::from_secs(3600));
            if stale {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// The database file this index reads and writes.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Move the index files at `location` aside to `<index>.previous`, so
    /// the next [`open`](Self::open) starts from an empty file. The last
    /// resort of a process that finds damage it cannot rebuild in place (at
    /// open, in `cdno reindex`, or in a server's repair, which then
    /// [`reopen`](Self::reopen)s); any other process still holding the file
    /// stays on the moved one until it reopens or restarts, so the caller
    /// must hold the vault write lock.
    ///
    /// At an overridden location an existing `.previous` that is not
    /// recognisably a cuaderno index is refused with
    /// [`IndexError::NotAnIndex`] rather than replaced, and nothing moves.
    pub fn set_aside(location: &crate::paths::IndexLocation) -> Result<(), IndexError> {
        let path = location.path.as_path();
        let previous = sibling(path, PREVIOUS_SUFFIX);
        if location.overridden {
            Self::ensure_discardable(&previous)?;
        }
        remove_index_files(&previous);
        move_index_files(path, &previous);
        Ok(())
    }

    /// Borrow the underlying connection under the mutex for ad-hoc
    /// operations that haven't been promoted to typed methods yet.
    ///
    /// Exists primarily for tests and for the CRUD work in issue #12.
    /// Prefer typed methods once they land.
    pub fn with_connection<F, T>(&self, f: F) -> Result<T, IndexError>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<T>,
    {
        let conn = self.lock_conn()?;
        f(&conn).map_err(IndexError::from)
    }
}

/// True for the SQLite error codes meaning "this file isn't a usable
/// database" — a truncated or corrupt index. The index is a rebuildable
/// cache, so these are recovered by dropping the file and reconciling
/// rather than surfaced to the user.
fn is_recoverable_corruption(err: &IndexError) -> bool {
    err.is_corruption()
}

/// The most problems an integrity check reports. The first few name the
/// damage; a full list on a badly broken file would only be noise.
pub const CHECK_PROBLEM_LIMIT: usize = 20;

/// Stem of the suffix a rebuild's fresh index is built under; each one
/// adds a random tag ([`SqliteIndex::open_fresh_beside`]).
pub const REBUILD_SUFFIX: &str = ".rebuild";

/// Sixteen hex digits that no other process is likely to draw: the
/// standard library's per-process random hash keys over the process id,
/// the time and a counter. No new dependency for a file name.
fn random_tag() -> String {
    use std::hash::{BuildHasher, Hasher};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    hasher.write_u64(NEXT.fetch_add(1, Ordering::Relaxed));
    if let Ok(since) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        hasher.write_u128(since.as_nanos());
    }
    format!("{:016x}", hasher.finish())
}

/// Whether `file_name` is a rebuild file of the index named `index_name`:
/// `<index_name>.rebuild-<tag>`, optionally with `-wal` or `-shm`, where
/// the tag is [`random_tag`]'s sixteen hex digits, or the `<pid>-<n>` an
/// earlier build of this branch wrote.
pub fn is_rebuild_file_name(index_name: &str, file_name: &str) -> bool {
    let Some(rest) = file_name
        .strip_prefix(index_name)
        .and_then(|r| r.strip_prefix(REBUILD_SUFFIX))
        .and_then(|r| r.strip_prefix('-'))
    else {
        return false;
    };
    let rest = rest
        .strip_suffix("-wal")
        .or_else(|| rest.strip_suffix("-shm"))
        .unwrap_or(rest);
    let hex = rest.len() == 16 && rest.bytes().all(|b| b.is_ascii_hexdigit());
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    let legacy = rest
        .split_once('-')
        .is_some_and(|(pid, n)| digits(pid) && digits(n));
    hex || legacy
}

/// How long [`SqliteIndex::replace_contents_from`] waits for the live
/// file's write lock before giving up: the same as a writer waits for the
/// vault lock, which the caller holds meanwhile.
const REPLACE_TIMEOUT: Duration = Duration::from_secs(5);

/// A fresh index built beside a live one, for a rebuild. Its files are
/// removed when it drops.
pub struct FreshIndex {
    // `Option` so `drop` can release the connection before deleting the
    // files it has open.
    index: Option<Arc<SqliteIndex>>,
    path: PathBuf,
    /// Set once [`quick_check`](Self::quick_check) has found it sound.
    checked: std::sync::atomic::AtomicBool,
}

impl FreshIndex {
    /// The fresh index, to populate through the trait.
    pub fn index(&self) -> Arc<dyn VaultIndex> {
        self.sqlite_arc()
    }

    fn sqlite_arc(&self) -> Arc<SqliteIndex> {
        self.index.clone().expect("present until drop")
    }

    fn sqlite(&self) -> &SqliteIndex {
        self.index.as_deref().expect("present until drop")
    }

    /// `quick_check` on the fresh index, so a caller can check it before
    /// taking a lock. A pass is remembered, and
    /// [`SqliteIndex::replace_contents_from`] does not repeat it.
    pub fn quick_check(&self) -> Result<Vec<String>, IndexError> {
        let problems = self.sqlite().quick_check()?;
        self.checked
            .store(problems.is_empty(), std::sync::atomic::Ordering::Release);
        Ok(problems)
    }

    /// Where its database file is.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FreshIndex {
    fn drop(&mut self) {
        drop(self.index.take());
        remove_index_files(&self.path);
    }
}

/// Copy the index file and its WAL to `<index>.previous` (and
/// `.previous-wal`), replacing an older copy. Best effort: it is for
/// diagnosis only, and a failure must not stop a repair. True when the
/// database file itself was copied.
fn keep_previous_copy(path: &Path) -> bool {
    let previous = sibling(path, PREVIOUS_SUFFIX);
    remove_index_files(&previous);
    let copied = fs::copy(path, &previous).is_ok();
    let wal = sibling(path, "-wal");
    if wal.exists() {
        let _ = fs::copy(&wal, sibling(&previous, "-wal"));
    }
    copied
}

/// The path of the `.previous` copy beside the index at `index_path`: where
/// [`SqliteIndex::keep_previous_copy`] copies a damaged file and
/// [`SqliteIndex::set_aside`] moves one.
pub fn previous_copy_path(index_path: &Path) -> PathBuf {
    sibling(index_path, PREVIOUS_SUFFIX)
}

/// Suffix of the copy of the replaced index a rebuild keeps, one
/// generation deep.
pub const PREVIOUS_SUFFIX: &str = ".previous";

/// The main file and SQLite's sidecars beside it, the write-ahead log and
/// its shared-memory index, sidecars first: a removal that stops half way
/// leaves a database without a log (an older, consistent state), never an
/// old log beside a missing or new database, which a new connection would
/// replay onto it.
const SIDECARS: [&str; 3] = ["-wal", "-shm", ""];

/// `path` with `suffix` appended to its file name (`index.db` →
/// `index.db.previous`, `index.db-wal`).
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Whether the SQLite database at `path` carries cuaderno's own mark,
/// read on a probe connection: a `schema_migrations` row for version 1
/// whose description is migration 1's, word for word. A table name alone
/// (`index_meta`, `schema_migrations`) is too generic to count. A
/// corruption error is returned as is, for the caller to decide.
fn has_cuaderno_mark(path: &Path) -> rusqlite::Result<bool> {
    has_cuaderno_mark_on(&SqliteIndex::open_probe(path)?)
}

/// [`has_cuaderno_mark`] on an open connection.
fn has_cuaderno_mark_on(conn: &Connection) -> rusqlite::Result<bool> {
    let has_migrations: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' \
         AND name = 'schema_migrations')",
        [],
        |r| r.get(0),
    )?;
    if !has_migrations {
        return Ok(false);
    }
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 1 AND description = ?1)",
        [MIGRATIONS[0].1],
        |r| r.get::<_, bool>(0),
    )
}

/// A `file:` URI opening `path` read-only and immutable. Every byte outside
/// the unreserved set and `/` is percent-encoded, so a path holding `?`,
/// `#` or `%` cannot be read as URI syntax.
fn immutable_uri(path: &Path) -> String {
    let mut uri = String::from("file:");
    for byte in path.as_os_str().as_encoded_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(char::from(*byte));
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    uri
}

/// The SQLite error codes of a damaged file, as [`IndexError::is_corruption`]
/// reads them, for a bare `rusqlite` error.
fn is_corruption(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, _)
            if matches!(
                e.code,
                rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
            )
    )
}

/// Remove the SQLite index file and its `-wal` / `-shm` sidecars,
/// ignoring not-found. Used to drop a corrupt cache before rebuilding.
fn remove_index_files(path: &Path) {
    for suffix in SIDECARS {
        let _ = fs::remove_file(sibling(path, suffix));
    }
}

/// Rename the index file and its sidecars from `from` to `to`, each
/// keeping its own suffix, ignoring any that do not exist. The sidecars
/// go first: a process that opens `from` in between then finds the main
/// file without its WAL and reads an older, consistent state, rather than
/// a new main file beside the old WAL, which SQLite would replay onto it.
fn move_index_files(from: &Path, to: &Path) {
    for suffix in ["-wal", "-shm", ""] {
        let _ = fs::rename(sibling(from, suffix), sibling(to, suffix));
    }
}

/// Open, configure and migrate a connection at `path`, setting the page
/// size first when one is given (it only takes effect on a new, empty
/// file).
fn open_configured(path: &Path, page_size: Option<u32>) -> Result<Connection, IndexError> {
    let conn = Connection::open(path)?;
    if let Some(size) = page_size {
        conn.pragma_update(None, "page_size", size)?;
    }
    configure_connection(&conn)?;
    apply_pending_migrations(&conn)?;
    Ok(conn)
}

/// [`SqliteIndex::bind_vault_root`] on a bare connection.
fn bind_on(conn: &mut Connection, path: &Path, root: &str) -> Result<(), IndexError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT OR IGNORE INTO index_meta (key, value) VALUES (?1, ?2)",
        params![VAULT_ROOT_KEY, root],
    )?;
    let bound: String = tx.query_row(
        "SELECT value FROM index_meta WHERE key = ?1",
        params![VAULT_ROOT_KEY],
        |r| r.get(0),
    )?;
    tx.commit()?;
    if bound == root {
        Ok(())
    } else {
        Err(IndexError::BoundToOtherVault {
            index: path.display().to_string(),
            bound,
            root: root.to_owned(),
        })
    }
}

/// Apply the PRAGMAs required by the schema contract on every new
/// connection. Most of these are per-connection (not persisted in the
/// file) and must be re-set on each open.
fn configure_connection(conn: &Connection) -> Result<(), IndexError> {
    // Cascade deletes on foreign keys. Without this the ON DELETE
    // CASCADE clauses in the schema are silently ignored.
    conn.pragma_update(None, "foreign_keys", "ON")?;

    // WAL mode lets readers proceed concurrently with a writer. This
    // is persisted in the database header, but re-setting is cheap
    // and documents intent.
    conn.pragma_update(None, "journal_mode", "WAL")?;

    // With WAL, NORMAL sync is durable under crash and much faster
    // than FULL. See https://www.sqlite.org/pragma.html#pragma_synchronous.
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    // Keep temp tables in memory; cheap speedup with no durability cost.
    conn.pragma_update(None, "temp_store", "MEMORY")?;

    // Avoid spurious SQLITE_BUSY on brief writer contention. 5s is
    // generous for a personal-scale vault but trips quickly enough
    // that a genuine deadlock still surfaces.
    conn.busy_timeout(BUSY_TIMEOUT)?;

    Ok(())
}

/// Read the current schema version (0 if the `schema_migrations`
/// table doesn't exist yet) and apply every migration whose version
/// exceeds it. Each migration runs in a transaction so a partial
/// failure leaves the database at the previous version.
fn apply_pending_migrations(conn: &Connection) -> Result<(), IndexError> {
    let current = current_schema_version(conn)?;

    for (version, description, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        apply_one_migration(conn, *version, description, sql)?;
    }

    Ok(())
}

/// Return the maximum applied migration version, or 0 if no migration
/// has ever run on this database.
fn current_schema_version(conn: &Connection) -> Result<u32, IndexError> {
    // Probe for the migrations table first — on a fresh database it
    // doesn't exist, and querying MAX(version) would error.
    let has_table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations')",
            [],
            |r| {
                let b: i64 = r.get(0)?;
                Ok(b != 0)
            },
        )?;

    if !has_table {
        return Ok(0);
    }

    // Fresh tables (after a migration that created the row) will have
    // one or more entries; pre-migration state has none — treat that
    // as version 0 so the first migration runs.
    let version: Option<u32> =
        conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })?;
    Ok(version.unwrap_or(0))
}

/// Apply a single migration inside a transaction. The migration SQL
/// is expected to be self-contained — it creates its own schema
/// objects and inserts its own `schema_migrations` row.
fn apply_one_migration(
    conn: &Connection,
    version: u32,
    description: &str,
    sql: &str,
) -> Result<(), IndexError> {
    conn.execute_batch(&format!("BEGIN; {sql} COMMIT;"))
        .map_err(|e| IndexError::Migration {
            version,
            reason: format!("{description}: {e}"),
        })
}

// ---------------------------------------------------------------------
// VaultIndex trait + entry types + SqliteIndex implementation
// ---------------------------------------------------------------------

/// Plain-data view of a row in the `notes` table.
///
/// Kept as a value type with no behaviour: this is the data the index
/// hands back, and what callers pass in to `upsert_note`.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteEntry {
    pub path: VaultPath,
    pub note_type: String,
    pub title: Option<String>,
    pub content_hash: String,
    pub mtime_ns: u64,
    pub size: u64,
    /// Full frontmatter as JSON. Stored as `TEXT` in SQLite; deserialised
    /// on the way out so callers get a structured value.
    pub frontmatter: JsonValue,
    pub indexed_at_ns: u64,
}

/// A note as an addressable candidate: enough to label it in a picker and
/// open it, and nothing else.
///
/// This is a read-side projection, deliberately not a [`NoteEntry`]. Two
/// reasons, and the first is the load-bearing one:
///
/// 1. **The title comes from `notes_fts.title`, the note's body H1 — not
///    [`NoteEntry::title`]**, which is the frontmatter `title:` field and is
///    absent on essentially every Cuaderno note (see
///    [`crate::extractors::first_h1`]).
///    A candidate list built off `NoteEntry` would be label-less.
/// 2. `NoteEntry` carries the full frontmatter JSON blob, which would be
///    deserialised for every note in the vault to reach a field this never
///    reads.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteCandidate {
    pub path: VaultPath,
    pub note_type: String,
    /// The note's body H1. `None` when the note has no FTS row yet or has no
    /// H1 — a candidate with a weaker label still beats one that vanished, so
    /// callers render a fallback rather than dropping the row.
    pub title: Option<String>,
    pub mtime_ns: u64,
}

/// One row of the `deadlines` table, scoped to a single note.
#[derive(Debug, Clone, PartialEq)]
pub struct DeadlineEntry {
    /// Which of the three aggregation sources this deadline came from:
    /// `"project_milestone" | "stewardship_periodic" | "commitment_note"`.
    pub source: String,
    pub title: String,
    /// ISO-8601 `YYYY-MM-DD`.
    pub due_date: String,
    pub is_hard: bool,
    pub context: Option<String>,
}

/// One row of the `links` table, scoped to a single source note.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkEntry {
    pub target_raw: String,
    pub resolved_path: Option<VaultPath>,
    pub label: Option<String>,
    /// The link resolved through a note's `aliases:` entry rather than a
    /// path or stem (RFC 0007 §5.5; the alias rule of
    /// [`crate::extractors::resolve_wikilinks`]). Always `false` when
    /// `resolved_path` is `None`.
    pub via_alias: bool,
}

/// One distinct resolution held by `links` rows, as
/// [`VaultIndex::link_resolutions`] reports it: the target text, where it
/// resolves, and whether it got there through an alias.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LinkResolution {
    pub target_raw: String,
    pub resolved_path: Option<VaultPath>,
    pub via_alias: bool,
}

/// One conditional update for [`VaultIndex::set_link_resolutions`]: the
/// rows with this `target_raw` currently resolved to `from` move to `to`,
/// with `via_alias` set to whether `to` was reached through an alias.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkResolutionUpdate {
    pub target_raw: String,
    pub from: Option<VaultPath>,
    pub to: Option<VaultPath>,
    pub via_alias: bool,
}

/// One row of the `milestones` table, scoped to a single project note.
///
/// A superset of [`DeadlineEntry`]'s project-milestone source: it
/// captures soft targets and completed markers as well as hard
/// deadlines. `date` is `None` for non-date markers (`target: April`),
/// which can't participate in date-window queries.
#[derive(Debug, Clone, PartialEq)]
pub struct MilestoneEntry {
    pub name: String,
    /// ISO `YYYY-MM-DD`, or `None` for a non-date marker.
    pub date: Option<String>,
    /// `true` for `hard:` deadlines; `false` for soft targets and
    /// keyword-less completed markers.
    pub is_hard: bool,
    /// `true` when the source checkbox is `- [x]`.
    pub completed: bool,
}

/// Snapshot of an action note at the moment it was archived to
/// `actions/_done/<year>/`. The append-only lint (#111, design §5.11)
/// re-hashes the file's first `frozen_size` bytes on each lint run
/// and flags any divergence; bytes appended past `frozen_size` are
/// allowed (late retrospectives). Set once by `stage_action_archival`
/// and never overwritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivalSnapshot {
    /// File length captured at archival, in bytes.
    pub frozen_size: u64,
    /// xxh3_64 hex digest of the file content at archival.
    pub frozen_hash: String,
    /// Wall-clock instant of the archival, ns since the UNIX epoch.
    pub archived_at_ns: u64,
    /// Hash of the same content with references masked
    /// ([`crate::hash::masked_content_hash`]); `None` until stamped or
    /// backfilled.
    pub masked_hash: Option<String>,
    /// The [`crate::hash::MASK_VERSION`] `masked_hash` was computed under.
    pub mask_version: Option<u32>,
    /// Byte length of the masked prefix ([`crate::hash::mask_references`]).
    /// Unlike `frozen_size` a rename cannot move it; verification hashes the
    /// first `masked_size` bytes of the masked current file.
    pub masked_size: Option<u64>,
}

/// One ranked full-text search result from [`VaultIndex::search`].
///
/// `snippet` is an excerpt of the matched body with the query terms
/// wrapped in `[`…`]`, straight from FTS5's `snippet()`. `score` is the
/// raw `bm25()` relevance value — **lower is a better match** (it is the
/// sort key, surfaced so callers can threshold or display it). `note_type`
/// is joined in from the `notes` row so callers can filter/group without a
/// second lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub path: VaultPath,
    pub note_type: String,
    pub title: Option<String>,
    pub snippet: String,
    pub score: f64,
}

/// Cache-oriented query API for the vault index.
///
/// All methods take `&self`; implementations are responsible for
/// interior mutability and transaction boundaries. Each `replace_*`
/// call is expected to be atomic — a partial failure must leave the
/// prior facet set intact.
///
/// The trait is `Send + Sync` so one implementation can be shared
/// through an `Arc` across MCP handlers, Tauri commands, and
/// background reconciliation.
pub trait VaultIndex: Send + Sync {
    // notes -----------------------------------------------------------
    fn upsert_note(&self, entry: &NoteEntry) -> Result<(), IndexError>;
    fn remove_note(&self, path: &VaultPath) -> Result<(), IndexError>;
    fn find_by_path(&self, path: &VaultPath) -> Result<Option<NoteEntry>, IndexError>;
    fn list_by_type(&self, note_type: &str) -> Result<Vec<NoteEntry>, IndexError>;
    /// Return every path currently in the index. Used by reconciliation
    /// to find orphans (index rows with no corresponding file on disk).
    fn list_all_paths(&self) -> Result<Vec<VaultPath>, IndexError>;
    /// Every slug-shaped alias any indexed note declares, paired with the
    /// note's path (RFC 0007 §5.5).
    ///
    /// Reads the top-level frontmatter key `aliases`, in either of the two
    /// shapes Obsidian accepts: a list of strings, or a single scalar string.
    /// An item that is not a string, or is not slug-shaped
    /// ([`is_slug_shaped`](crate::slug::is_slug_shaped)), is skipped, so a
    /// display name such as `"Surrogate model"` contributes nothing. Ordered by
    /// path (compared as text), then by position in the note's list.
    ///
    /// Rows are not deduplicated: an alias is returned once per occurrence,
    /// so one two notes declare yields two rows (and a note listing it twice,
    /// two), and resolution treats it as ambiguous (§5.5).
    fn list_aliases(&self) -> Result<Vec<(String, VaultPath)>, IndexError>;
    /// Every indexed note as a [`NoteCandidate`], most-recently-modified
    /// first. The listing behind `cdno open`'s picker and `--list`.
    ///
    /// Ordering is part of the contract: recency is the useful default for a
    /// navigation surface, and the secondary sort on path keeps the result
    /// deterministic when two notes share an mtime.
    fn list_candidates(&self) -> Result<Vec<NoteCandidate>, IndexError>;

    // deadlines -------------------------------------------------------
    fn replace_deadlines(
        &self,
        path: &VaultPath,
        deadlines: &[DeadlineEntry],
    ) -> Result<(), IndexError>;
    fn deadlines_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, DeadlineEntry)>, IndexError>;

    // links -----------------------------------------------------------
    fn replace_links(&self, path: &VaultPath, links: &[LinkEntry]) -> Result<(), IndexError>;
    fn find_backlinks(&self, path: &VaultPath) -> Result<Vec<VaultPath>, IndexError>;
    fn find_outgoing_links(&self, path: &VaultPath) -> Result<Vec<LinkEntry>, IndexError>;
    /// Every distinct [`LinkResolution`] (`target_raw`, `resolved_path`,
    /// `via_alias`) across all `links` rows, in no particular order.
    /// Resolution is a function of the target text, the vault's path set and
    /// its alias map alone, so this is the whole input the stale-link
    /// re-resolution pass needs (#640): it re-resolves the rows that may be
    /// stale, without re-reading any source note.
    fn link_resolutions(&self) -> Result<Vec<LinkResolution>, IndexError>;
    /// Apply each [`LinkResolutionUpdate`] in order, atomically: set
    /// `resolved_path` to `to` and `via_alias` to `via_alias` on every
    /// `links` row whose `target_raw` is `target_raw` and whose
    /// `resolved_path` currently equals `from` (`None` matching an
    /// unresolved row). Returns the number of rows changed. The `from` guard
    /// keeps a row resolved differently since it was read from being
    /// overwritten with a stale answer.
    fn set_link_resolutions(&self, updates: &[LinkResolutionUpdate]) -> Result<usize, IndexError>;

    // tags ------------------------------------------------------------
    fn replace_tags(&self, path: &VaultPath, tags: &[String]) -> Result<(), IndexError>;
    fn find_by_tag(&self, tag: &str) -> Result<Vec<VaultPath>, IndexError>;

    // milestones ------------------------------------------------------
    fn replace_milestones(
        &self,
        path: &VaultPath,
        milestones: &[MilestoneEntry],
    ) -> Result<(), IndexError>;
    /// Every milestone of the project at `path`, in source order
    /// (by row id). The caller resolves where the project lives; the
    /// index does not guess.
    fn milestones_for_project(&self, path: &VaultPath) -> Result<Vec<MilestoneEntry>, IndexError>;
    /// Dated milestones across all projects whose `date` falls in the
    /// inclusive `[from, to]` window, sorted by date. Non-date markers
    /// (`date IS NULL`) are excluded — they can't be placed on a
    /// timeline. This is the source the commitments aggregation (#32)
    /// reads project deadlines from.
    fn milestones_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, MilestoneEntry)>, IndexError>;

    // full-text search ------------------------------------------------
    /// Replace the FTS row for `path` with the given title + body
    /// (delete-then-insert; FTS5 has no clean per-row UPSERT). Mirrors
    /// the `replace_*` facet idiom: the note's searchable text is just
    /// another projection of the note, refreshed on every write.
    fn replace_fts(
        &self,
        path: &VaultPath,
        title: Option<&str>,
        body: &str,
    ) -> Result<(), IndexError>;
    /// Full-text search over title + body, ranked best-first. `query` is
    /// an FTS5 MATCH expression (bare terms, `"phrases"`, `AND`/`OR`,
    /// `prefix*`). Title matches are weighted above body matches. At most
    /// `limit` hits are returned.
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, IndexError>;
    /// Every path currently present in the FTS index. Used by the
    /// reconciliation FTS-heal pass to diff against `notes` and backfill
    /// any note missing from search (independent of the per-file hash
    /// fast-path, so it fills a just-migrated/dropped FTS table).
    fn fts_indexed_paths(&self) -> Result<Vec<VaultPath>, IndexError>;

    // archival snapshots -------------------------------------------------
    /// Record (or replace) the archival snapshot for `path`. Written
    /// once at archival by `stage_action_archival`; subsequent writes
    /// for the same path simply overwrite, which is harmless because
    /// the snapshot only matters as a stable per-path baseline.
    fn record_archival_snapshot(
        &self,
        path: &VaultPath,
        snapshot: &ArchivalSnapshot,
    ) -> Result<(), IndexError>;
    /// Look up the archival snapshot for `path`, or `None` if there
    /// isn't one (the note was never archived through the lifecycle).
    fn find_archival_snapshot(
        &self,
        path: &VaultPath,
    ) -> Result<Option<ArchivalSnapshot>, IndexError>;
    /// Every archival snapshot row, paired with its note path, sorted by
    /// path. The startup backfill of the masked hash (RFC 0007 T16) reads
    /// it to find the rows archived before migration 004.
    fn list_archival_snapshots(&self) -> Result<Vec<(VaultPath, ArchivalSnapshot)>, IndexError>;
}

impl VaultIndex for SqliteIndex {
    fn upsert_note(&self, entry: &NoteEntry) -> Result<(), IndexError> {
        // INSERT OR REPLACE is shorthand for "delete-if-exists then
        // insert". For the notes table that's exactly the semantics we
        // want: upsert by primary key `path`. The dependent tables
        // (deadlines, links, note_tags) are refreshed through their own
        // `replace_*` calls — cascades from INSERT OR REPLACE would
        // silently drop those rows, which would be surprising.
        let frontmatter_json = serde_json::to_string(&entry.frontmatter)
            .map_err(|e| IndexError::Update(format!("failed to serialise frontmatter: {e}")))?;
        let conn = self.lock_conn()?;
        conn.execute(
            "INSERT INTO notes (path, note_type, title, content_hash, mtime_ns, size, frontmatter, indexed_at_ns) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(path) DO UPDATE SET \
                 note_type = excluded.note_type, \
                 title = excluded.title, \
                 content_hash = excluded.content_hash, \
                 mtime_ns = excluded.mtime_ns, \
                 size = excluded.size, \
                 frontmatter = excluded.frontmatter, \
                 indexed_at_ns = excluded.indexed_at_ns",
            params![
                entry.path.to_string(),
                entry.note_type,
                entry.title,
                entry.content_hash,
                entry.mtime_ns as i64,
                entry.size as i64,
                frontmatter_json,
                entry.indexed_at_ns as i64,
            ],
        )?;
        Ok(())
    }

    fn remove_note(&self, path: &VaultPath) -> Result<(), IndexError> {
        // Cascade FKs drop dependent rows in deadlines, links, note_tags.
        // `notes_fts` is a virtual table, so FK cascade doesn't reach it —
        // delete its row explicitly under the same lock to keep search in
        // step with the notes table.
        let conn = self.lock_conn()?;
        conn.execute(
            "DELETE FROM notes WHERE path = ?1",
            params![path.to_string()],
        )?;
        conn.execute(
            "DELETE FROM notes_fts WHERE path = ?1",
            params![path.to_string()],
        )?;
        Ok(())
    }

    fn find_by_path(&self, path: &VaultPath) -> Result<Option<NoteEntry>, IndexError> {
        let conn = self.lock_conn()?;
        conn.query_row(
            "SELECT path, note_type, title, content_hash, mtime_ns, size, frontmatter, indexed_at_ns \
             FROM notes WHERE path = ?1",
            params![path.to_string()],
            row_to_note_entry,
        )
        .optional()
        .map_err(IndexError::from)?
        .transpose()
    }

    fn list_by_type(&self, note_type: &str) -> Result<Vec<NoteEntry>, IndexError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT path, note_type, title, content_hash, mtime_ns, size, frontmatter, indexed_at_ns \
             FROM notes WHERE note_type = ?1 ORDER BY path",
        )?;
        let rows = stmt.query_map(params![note_type], row_to_note_entry)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row??);
        }
        Ok(out)
    }

    fn list_all_paths(&self) -> Result<Vec<VaultPath>, IndexError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare("SELECT path FROM notes ORDER BY path")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let path_str = row?;
            out.push(VaultPath::new(path_str).map_err(|e| {
                IndexError::Query(format!("invalid stored path in list_all_paths: {e}"))
            })?);
        }
        Ok(out)
    }

    fn list_aliases(&self) -> Result<Vec<(String, VaultPath)>, IndexError> {
        let conn = self.lock_conn()?;
        // One query. `json_quote(json_extract(..))` yields valid JSON text
        // for both shapes (an array comes back as itself, a string as a
        // quoted string), so Rust parses one form and `alias_slugs` does the
        // shape filtering; `json_type` keeps rows without `aliases`, or with
        // a number or map there, out of the result set.
        let mut stmt = conn.prepare(
            "SELECT path, json_quote(json_extract(frontmatter, '$.aliases')) FROM notes \
             WHERE json_type(frontmatter, '$.aliases') IN ('array', 'text') \
             ORDER BY path",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (path_str, aliases_json) = row?;
            let path = VaultPath::new(path_str).map_err(|e| {
                IndexError::Query(format!("invalid stored path in list_aliases: {e}"))
            })?;
            let aliases: JsonValue = serde_json::from_str(&aliases_json)
                .map_err(|e| IndexError::Query(format!("invalid aliases JSON: {e}")))?;
            for alias in alias_slugs(&aliases) {
                out.push((alias, path.clone()));
            }
        }
        Ok(out)
    }

    fn list_candidates(&self) -> Result<Vec<NoteCandidate>, IndexError> {
        let conn = self.lock_conn()?;

        // Two queries and a map, rather than the LEFT JOIN this obviously
        // wants to be. `notes_fts` is an FTS5 virtual table whose `path` is
        // UNINDEXED, and a virtual table cannot carry an index, so SQLite can
        // only satisfy the join by rescanning it once per `notes` row —
        // quadratic. Measured on this exact schema: 1k notes 0.07s, 5k 1.8s,
        // 10k 7.1s, against ~6ms for the form below. That would land on every
        // TAB press, since shell completion calls this.
        //
        // The map is built first so the second pass can stream, and a missing
        // entry yields `None` — the same semantics the LEFT JOIN had, and it
        // matters: a note with no FTS row is a real, openable note that
        // simply has no title yet.
        let mut titles: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();
        {
            let mut stmt = conn.prepare("SELECT path, title FROM notes_fts")?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?;
            for row in rows {
                let (path, title) = row?;
                titles.insert(path, title);
            }
        }

        // The `path` tiebreak is deliberate: mtimes collide readily (a batch
        // write, a fresh `cdno init`), and an unstable order would make tests
        // flake and the picker reshuffle between invocations.
        let mut stmt = conn.prepare(
            "SELECT path, note_type, mtime_ns FROM notes \
             ORDER BY mtime_ns DESC, path ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? as u64,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (path_str, note_type, mtime_ns) = row?;
            let title = titles.get(&path_str).cloned().flatten();
            out.push(NoteCandidate {
                path: VaultPath::new(&path_str).map_err(|e| {
                    IndexError::Query(format!("invalid stored path in list_candidates: {e}"))
                })?,
                note_type,
                title,
                mtime_ns,
            });
        }
        Ok(out)
    }

    fn replace_fts(
        &self,
        path: &VaultPath,
        title: Option<&str>,
        body: &str,
    ) -> Result<(), IndexError> {
        // Delete-then-insert under one transaction: FTS5 regular tables
        // have no clean per-row UPSERT, and an UNINDEXED key column can't
        // anchor an ON CONFLICT. The delete is keyed on the UNINDEXED
        // `path` column (an O(n) scan — negligible at vault scale; a
        // path->rowid side-table is the optimisation if a profile ever
        // shows it).
        let mut conn = self.lock_conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM notes_fts WHERE path = ?1",
            params![path.to_string()],
        )?;
        tx.execute(
            "INSERT INTO notes_fts (path, title, body) VALUES (?1, ?2, ?3)",
            params![path.to_string(), title, body],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, IndexError> {
        let conn = self.lock_conn()?;
        // Column order in `notes_fts` is (path, title, body) => indices
        // (0, 1, 2). bm25 weights mirror that: path contributes nothing
        // (UNINDEXED anyway), a title hit is weighted 10x a body hit so a
        // note *about* the query outranks one that mentions it in passing.
        // bm25 returns lower-is-better, so ORDER BY ascending is best-first.
        // `snippet`'s column arg is -1 (auto-select): it excerpts whichever
        // column actually matched, so a title-only hit brackets the title
        // term instead of returning an unrelated body prefix.
        // The JOIN to `notes` is inner by design — an FTS row with no
        // surviving note (transient reconcile state) is dropped, not shown.
        let mut stmt = conn.prepare(
            "SELECT f.path, n.note_type, f.title, \
                    snippet(notes_fts, -1, '[', ']', '…', 10), \
                    bm25(notes_fts, 0.0, 10.0, 1.0) \
             FROM notes_fts f \
             JOIN notes n ON n.path = f.path \
             WHERE notes_fts MATCH ?1 \
             ORDER BY bm25(notes_fts, 0.0, 10.0, 1.0) \
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |row| {
            let path_str: String = row.get(0)?;
            Ok((
                path_str,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (path_str, note_type, title, snippet, score) = row?;
            let path = VaultPath::new(&path_str)
                .map_err(|e| IndexError::Query(format!("invalid stored path in search: {e}")))?;
            out.push(SearchHit {
                path,
                note_type,
                title,
                snippet,
                score,
            });
        }
        Ok(out)
    }

    fn fts_indexed_paths(&self) -> Result<Vec<VaultPath>, IndexError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare("SELECT path FROM notes_fts ORDER BY path")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let path_str = row?;
            out.push(VaultPath::new(path_str).map_err(|e| {
                IndexError::Query(format!("invalid stored path in fts_indexed_paths: {e}"))
            })?);
        }
        Ok(out)
    }

    fn replace_deadlines(
        &self,
        path: &VaultPath,
        deadlines: &[DeadlineEntry],
    ) -> Result<(), IndexError> {
        let mut conn = self.lock_conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM deadlines WHERE note_path = ?1",
            params![path.to_string()],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO deadlines (note_path, source, title, due_date, is_hard, context) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for d in deadlines {
                stmt.execute(params![
                    path.to_string(),
                    d.source,
                    d.title,
                    d.due_date,
                    d.is_hard as i64,
                    d.context,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn deadlines_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, DeadlineEntry)>, IndexError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT note_path, source, title, due_date, is_hard, context \
             FROM deadlines WHERE due_date BETWEEN ?1 AND ?2 ORDER BY due_date",
        )?;
        let rows = stmt.query_map(params![from, to], |r| {
            let path_str: String = r.get(0)?;
            let entry = DeadlineEntry {
                source: r.get(1)?,
                title: r.get(2)?,
                due_date: r.get(3)?,
                is_hard: r.get::<_, i64>(4)? != 0,
                context: r.get(5)?,
            };
            Ok((path_str, entry))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (path_str, entry) = row?;
            let path = VaultPath::new(path_str).map_err(|e| {
                IndexError::Query(format!("invalid stored VaultPath in deadlines: {e}"))
            })?;
            out.push((path, entry));
        }
        Ok(out)
    }

    fn replace_links(&self, path: &VaultPath, links: &[LinkEntry]) -> Result<(), IndexError> {
        let mut conn = self.lock_conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM links WHERE source_path = ?1",
            params![path.to_string()],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO links (source_path, target_raw, resolved_path, label, via_alias) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for link in links {
                stmt.execute(params![
                    path.to_string(),
                    link.target_raw,
                    link.resolved_path.as_ref().map(|p| p.to_string()),
                    link.label,
                    link.via_alias,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn find_backlinks(&self, path: &VaultPath) -> Result<Vec<VaultPath>, IndexError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT source_path FROM links WHERE resolved_path = ?1 ORDER BY source_path",
        )?;
        let rows = stmt.query_map(params![path.to_string()], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let path_str = row?;
            out.push(
                VaultPath::new(path_str)
                    .map_err(|e| IndexError::Query(format!("invalid stored backlink path: {e}")))?,
            );
        }
        Ok(out)
    }

    fn find_outgoing_links(&self, path: &VaultPath) -> Result<Vec<LinkEntry>, IndexError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT target_raw, resolved_path, label, via_alias FROM links \
             WHERE source_path = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![path.to_string()], |r| {
            let resolved: Option<String> = r.get(1)?;
            Ok((
                r.get::<_, String>(0)?,
                resolved,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (target_raw, resolved_str, label, via_alias) = row?;
            let resolved_path = match resolved_str {
                Some(s) => Some(VaultPath::new(s).map_err(|e| {
                    IndexError::Query(format!("invalid stored resolved_path: {e}"))
                })?),
                None => None,
            };
            out.push(LinkEntry {
                target_raw,
                resolved_path,
                label,
                via_alias,
            });
        }
        Ok(out)
    }

    fn link_resolutions(&self) -> Result<Vec<LinkResolution>, IndexError> {
        let conn = self.lock_conn()?;
        let mut stmt =
            conn.prepare("SELECT DISTINCT target_raw, resolved_path, via_alias FROM links")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, bool>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (target_raw, resolved_str, via_alias) = row?;
            let resolved_path = match resolved_str {
                Some(s) => Some(VaultPath::new(s).map_err(|e| {
                    IndexError::Query(format!("invalid stored resolved_path: {e}"))
                })?),
                None => None,
            };
            out.push(LinkResolution {
                target_raw,
                resolved_path,
                via_alias,
            });
        }
        Ok(out)
    }

    fn set_link_resolutions(&self, updates: &[LinkResolutionUpdate]) -> Result<usize, IndexError> {
        let mut conn = self.lock_conn()?;
        // One transaction for the batch: a commit per row would make a
        // write that heals many links pay a sync per link.
        let tx = conn.transaction()?;
        let mut changed = 0;
        {
            // `IS` rather than `=` so a `None` guard matches NULL rows.
            let mut stmt = tx.prepare(
                "UPDATE links SET resolved_path = ?3, via_alias = ?4 \
                 WHERE target_raw = ?1 AND resolved_path IS ?2",
            )?;
            for u in updates {
                changed += stmt.execute(params![
                    u.target_raw,
                    u.from.as_ref().map(|p| p.to_string()),
                    u.to.as_ref().map(|p| p.to_string()),
                    u.via_alias,
                ])?;
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    fn replace_tags(&self, path: &VaultPath, tags: &[String]) -> Result<(), IndexError> {
        let mut conn = self.lock_conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM note_tags WHERE note_path = ?1",
            params![path.to_string()],
        )?;
        {
            let mut stmt =
                tx.prepare("INSERT OR IGNORE INTO note_tags (note_path, tag) VALUES (?1, ?2)")?;
            for tag in tags {
                stmt.execute(params![path.to_string(), tag])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn find_by_tag(&self, tag: &str) -> Result<Vec<VaultPath>, IndexError> {
        let conn = self.lock_conn()?;
        let mut stmt =
            conn.prepare("SELECT note_path FROM note_tags WHERE tag = ?1 ORDER BY note_path")?;
        let rows = stmt.query_map(params![tag], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let path_str = row?;
            out.push(
                VaultPath::new(path_str)
                    .map_err(|e| IndexError::Query(format!("invalid tag path: {e}")))?,
            );
        }
        Ok(out)
    }

    fn replace_milestones(
        &self,
        path: &VaultPath,
        milestones: &[MilestoneEntry],
    ) -> Result<(), IndexError> {
        let mut conn = self.lock_conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM milestones WHERE note_path = ?1",
            params![path.to_string()],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO milestones (note_path, name, date, hard_soft, status) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for m in milestones {
                stmt.execute(params![
                    path.to_string(),
                    m.name,
                    m.date,
                    hard_soft_str(m.is_hard),
                    status_str(m.completed),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn milestones_for_project(&self, path: &VaultPath) -> Result<Vec<MilestoneEntry>, IndexError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT name, date, hard_soft, status FROM milestones \
             WHERE note_path = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![path.to_string()], row_to_milestone_entry)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn milestones_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, MilestoneEntry)>, IndexError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT note_path, name, date, hard_soft, status FROM milestones \
             WHERE date IS NOT NULL AND date BETWEEN ?1 AND ?2 ORDER BY date",
        )?;
        let rows = stmt.query_map(params![from, to], |r| {
            let path_str: String = r.get(0)?;
            Ok((path_str, row_to_milestone_entry_from(r, 1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (path_str, entry) = row?;
            let path = VaultPath::new(path_str).map_err(|e| {
                IndexError::Query(format!("invalid stored VaultPath in milestones: {e}"))
            })?;
            out.push((path, entry));
        }
        Ok(out)
    }

    fn record_archival_snapshot(
        &self,
        path: &VaultPath,
        snapshot: &ArchivalSnapshot,
    ) -> Result<(), IndexError> {
        let conn = self.lock_conn()?;
        conn.execute(
            "INSERT INTO archived_action_snapshots (note_path, frozen_size, frozen_hash, archived_at_ns, masked_hash, mask_version, masked_size) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(note_path) DO UPDATE SET \
                 frozen_size = excluded.frozen_size, \
                 frozen_hash = excluded.frozen_hash, \
                 archived_at_ns = excluded.archived_at_ns, \
                 masked_hash = excluded.masked_hash, \
                 mask_version = excluded.mask_version, \
                 masked_size = excluded.masked_size",
            params![
                path.to_string(),
                snapshot.frozen_size as i64,
                snapshot.frozen_hash,
                snapshot.archived_at_ns as i64,
                snapshot.masked_hash,
                snapshot.mask_version,
                snapshot.masked_size.map(|n| n as i64),
            ],
        )?;
        Ok(())
    }

    fn find_archival_snapshot(
        &self,
        path: &VaultPath,
    ) -> Result<Option<ArchivalSnapshot>, IndexError> {
        let conn = self.lock_conn()?;
        conn.query_row(
            "SELECT frozen_size, frozen_hash, archived_at_ns, masked_hash, mask_version, masked_size FROM archived_action_snapshots \
             WHERE note_path = ?1",
            params![path.to_string()],
            |r| {
                Ok(ArchivalSnapshot {
                    frozen_size: r.get::<_, i64>(0)? as u64,
                    frozen_hash: r.get(1)?,
                    archived_at_ns: r.get::<_, i64>(2)? as u64,
                    masked_hash: r.get(3)?,
                    mask_version: r.get(4)?,
                    masked_size: r.get::<_, Option<i64>>(5)?.map(|n| n as u64),
                })
            },
        )
        .optional()
        .map_err(IndexError::from)
    }

    fn list_archival_snapshots(&self) -> Result<Vec<(VaultPath, ArchivalSnapshot)>, IndexError> {
        snapshots_on(&*self.lock_conn()?)
    }
}

/// Every archival snapshot row `conn` reads, sorted by path:
/// [`VaultIndex::list_archival_snapshots`] on a bare connection, so a reopen
/// can salvage the old connection's rows before closing it.
fn snapshots_on(conn: &Connection) -> Result<Vec<(VaultPath, ArchivalSnapshot)>, IndexError> {
    {
        let mut stmt = conn.prepare(
            "SELECT note_path, frozen_size, frozen_hash, archived_at_ns, masked_hash, mask_version, masked_size \
             FROM archived_action_snapshots ORDER BY note_path",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                ArchivalSnapshot {
                    frozen_size: r.get::<_, i64>(1)? as u64,
                    frozen_hash: r.get(2)?,
                    archived_at_ns: r.get::<_, i64>(3)? as u64,
                    masked_hash: r.get(4)?,
                    mask_version: r.get(5)?,
                    masked_size: r.get::<_, Option<i64>>(6)?.map(|n| n as u64),
                },
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (path_str, snap) = row?;
            let path = VaultPath::new(path_str).map_err(|e| {
                IndexError::Query(format!(
                    "invalid stored VaultPath in archived_action_snapshots: {e}"
                ))
            })?;
            out.push((path, snap));
        }
        Ok(out)
    }
}

/// Encode the `is_hard` flag as the `hard_soft` column's text value.
fn hard_soft_str(is_hard: bool) -> &'static str {
    if is_hard { "hard" } else { "soft" }
}

/// Encode the `completed` flag as the `status` column's text value.
fn status_str(completed: bool) -> &'static str {
    if completed { "completed" } else { "pending" }
}

/// Build a [`MilestoneEntry`] from a row whose columns start at offset
/// `base`: `name, date, hard_soft, status`.
fn row_to_milestone_entry_from(
    row: &rusqlite::Row<'_>,
    base: usize,
) -> rusqlite::Result<MilestoneEntry> {
    let hard_soft: String = row.get(base + 2)?;
    let status: String = row.get(base + 3)?;
    Ok(MilestoneEntry {
        name: row.get(base)?,
        date: row.get(base + 1)?,
        is_hard: hard_soft == "hard",
        completed: status == "completed",
    })
}

/// Row extractor for a `name, date, hard_soft, status` column tuple
/// starting at column 0.
fn row_to_milestone_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<MilestoneEntry> {
    row_to_milestone_entry_from(row, 0)
}

/// The slug-shaped aliases a note's frontmatter declares (RFC 0007 §5.5):
/// the strings under its top-level `aliases` key, read exactly as
/// [`VaultIndex::list_aliases`] reads them, one entry per occurrence; empty
/// when the key is absent. Exposed so a caller holding a [`NoteEntry`] (the
/// transaction comparing a note's aliases before and after a commit) cannot
/// disagree with the index.
pub fn frontmatter_aliases(frontmatter: &JsonValue) -> Vec<String> {
    frontmatter
        .get("aliases")
        .map(alias_slugs)
        .unwrap_or_default()
}

/// The slug-shaped strings of an `aliases` value: a list of strings or one
/// scalar string; anything else is skipped. Shared by both indexes so they
/// cannot disagree.
fn alias_slugs(aliases: &JsonValue) -> Vec<String> {
    let items: &[JsonValue] = match aliases {
        JsonValue::Array(items) => items,
        single @ JsonValue::String(_) => std::slice::from_ref(single),
        _ => &[],
    };
    items
        .iter()
        .filter_map(JsonValue::as_str)
        .filter(|s| crate::slug::is_slug_shaped(s))
        .map(str::to_owned)
        .collect()
}

/// Shared row extractor for the full `notes` column set. Used by
/// `find_by_path` and `list_by_type`.
fn row_to_note_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<NoteEntry, IndexError>> {
    let path_str: String = row.get(0)?;
    let frontmatter_str: String = row.get(6)?;

    let path = match VaultPath::new(&path_str) {
        Ok(p) => p,
        Err(e) => return Ok(Err(IndexError::Query(format!("invalid stored path: {e}")))),
    };
    let frontmatter: JsonValue = match serde_json::from_str(&frontmatter_str) {
        Ok(v) => v,
        Err(e) => {
            return Ok(Err(IndexError::Query(format!(
                "invalid stored frontmatter JSON: {e}"
            ))));
        }
    };

    Ok(Ok(NoteEntry {
        path,
        note_type: row.get(1)?,
        title: row.get(2)?,
        content_hash: row.get(3)?,
        mtime_ns: row.get::<_, i64>(4)? as u64,
        size: row.get::<_, i64>(5)? as u64,
        frontmatter,
        indexed_at_ns: row.get::<_, i64>(7)? as u64,
    }))
}

/// Split a query into lowercased match terms for [`MemoryIndex::search`].
///
/// Accepts both a raw single word (`budget`) and the domain sanitiser's
/// output (`"budget" "meeting"`): each whitespace-separated token has its
/// surrounding double-quotes stripped and the FTS5 `""` escape collapsed,
/// then is lowercased. Empty tokens are dropped.
fn memory_query_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|tok| tok.trim_matches('"').replace("\"\"", "\"").to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Build a short excerpt of `body` around the first occurrence of
/// `needle` (already lowercased), bracketing the match — the
/// [`MemoryIndex`] stand-in for FTS5's `snippet()`. Falls back to a
/// prefix of the body when the match is title-only (not in the body).
fn memory_snippet(body: &str, needle: &str) -> String {
    const WINDOW: usize = 40;
    match body.to_lowercase().find(needle) {
        Some(pos) => {
            let start = body[..pos]
                .char_indices()
                .rev()
                .nth(WINDOW)
                .map_or(0, |(i, _)| i);
            let match_end = pos + needle.len();
            let end = body[match_end..]
                .char_indices()
                .nth(WINDOW)
                .map_or(body.len(), |(i, _)| match_end + i);
            format!(
                "{}[{}]{}",
                &body[start..pos],
                &body[pos..match_end],
                &body[match_end..end]
            )
        }
        None => body.chars().take(WINDOW * 2).collect(),
    }
}

// ---------------------------------------------------------------------
// MemoryIndex — test-only in-memory implementation of VaultIndex
// ---------------------------------------------------------------------

/// In-memory [`VaultIndex`] used for fast, deterministic domain tests.
///
/// Backed by `Mutex<MemoryIndexState>` (four `HashMap`s) so it satisfies
/// the trait's `Send + Sync` bound and lets tests share one index by
/// reference without platform-dependent IO. Production code always
/// uses [`SqliteIndex`]; having two impls proves the trait abstraction
/// is real — if domain code silently hard-coded `SqliteIndex`, this
/// suite would catch it at compile time.
///
/// Cross-reference queries (`find_backlinks`, `find_by_tag`,
/// `deadlines_between`) are O(n) linear scans. That's fine at test-fake
/// scale; never call it from production.
#[derive(Debug, Default)]
pub struct MemoryIndex {
    state: Mutex<MemoryIndexState>,
}

/// Internal state: one map per table family, keyed by the source
/// `VaultPath`. Cascading deletes are implemented by removing the path
/// from every map in `remove_note`.
#[derive(Debug, Default)]
struct MemoryIndexState {
    notes: HashMap<VaultPath, NoteEntry>,
    deadlines: HashMap<VaultPath, Vec<DeadlineEntry>>,
    links: HashMap<VaultPath, Vec<LinkEntry>>,
    tags: HashMap<VaultPath, Vec<String>>,
    milestones: HashMap<VaultPath, Vec<MilestoneEntry>>,
    archival_snapshots: HashMap<VaultPath, ArchivalSnapshot>,
    /// Searchable (title, body) text per note, mirroring `notes_fts`.
    fts: HashMap<VaultPath, (Option<String>, String)>,
}

impl MemoryIndex {
    /// See [`SqliteIndex::lock_conn`]: recover the guard instead of
    /// panicking on poison, so the test fake matches production
    /// semantics.
    fn lock_state(&self) -> std::sync::MutexGuard<'_, MemoryIndexState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn new() -> Self {
        Self::default()
    }
}

impl VaultIndex for MemoryIndex {
    fn upsert_note(&self, entry: &NoteEntry) -> Result<(), IndexError> {
        let mut state = self.lock_state();
        state.notes.insert(entry.path.clone(), entry.clone());
        Ok(())
    }

    fn remove_note(&self, path: &VaultPath) -> Result<(), IndexError> {
        // No FK engine in memory; mirror `ON DELETE CASCADE` manually
        // by dropping the path from every facet map.
        let mut state = self.lock_state();
        state.notes.remove(path);
        state.deadlines.remove(path);
        state.links.remove(path);
        state.tags.remove(path);
        state.milestones.remove(path);
        state.archival_snapshots.remove(path);
        state.fts.remove(path);
        Ok(())
    }

    fn find_by_path(&self, path: &VaultPath) -> Result<Option<NoteEntry>, IndexError> {
        let state = self.lock_state();
        Ok(state.notes.get(path).cloned())
    }

    fn list_by_type(&self, note_type: &str) -> Result<Vec<NoteEntry>, IndexError> {
        let state = self.lock_state();
        let mut out: Vec<NoteEntry> = state
            .notes
            .values()
            .filter(|n| n.note_type == note_type)
            .cloned()
            .collect();
        out.sort_by(|a, b| a.path.as_path().cmp(b.path.as_path()));
        Ok(out)
    }

    fn list_all_paths(&self) -> Result<Vec<VaultPath>, IndexError> {
        let state = self.lock_state();
        let mut out: Vec<VaultPath> = state.notes.keys().cloned().collect();
        out.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(out)
    }

    fn list_aliases(&self) -> Result<Vec<(String, VaultPath)>, IndexError> {
        let state = self.lock_state();
        // Ordered by the path's text, as SQLite orders `path`: `Path` compares
        // by component, which would put `x/foo/_index.md` before `x/foo.md`.
        let mut paths: Vec<&VaultPath> = state.notes.keys().collect();
        paths.sort_by_cached_key(|p| p.to_string());
        let mut out = Vec::new();
        for path in paths {
            for alias in frontmatter_aliases(&state.notes[path].frontmatter) {
                out.push((alias, path.clone()));
            }
        }
        Ok(out)
    }

    fn list_candidates(&self) -> Result<Vec<NoteCandidate>, IndexError> {
        let state = self.lock_state();
        // Mirrors the SQL's LEFT JOIN: a note with no `fts` entry still
        // appears, with `title: None`.
        let mut out: Vec<NoteCandidate> = state
            .notes
            .values()
            .map(|entry| NoteCandidate {
                path: entry.path.clone(),
                note_type: entry.note_type.clone(),
                title: state
                    .fts
                    .get(&entry.path)
                    .and_then(|(title, _body)| title.clone()),
                mtime_ns: entry.mtime_ns,
            })
            .collect();
        // Same key as the SQL: mtime descending, then path ascending.
        out.sort_by(|a, b| {
            b.mtime_ns
                .cmp(&a.mtime_ns)
                .then_with(|| a.path.as_path().cmp(b.path.as_path()))
        });
        Ok(out)
    }

    fn replace_fts(
        &self,
        path: &VaultPath,
        title: Option<&str>,
        body: &str,
    ) -> Result<(), IndexError> {
        let mut state = self.lock_state();
        state
            .fts
            .insert(path.clone(), (title.map(str::to_owned), body.to_owned()));
        Ok(())
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, IndexError> {
        // Deliberately simple: case-insensitive substring matching over the
        // stored title + body, ranked title-first. This double exists for
        // fast domain/MCP tests, not to reproduce FTS5 — no porter stemming
        // and no operator grammar (assert those against `SqliteIndex`). It
        // does, however, understand the shape the domain query sanitiser
        // emits: whitespace-separated, double-quoted terms ANDed together
        // (`"budget" "meeting"`). Terms are unquoted and ALL must appear —
        // a workable stand-in for FTS5's implicit AND of quoted terms,
        // which keeps both raw single-word queries and sanitised multi-term
        // queries matching here. See `memory_query_terms`.
        let terms = memory_query_terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let state = self.lock_state();
        let mut hits: Vec<SearchHit> = state
            .fts
            .iter()
            .filter_map(|(path, (title, body))| {
                let title_lc = title.as_deref().unwrap_or_default().to_lowercase();
                let body_lc = body.to_lowercase();
                // AND semantics: every term must appear somewhere.
                if !terms
                    .iter()
                    .all(|t| title_lc.contains(t) || body_lc.contains(t))
                {
                    return None;
                }
                let title_hit = terms.iter().any(|t| title_lc.contains(t));
                let note_type = state
                    .notes
                    .get(path)
                    .map(|n| n.note_type.clone())
                    .unwrap_or_default();
                // Snippet around the first term that lands in the body, or
                // the body prefix if the match is title-only.
                let snippet_needle = terms
                    .iter()
                    .find(|t| body_lc.contains(*t))
                    .map(String::as_str)
                    .unwrap_or(&terms[0]);
                Some(SearchHit {
                    path: path.clone(),
                    note_type,
                    title: title.clone(),
                    snippet: memory_snippet(body, snippet_needle),
                    // Lower is better; a title hit outranks a body-only
                    // hit, matching the weighted bm25 ordering.
                    score: if title_hit { 0.0 } else { 1.0 },
                })
            })
            .collect();
        // Stable order: score first, then path for determinism.
        hits.sort_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.as_path().cmp(b.path.as_path()))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    fn fts_indexed_paths(&self) -> Result<Vec<VaultPath>, IndexError> {
        let state = self.lock_state();
        let mut out: Vec<VaultPath> = state.fts.keys().cloned().collect();
        out.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(out)
    }

    fn replace_deadlines(
        &self,
        path: &VaultPath,
        deadlines: &[DeadlineEntry],
    ) -> Result<(), IndexError> {
        let mut state = self.lock_state();
        state.deadlines.insert(path.clone(), deadlines.to_vec());
        Ok(())
    }

    fn deadlines_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, DeadlineEntry)>, IndexError> {
        let state = self.lock_state();
        let mut out: Vec<(VaultPath, DeadlineEntry)> = state
            .deadlines
            .iter()
            .flat_map(|(path, entries)| {
                entries
                    .iter()
                    .filter(|d| d.due_date.as_str() >= from && d.due_date.as_str() <= to)
                    .map(move |d| (path.clone(), d.clone()))
            })
            .collect();
        // Match SqliteIndex's `ORDER BY due_date`.
        out.sort_by(|a, b| a.1.due_date.cmp(&b.1.due_date));
        Ok(out)
    }

    fn replace_links(&self, path: &VaultPath, links: &[LinkEntry]) -> Result<(), IndexError> {
        let mut state = self.lock_state();
        state.links.insert(path.clone(), links.to_vec());
        Ok(())
    }

    fn find_backlinks(&self, path: &VaultPath) -> Result<Vec<VaultPath>, IndexError> {
        let state = self.lock_state();
        // Distinct source paths where any link resolves to `path`.
        // A note with multiple links to the same target still appears
        // once, matching SqliteIndex's SELECT DISTINCT.
        let mut seen: Vec<VaultPath> = Vec::new();
        for (source_path, entries) in state.links.iter() {
            if entries
                .iter()
                .any(|l| l.resolved_path.as_ref() == Some(path))
                && !seen.contains(source_path)
            {
                seen.push(source_path.clone());
            }
        }
        seen.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(seen)
    }

    fn find_outgoing_links(&self, path: &VaultPath) -> Result<Vec<LinkEntry>, IndexError> {
        let state = self.lock_state();
        // Insertion order preserved, matching SqliteIndex's `ORDER BY id`
        // (rowids are monotonic with insertion).
        Ok(state.links.get(path).cloned().unwrap_or_default())
    }

    fn link_resolutions(&self) -> Result<Vec<LinkResolution>, IndexError> {
        let state = self.lock_state();
        let rows: HashSet<LinkResolution> = state
            .links
            .values()
            .flatten()
            .map(|l| LinkResolution {
                target_raw: l.target_raw.clone(),
                resolved_path: l.resolved_path.clone(),
                via_alias: l.via_alias,
            })
            .collect();
        Ok(rows.into_iter().collect())
    }

    fn set_link_resolutions(&self, updates: &[LinkResolutionUpdate]) -> Result<usize, IndexError> {
        // One lock hold for the batch, matching SqliteIndex's transaction.
        let mut state = self.lock_state();
        let mut changed = 0;
        for u in updates {
            for link in state.links.values_mut().flatten() {
                if link.target_raw == u.target_raw && link.resolved_path == u.from {
                    link.resolved_path = u.to.clone();
                    link.via_alias = u.via_alias;
                    changed += 1;
                }
            }
        }
        Ok(changed)
    }

    fn replace_tags(&self, path: &VaultPath, tags: &[String]) -> Result<(), IndexError> {
        let mut state = self.lock_state();
        // Dedupe in-place to match SqliteIndex's "INSERT OR IGNORE"
        // against the (note_path, tag) primary key. A duplicate tag
        // in the caller's input silently becomes a single entry.
        let mut deduped: Vec<String> = Vec::with_capacity(tags.len());
        for t in tags {
            if !deduped.contains(t) {
                deduped.push(t.clone());
            }
        }
        state.tags.insert(path.clone(), deduped);
        Ok(())
    }

    fn find_by_tag(&self, tag: &str) -> Result<Vec<VaultPath>, IndexError> {
        let state = self.lock_state();
        let mut out: Vec<VaultPath> = state
            .tags
            .iter()
            .filter_map(|(path, tags)| {
                if tags.iter().any(|t| t == tag) {
                    Some(path.clone())
                } else {
                    None
                }
            })
            .collect();
        out.sort_by(|a, b| a.as_path().cmp(b.as_path()));
        Ok(out)
    }

    fn replace_milestones(
        &self,
        path: &VaultPath,
        milestones: &[MilestoneEntry],
    ) -> Result<(), IndexError> {
        let mut state = self.lock_state();
        state.milestones.insert(path.clone(), milestones.to_vec());
        Ok(())
    }

    fn milestones_for_project(&self, path: &VaultPath) -> Result<Vec<MilestoneEntry>, IndexError> {
        let state = self.lock_state();
        // Stored Vec order is source order, mirroring SqliteIndex's
        // `ORDER BY id`. A path with no rows yields an empty list.
        Ok(state.milestones.get(path).cloned().unwrap_or_default())
    }

    fn milestones_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, MilestoneEntry)>, IndexError> {
        let state = self.lock_state();
        let mut out: Vec<(VaultPath, MilestoneEntry)> = state
            .milestones
            .iter()
            .flat_map(|(path, entries)| {
                entries
                    .iter()
                    .filter(|m| m.date.as_deref().is_some_and(|d| d >= from && d <= to))
                    .map(move |m| (path.clone(), m.clone()))
            })
            .collect();
        // Match SqliteIndex's `ORDER BY date`.
        out.sort_by_key(|(_, m)| m.date.clone());
        Ok(out)
    }

    fn record_archival_snapshot(
        &self,
        path: &VaultPath,
        snapshot: &ArchivalSnapshot,
    ) -> Result<(), IndexError> {
        let mut state = self.lock_state();
        state
            .archival_snapshots
            .insert(path.clone(), snapshot.clone());
        Ok(())
    }

    fn find_archival_snapshot(
        &self,
        path: &VaultPath,
    ) -> Result<Option<ArchivalSnapshot>, IndexError> {
        let state = self.lock_state();
        Ok(state.archival_snapshots.get(path).cloned())
    }

    fn list_archival_snapshots(&self) -> Result<Vec<(VaultPath, ArchivalSnapshot)>, IndexError> {
        let state = self.lock_state();
        let mut out: Vec<(VaultPath, ArchivalSnapshot)> = state
            .archival_snapshots
            .iter()
            .map(|(p, s)| (p.clone(), s.clone()))
            .collect();
        // Match SqliteIndex's `ORDER BY note_path`.
        out.sort_by_key(|(path, _)| path.to_string());
        Ok(out)
    }
}
