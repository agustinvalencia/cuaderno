//! Index health for a long-lived server: assess, repair, report (#526).
//!
//! The index is a cache of the notes, so it is the one thing in the vault
//! that may be repaired automatically: rebuilding it from the markdown
//! loses nothing. This module gives a running server that authority.
//! Before it, the periodic reconciliation could only log a count of the
//! notes it failed to index, and a damaged database stayed damaged until
//! somebody with a shell on the host deleted it.
//!
//! The order of responses to trouble, on every sweep and whenever a tool
//! call meets a corruption error:
//!
//! 1. **Are the files still ours?** ([`SqliteIndex::check_files`], #1000.)
//!    The server opens the index once, and its connection reads and writes
//!    through descriptors, not the path. When the database file is deleted,
//!    moved aside or replaced (`cdno reindex`'s set-aside, `reindex
//!    --rebind`, the open's self-heal, a restore), or its `-wal` and `-shm`
//!    are deleted under it (by a process on another kernel, whose locks this
//!    one cannot see, closing as "the last connection"), the connection goes
//!    on with files no other process sees, and every check through it
//!    reports that private view: damage on a sound file, or health on a file
//!    nobody else can read. So the identity check comes first, a few `stat`
//!    calls.
//! 2. **Reopen.** When the files are not ours, the connection is replaced
//!    with a fresh one at the same path ([`SqliteIndex::reopen`]), behind
//!    the same handle, the index reconciled, and the archival snapshots the
//!    old connection held carried over where the reopened file lacks them
//!    and the note still verifies (they are the one table the notes cannot
//!    rebuild; any other row the old connection had committed only to its
//!    orphaned files is dropped and written again from the notes). The old
//!    file's `-wal`/`-shm` left at the path by a restore of the main file
//!    alone, and a file at the path that will not open, are set aside under
//!    the vault write lock first. Then the checks below run on the file as
//!    it is. Each reopen is one WARN saying what was found and what was
//!    done. It fixes the view, not damage, so it never counts towards the
//!    damage recurrence below. It is always made, at most once per
//!    [`RepairPolicy::min_reopen_interval`]: a server never settles on a
//!    copy nobody else sees. Until it is made, nothing reconciles into or
//!    checks that copy. Reopens that keep recurring
//!    ([`RepairPolicy::reopen_limit`] in the window) are reported once, with
//!    advice that depends on what moved, and the report ends by itself when
//!    they stop. A reopen that fails is retried at the same spacing and said
//!    once per cause.
//! 3. **Assess.** Reconcile, then `PRAGMA quick_check` (the thorough
//!    `integrity_check` when the sweep could not index some notes, at most
//!    once an hour, and whenever damage only the thorough check sees is
//!    still outstanding). Page-level damage can leave every read working
//!    while writes to the damaged pages fail, so the check, not a query
//!    error, is the signal that can be relied on. A tool call that meets a
//!    corruption error wakes the loop at once ([`corruption_seen`]) for
//!    `integrity_check`.
//! 4. **Repair.** A failed check rebuilds the index from the notes and
//!    copies it into the live file in place
//!    ([`cdno_core::reconcile::rebuild`]), with no restart, so every process
//!    on this machine using the vault sees the repair. A file too damaged to
//!    be written over in place (its header) is set aside to `.previous`
//!    under the vault write lock and the index reopened on a fresh file,
//!    which the notes then fill: what a restart used to be needed for. Other
//!    servers on this machine holding the old file find it moved on their
//!    next sweep and reopen too. The set-aside keeps the first damaged copy
//!    of a streak (a later one is deleted), and is skipped when, under the
//!    lock, a fresh connection finds the file sound (another process
//!    repaired or replaced it). Rebuilds are serialised and spaced at least
//!    [`RepairPolicy::min_rebuild_interval`] apart.
//! - **Stop and say why.** This contains a fault on one machine: a torn
//!   write, a crash, a bad page. It is not protection for an index file
//!   shared between two machines or kernels (a container and its host, a
//!   network share), whose locks do not reach each other: there the damage
//!   comes back after every repair. So damage-triggered rebuilds are
//!   counted over a sliding window, and after
//!   [`RepairPolicy::recurrence_limit`] of them the server stops repairing
//!   by itself, logs one error naming the likely cause and the fix (give
//!   this machine its own index, `CUADERNO_INDEX_PATH` / `--index-path`),
//!   and reports the same through `index_status` and the hint every client
//!   error carries. Only repairs that copied in are counted; failed
//!   attempts keep the spacing and their own log line. The stop ends by
//!   itself when a check as strong as the one that found the damage
//!   passes; otherwise it stays until a `rebuild_index` succeeds (which
//!   allows one more automatic repair, not a fresh count) or a restart.
//! - **Report.** Failing notes are logged with their reasons, and the
//!   latest file check, reopen, sweep, check and rebuild are kept for
//!   `index_status`.
//!
//! What it never does: touch a note. Automatic repair stops at derived
//! state; the notes are the source of truth, and `lint` reports on them
//! without changing them.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cdno_core::classify::NoteScope;
use cdno_core::error::IndexError;
use cdno_core::index::{FileCheck, SqliteIndex, VaultIndex};
use cdno_core::reconcile::{ReconciliationReport, rebuild, reconcile};
use cdno_core::store::VaultStore;
use schemars::JsonSchema;
use serde::Serialize;
use tokio::sync::Notify;

/// The shortest time between two rebuilds, automatic or asked for, by
/// default. Also how long one `integrity_check` answers repeated
/// corruption signals and `index_status` calls.
pub const MIN_REBUILD_INTERVAL: Duration = Duration::from_secs(60);

/// How often a sweep that keeps failing on some notes repeats the
/// thorough check.
pub const INTEGRITY_RECHECK: Duration = Duration::from_secs(3600);

/// How many failing notes a sweep names, in the log and in the record.
/// The first few show what is wrong; all of them would bury it.
pub const FAILING_NOTES_SHOWN: usize = 5;

/// Seconds between two sweeps unless configured, on both servers.
pub const DEFAULT_SWEEP_INTERVAL_SECS: u64 = 300;

/// The variable both servers read the sweep interval from, in seconds; `0`
/// turns the sweeps off. `cdno-mcp-server` also takes it as
/// `--reconcile-interval-secs`; the stdio `cdno-mcp`, launched from a
/// client's config, reads only the variable (#1002).
pub const SWEEP_INTERVAL_ENV: &str = "CDNO_MCP_RECONCILE_INTERVAL_SECS";

/// The sweep interval in seconds from [`SWEEP_INTERVAL_ENV`], for the stdio
/// server: unset, empty or only spaces (as with `CUADERNO_INDEX_PATH`)
/// means [`DEFAULT_SWEEP_INTERVAL_SECS`]. Spaces around a number are
/// ignored, which the HTTP server's flag parser does not do. A value that
/// is not a whole number of seconds is an error naming the variable, so a
/// typo stops the server at start rather than silently running on the
/// default.
pub fn sweep_interval_secs_from_env() -> Result<u64, String> {
    parse_sweep_interval(std::env::var_os(SWEEP_INTERVAL_ENV).as_deref())
}

/// [`sweep_interval_secs_from_env`] on a given value, for tests.
pub fn parse_sweep_interval(value: Option<&std::ffi::OsStr>) -> Result<u64, String> {
    let blank = |v: &&std::ffi::OsStr| v.to_str().is_some_and(|s| s.trim().is_empty());
    let Some(value) = value.filter(|v| !blank(v)) else {
        return Ok(DEFAULT_SWEEP_INTERVAL_SECS);
    };
    value
        .to_str()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .ok_or_else(|| {
            format!(
                "{SWEEP_INTERVAL_ENV} must be a whole number of seconds (0 turns the sweeps off), \
                 got {value:?}"
            )
        })
}

/// How the server paces and bounds its own repairs.
#[derive(Debug, Clone, Copy)]
pub struct RepairPolicy {
    /// The shortest time between two rebuilds.
    pub min_rebuild_interval: Duration,
    /// The window over which damage-triggered rebuilds, and automatic
    /// reopens, are counted.
    pub recurrence_window: Duration,
    /// How many damage-triggered rebuilds within the window mean the damage
    /// keeps coming back: the next damage is reported, not repaired.
    pub recurrence_limit: usize,
    /// The shortest time between two automatic reopens (#1000). A reopen
    /// is always made when the files are not held, at most this often.
    pub min_reopen_interval: Duration,
    /// How many automatic reopens within the window are reported as
    /// recurring (`reopen_recurring`, one ERROR). It never stops a reopen.
    pub reopen_limit: usize,
}

impl Default for RepairPolicy {
    fn default() -> Self {
        Self {
            min_rebuild_interval: MIN_REBUILD_INTERVAL,
            recurrence_window: Duration::from_secs(3600),
            recurrence_limit: 3,
            min_reopen_interval: MIN_REBUILD_INTERVAL,
            reopen_limit: 3,
        }
    }
}

/// Woken when a tool call meets a corruption error. Process-wide because
/// the errors are converted in free functions with no server at hand
/// (`util::into_mcp_error`), and one process serves one vault. A wake
/// with no loop listening is kept as a permit, so the next loop to wait
/// still sees it.
static CORRUPTION_SEEN: Notify = Notify::const_new();

/// What the hint on a damaged-index error may promise, for the same
/// process-wide reason as [`CORRUPTION_SEEN`]: a [`RepairState`] as a byte.
static HINT_STATE: AtomicU8 = AtomicU8::new(0);

/// Tell the health loop that a tool call met a damaged index, or one that
/// is between connections, so it looks now rather than at the next sweep.
pub fn corruption_seen() {
    CORRUPTION_SEEN.notify_one();
}

/// The sentence appended to every client error caused by a damaged index,
/// true to what this server will do about it now.
pub fn damage_hint() -> &'static str {
    match HINT_STATE.load(Ordering::Acquire) {
        1 => DAMAGE_RECURRING_HINT,
        2 => REOPEN_RECURRING_HINT,
        3 => REPLACED_RECURRING_HINT,
        _ => REPAIRING_HINT,
    }
}

/// The sentence appended to an error from an index that is between
/// connections ([`IndexError::Detached`]).
pub const DETACHED_HINT: &str = "the notes on disk are intact; the index (a cache of them) is \
     being reopened, because its files were replaced or deleted under the server, which retries \
     by itself. `index_status` shows the state.";

const REPAIRING_HINT: &str = "the notes on disk are intact; the index (a cache of them) may be \
     damaged, and the server is checking it and rebuilds it if it is. `index_status` shows the \
     outcome; `rebuild_index` forces a rebuild.";

/// The advice for recurring damage, in the log, in `index_status` and in
/// the client hint.
pub const DAMAGE_RECURRING_ADVICE: &str = "the index keeps being damaged after each repair, so \
     something else is writing this index file, most likely another machine or a container \
     sharing the vault folder; give this machine its own index with `CUADERNO_INDEX_PATH` (or \
     `--index-path` on cdno-mcp-server)";

const DAMAGE_RECURRING_HINT: &str = "the notes on disk are intact; the index (a cache of them) \
     keeps being damaged after each repair, so the server has stopped repairing it by itself: \
     something else is writing the index file, most likely another machine or a container \
     sharing the vault folder. Tell the person to give this machine its own index \
     (`CUADERNO_INDEX_PATH`, or `--index-path` on cdno-mcp-server). `index_status` shows the \
     state; `rebuild_index` still forces one repair.";

/// The advice when the index's `-wal` and `-shm` keep being deleted under
/// the server: the signature of a process on another kernel sharing the
/// file.
pub const REOPEN_RECURRING_ADVICE: &str = "the index's -wal and -shm keep being deleted or \
     replaced under this server, the signature of another machine or a container sharing the \
     index file (each of its commands takes itself for the last user and deletes them); the \
     server reopens the index each time, but between reopens it reads and writes a copy nobody \
     else sees: give this machine its own index with `CUADERNO_INDEX_PATH` (or `--index-path` on \
     cdno-mcp-server)";

/// The advice when the index's database file keeps being replaced under the
/// server, nearly always by something on this machine.
pub const REPLACED_RECURRING_ADVICE: &str = "the index database file keeps being deleted or \
     replaced under this server, nearly always by something on this machine (`cdno reindex` \
     setting a damaged file aside, `cdno reindex --rebind`, restores, `git clean -x`, a sync tool \
     syncing the index); the server reopens it each time and nothing is lost but the archival \
     snapshots it could not carry; find what keeps replacing the file";

const REOPEN_RECURRING_HINT: &str = "the notes on disk are intact; the index's -wal and -shm (a \
     cache of the notes) keep being deleted under the server, the signature of another machine or \
     a container sharing the index file. The server reopens it each time. Tell the person to give \
     this machine its own index (`CUADERNO_INDEX_PATH`, or `--index-path` on cdno-mcp-server). \
     `index_status` shows the state.";

const REPLACED_RECURRING_HINT: &str = "the notes on disk are intact; the index file (a cache of \
     them) keeps being replaced under the server, which reopens it each time. Something on this \
     machine keeps replacing it (repeated `cdno reindex`, restores, a sync tool); `index_status` \
     shows what was found.";

/// The index's health supervisor for one server process.
pub struct IndexHealth {
    store: Arc<dyn VaultStore>,
    index: Arc<dyn VaultIndex>,
    sqlite: Arc<SqliteIndex>,
    scope: Arc<NoteScope>,
    policy: RepairPolicy,
    state: Mutex<State>,
    rebuilding: AtomicBool,
    /// Held while the files are checked and, if need be, reopened or set
    /// aside, so a sweep, a tool call's signal and a rebuild that find the
    /// same move act on it once.
    verifying: Mutex<()>,
}

/// What a reopen found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Moved {
    Database,
    Sidecars,
}

#[derive(Default)]
struct State {
    last_sweep: Option<SweepRecord>,
    last_check: Option<CheckRecord>,
    last_rebuild: Option<RebuildRecord>,
    last_rebuild_at: Option<Instant>,
    last_files: Option<FilesRecord>,
    last_reopen: Option<ReopenRecord>,
    damage_at_open: Option<Vec<String>>,
    /// The strongest check that has failed since the index was last known
    /// sound. Cleared by a passing check at least that strong, by a
    /// rebuild, or by a reopen (the check saw the old connection's view).
    /// `quick_check` cannot see what `integrity_check` found, so a later
    /// quick pass must not clear it.
    outstanding_damage: Option<CheckKind>,
    /// The latest `integrity_check`, kept so repeated asks within
    /// `min_rebuild_interval` reuse it rather than rescan the file.
    last_integrity: Option<(Instant, CheckRecord)>,
    /// When each damage-triggered rebuild in the recurrence window copied
    /// its fresh index in. Failed attempts are not counted: a peer holding
    /// the lock, a full disk or a read-only directory is not recurring
    /// damage, and the spacing already bounds them.
    damage_rebuilds: VecDeque<Instant>,
    /// When a `.previous` copy of a damaged file was last asked for, so a
    /// streak keeps its first one.
    copy_kept_at: Option<Instant>,
    /// Set when the damage kept coming back after repairs, with the
    /// strength of the check that had found it: automatic repair is off
    /// until a check that strong passes, a requested rebuild succeeds, or
    /// the server restarts.
    recurring: Option<CheckKind>,
    /// One automatic repair allowed past the recurrence limit, granted by a
    /// requested rebuild that succeeded while at the limit. A requested
    /// rebuild buys one round, not a fresh count, so answering every stop
    /// with `rebuild_index` cannot loop the repair on a shared index.
    grace: bool,
    /// When each automatic reopen in the recurrence window happened, and
    /// what it found. Failed reopens are not counted.
    reopens: VecDeque<(Instant, Moved)>,
    /// When the last automatic reopen was attempted, for the spacing.
    last_reopen_attempt: Option<Instant>,
    /// Whether the reopens are being reported as recurring: set, with one
    /// ERROR, when the count in the window reaches the limit, and cleared,
    /// with one INFO, when it drops below.
    reopen_recurring: Option<Moved>,
    /// The latest reopen failure said at ERROR, so one cause is said once.
    reopen_failure_said: Option<String>,
}

/// One reconciliation sweep, as `index_status` reports it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SweepRecord {
    /// When the sweep finished (local time, RFC 3339).
    pub at: String,
    pub scanned: usize,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    /// How many notes the sweep could not index.
    pub errors: usize,
    /// The first few of them, with the reason each failed.
    pub failing: Vec<FailingNote>,
    /// Set when the pass failed as a whole rather than note by note, or did
    /// not run (the server does not hold its index files and has not
    /// reopened them yet).
    pub failure: Option<String>,
}

/// A note a sweep could not index.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FailingNote {
    pub path: String,
    pub reason: String,
}

/// One integrity check of the index's database file.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CheckRecord {
    /// When the check ran. A recent thorough check may be reported again
    /// rather than rerun, so this can be earlier than the call.
    pub at: String,
    /// `quick_check` (every sweep) or `integrity_check` (thorough: also
    /// compares each index with its table).
    pub kind: CheckKind,
    /// True when the check found nothing wrong.
    pub ok: bool,
    /// What it found, at most [`CHECK_PROBLEM_LIMIT`](cdno_core::index::CHECK_PROBLEM_LIMIT)
    /// lines; empty when `ok`. When the server does not hold its files the
    /// check is not run, and its one line says so.
    pub problems: Vec<String>,
}

/// Ordered by strength: `IntegrityCheck` sees everything `QuickCheck` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    QuickCheck,
    IntegrityCheck,
}

/// One look at whether the server's connection still holds the files at
/// the index's path (#1000).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FilesRecord {
    /// When it looked.
    pub at: String,
    /// False when the database file, its `-wal` or its `-shm` are known not
    /// to be the ones the connection has open: the server then reads and
    /// writes a copy no other process sees until it reopens. True when they
    /// are, or when they could not be checked (`kind: unchecked`).
    pub held: bool,
    /// `held`, `database_moved` (the database file deleted, moved aside or
    /// replaced; nearly always something on this machine), `sidecars_moved`
    /// (its `-wal`/`-shm` deleted or replaced; the signature of another
    /// kernel sharing the file, or of `rm`, `git clean -x`, a sync tool),
    /// `detached` (a reopen could not open the file yet) or `unchecked`
    /// (a `stat` failed; nothing concluded).
    pub kind: String,
    /// What was found, when it is not `held`.
    pub found: Option<String>,
}

/// One reopen of the index on the files at its path.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ReopenRecord {
    pub at: String,
    /// What asked: the sweep, a tool call that met a damaged index, or a
    /// `rebuild_index` call.
    pub trigger: String,
    /// What the file check found that called for it (`FilesRecord.kind`).
    pub kind: String,
    /// The finding, in a sentence.
    pub found: String,
    /// True when the index now holds the files at its path.
    pub reopened: bool,
    /// What was done, in a sentence.
    pub done: String,
    /// True when the old file's `-wal`/`-shm`, left at the path by a
    /// restore that replaced only the main file, were moved aside (to
    /// `<index>.stale-wal` / `.stale-shm`) before the new file was opened.
    pub stale_sidecars_set_aside: bool,
    /// True when this server found the file at the path unopenable and moved
    /// it aside under the vault write lock (to `previous`, or deleted it when
    /// the first damaged copy of a streak is already kept), then reopened on
    /// a fresh file. False when another process had already replaced it.
    pub set_aside: bool,
    /// Where the set-aside file went, when it was kept.
    pub previous: Option<String>,
    /// Archival snapshots the old connection held that the reopened file
    /// lacked, carried into it: each one's note is indexed and still
    /// verifies against its snapshot.
    pub snapshots_carried: usize,
    /// Snapshots the old connection held that were not carried, with why
    /// (the note not indexed, no longer verifying, or the rows unreadable).
    /// Each costs `lint`'s frozen-prefix check on that note.
    pub snapshots_lost: Vec<String>,
    /// Why the reopen failed, when it did (a file bound to another vault,
    /// one that is not cuaderno's index, an I/O error). It is retried at the
    /// reopen spacing.
    pub failure: Option<String>,
}

/// What the server does about damage now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RepairState {
    /// It checks the index and repairs damage by itself.
    Automatic,
    /// The damage kept coming back after repairs, so it has stopped
    /// repairing by itself; `advice` says why and what to do.
    DamageRecurring,
    /// The index's files keep being deleted or replaced under the server.
    /// A report, not a stop: it reopens them every time, at most once a
    /// minute, and the state ends by itself once the reopens stop
    /// recurring over the window. `advice` says what it most likely is.
    ReopenRecurring,
}

/// One rebuild of the index.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RebuildRecord {
    pub at: String,
    /// Why it ran: a failed sweep check, a corruption a tool call met, or
    /// a `rebuild_index` call.
    pub trigger: String,
    /// True when the index was rebuilt: a fresh index copied in, or (with
    /// `set_aside`) the damaged file replaced.
    pub rebuilt: bool,
    /// Notes the rebuilt index holds.
    pub notes: usize,
    /// Notes the rebuild could not index (see the next sweep for which).
    pub errors: usize,
    /// Archival snapshots carried over from the old index (the record
    /// `lint` checks archived action notes against; not in the notes).
    pub snapshots_carried: usize,
    /// Snapshot rows the old index's damage kept from being carried, or
    /// whose note is no longer indexed. Each costs only `lint`'s
    /// frozen-prefix check on that note.
    pub snapshots_lost: Vec<String>,
    /// Why the rebuild failed, when it did. The old index is then still
    /// the live one.
    pub failure: Option<String>,
    /// True when this server found the live file too damaged to be written
    /// over in place (its header, typically), moved it aside under the
    /// vault write lock (to `.previous`, unless the first damaged copy of a
    /// streak is already kept, when it is deleted) and reopened on a fresh
    /// file, filled from the notes. No restart is needed; other servers on
    /// this machine reopen on their next sweep. Its archival snapshots are
    /// lost. False when another process had replaced the file meanwhile.
    pub set_aside: bool,
    /// The reopen this call made first, when the server's files had moved.
    pub reopen: Option<ReopenRecord>,
}

/// What `index_status` returns.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct IndexStatus {
    /// The verdict on the index itself: true when the server holds the
    /// files at the index's path (`files.held`), the check reported for
    /// this call found nothing, no stronger check has found damage since
    /// the index was last known sound, and neither damage nor reopens are
    /// recurring. Notes that could not be indexed do not make it false:
    /// they are nearly always notes that do not parse, which no rebuild can
    /// fix.
    pub healthy: bool,
    /// What the server does about damage now: `automatic`,
    /// `damage_recurring` or `reopen_recurring`.
    pub repair: RepairState,
    /// What to do, when `repair` is not `automatic`.
    pub advice: Option<String>,
    /// True while automatic repair is stopped by recurring damage (`repair`
    /// is then `damage_recurring`, whatever the reopens do).
    pub damage_repair_stopped: bool,
    /// Damage-triggered rebuilds within the last hour (the recurrence
    /// window).
    pub damage_rebuilds_recent: usize,
    /// Automatic reopens within the last hour (the recurrence window).
    pub reopens_recent: usize,
    /// The index database file this server uses, after the per-machine
    /// override was resolved (#998). Every file a rebuild writes (the
    /// fresh `.rebuild-*` index while it runs, the `.previous` copy of a
    /// damaged file) sits beside it.
    pub index_path: String,
    /// A file check run for this call: whether the server's connection
    /// still holds the files at `index_path`.
    pub files: FilesRecord,
    /// The check reported for this call: a fresh `quick_check`, or, while
    /// damage only `integrity_check` sees is outstanding, a thorough check
    /// from the last minute or run for this call. Not run while `files.held`
    /// is false: it would describe the server's own copy, not the file.
    pub check: CheckRecord,
    /// How many notes the latest sweep (or rebuild) could not index, listed
    /// in `last_sweep.failing`. Usually malformed notes: `lint` names them,
    /// and fixing the note fixes it.
    pub notes_not_indexed: usize,
    pub last_sweep: Option<SweepRecord>,
    pub last_rebuild: Option<RebuildRecord>,
    /// The latest reopen: when, why, what was found and what was done.
    pub last_reopen: Option<ReopenRecord>,
    /// What the server found wrong with the index when it started, before
    /// it rebuilt it; null when nothing was.
    pub damage_at_open: Option<Vec<String>>,
}

/// Why a rebuild did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildRefused {
    /// Another rebuild is running now.
    InProgress,
    /// The last one finished too recently; try again after this long.
    TooSoon(Duration),
    /// An automatic rebuild only: the damage keeps coming back, so the
    /// server reports it instead.
    DamageRecurring,
    /// The server's connection does not hold the files at the index's path
    /// and reopening them failed, so a rebuild would land in a copy no other
    /// process sees. Carries why the reopen failed.
    FilesNotHeld(String),
}

impl RebuildRefused {
    /// A machine-readable name for the reason: `in_progress`, `too_soon`,
    /// `damage_recurring` or `files_not_held`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::TooSoon(_) => "too_soon",
            Self::DamageRecurring => "damage_recurring",
            Self::FilesNotHeld(_) => "files_not_held",
        }
    }
}

impl std::fmt::Display for RebuildRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InProgress => write!(f, "a rebuild is already running"),
            Self::TooSoon(wait) => write!(
                f,
                "the index was rebuilt moments ago; try again in {}s",
                wait.as_secs().max(1)
            ),
            Self::DamageRecurring => write!(f, "{DAMAGE_RECURRING_ADVICE}"),
            Self::FilesNotHeld(why) => write!(
                f,
                "the index file at its path is not the one this server has open, and reopening \
                 it failed ({why}), so a rebuild would not reach it"
            ),
        }
    }
}

/// A requested rebuild that did not run, with the reopen the call made
/// before it was refused, if any: the reopened file is reconciled either
/// way.
#[derive(Debug, Clone)]
pub struct RebuildRefusal {
    pub refused: RebuildRefused,
    pub reopen: Option<Box<ReopenRecord>>,
}

impl std::fmt::Display for RebuildRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.refused)?;
        if let Some(reopen) = self.reopen.as_ref().filter(|r| r.reopened) {
            write!(
                f,
                " (the index files had changed under this server, so it was reopened on the \
                 file at its path and reconciled first: {})",
                reopen.found
            )?;
        }
        Ok(())
    }
}

/// Who asked for a rebuild or a reopen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    /// The server itself: a sweep, a failed check, or a tool call's
    /// corruption. Counted, spaced, and stopped (rebuilds only) when it
    /// recurs.
    Damage,
    /// `rebuild_index`: always runs, subject only to the rebuild spacing.
    Requested,
}

/// What looking at the files found, before anything acts on the
/// connection's view of them.
#[derive(Debug, Clone)]
enum Files {
    /// The connection holds the files at the path (or they could not be
    /// checked this time).
    Held,
    /// It did not; the index was reopened on the files at the path and
    /// reconciled, which this sweep record says.
    Reopened(SweepRecord),
    /// It does not, and was not reopened (the spacing, or a failed reopen):
    /// why.
    NotHeld(String),
}

/// What a set-aside under the vault lock did.
struct SetAside {
    /// True when this server moved or deleted the file; false when another
    /// process had replaced it with a sound one meanwhile.
    done: bool,
    previous: Option<std::path::PathBuf>,
    reopened: cdno_core::index::Reopened,
}

impl IndexHealth {
    pub fn new(
        store: Arc<dyn VaultStore>,
        index: Arc<dyn VaultIndex>,
        sqlite: Arc<SqliteIndex>,
        scope: Arc<NoteScope>,
    ) -> Self {
        Self {
            store,
            index,
            sqlite,
            scope,
            policy: RepairPolicy::default(),
            state: Mutex::new(State::default()),
            rebuilding: AtomicBool::new(false),
            verifying: Mutex::new(()),
        }
    }

    /// Build from what [`open_vault_checked`](cdno_domain::bootstrap::open_vault_checked)
    /// returned, keeping what it found at open. Damage repaired at open
    /// counts as the first repair of a streak (damage found but not
    /// repaired does not), and its `.previous` copy is the one kept.
    pub fn from_opened(opened: &cdno_domain::bootstrap::OpenedVault) -> Self {
        let health = Self::new(
            opened.store.clone(),
            opened.index.clone(),
            opened.sqlite.clone(),
            opened.scope.clone(),
        );
        {
            let mut state = health.lock();
            state.damage_at_open = opened.damage_at_open.clone();
            if opened.damage_at_open.is_some() {
                state.copy_kept_at = Some(Instant::now());
            }
            if opened.repaired_at_open {
                state.damage_rebuilds.push_back(Instant::now());
            }
        }
        health
    }

    /// The same, with its own pacing and bounds (tests use a zero spacing
    /// to reach the recurrence limit without waiting).
    pub fn with_policy(mut self, policy: RepairPolicy) -> Self {
        self.policy = policy;
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_verifying(&self) -> std::sync::MutexGuard<'_, ()> {
        self.verifying
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Look at whether the connection still holds the files at the index's
    /// path ([`SqliteIndex::check_files`]), and record it.
    pub fn check_files(&self) -> FilesRecord {
        self.record_files(&self.sqlite.check_files())
    }

    fn record_files(&self, found: &FileCheck) -> FilesRecord {
        let record = FilesRecord {
            at: now(),
            held: !found.needs_reopen(),
            kind: found.kind().to_owned(),
            found: found.finding().map(str::to_owned),
        };
        self.lock().last_files = Some(record.clone());
        record
    }

    /// Whether a first damaged copy is still to be kept in this window, and
    /// if so, mark it as kept now.
    fn take_first_copy(&self) -> bool {
        let mut state = self.lock();
        let window = self.policy.recurrence_window;
        let first = state.copy_kept_at.is_none_or(|at| at.elapsed() >= window);
        if first {
            state.copy_kept_at = Some(Instant::now());
        }
        first
    }

    /// The first response to trouble (#1000): check that the connection
    /// still holds the files at the path, and when it does not, reopen it on
    /// them (automatic reopens at most once per
    /// [`RepairPolicy::min_reopen_interval`]), reconcile the reopened index
    /// and carry the archival snapshots the old connection held. A file at
    /// the path that will not open is set aside under the vault write lock
    /// first; the old file's sidecars left at the path by a restore of the
    /// main file alone are moved aside under it too. One WARN per reopen
    /// says what was found and what was done; a failed reopen is an ERROR
    /// once per distinct cause.
    fn verify_files(&self, trigger: &str, who: Trigger) -> Files {
        let _one_at_a_time = self.lock_verifying();
        let found = self.sqlite.check_files();
        self.record_files(&found);
        if !found.needs_reopen() {
            if let FileCheck::Unchecked(why) = &found {
                tracing::debug!(why = %why, "the index files could not be checked this time");
            }
            return Files::Held;
        }
        let finding = found.finding().unwrap_or_default().to_owned();
        let moved = match found {
            FileCheck::SidecarsMoved(_) => Moved::Sidecars,
            _ => Moved::Database,
        };
        {
            let mut state = self.lock();
            if who == Trigger::Damage
                && let Some(at) = state.last_reopen_attempt
                && at.elapsed() < self.policy.min_reopen_interval
            {
                tracing::debug!(found = %finding, "the index files are not held; the next reopen is spaced");
                return Files::NotHeld(format!(
                    "{finding}; the server reopens at most every {}s, and will again",
                    self.policy.min_reopen_interval.as_secs().max(1)
                ));
            }
            state.last_reopen_attempt = Some(Instant::now());
        }

        let outcome = self.reopen_now();
        let mut record = ReopenRecord {
            at: now(),
            trigger: trigger.to_owned(),
            kind: found.kind().to_owned(),
            found: finding.clone(),
            reopened: false,
            done: String::new(),
            stale_sidecars_set_aside: false,
            set_aside: false,
            previous: None,
            snapshots_carried: 0,
            snapshots_lost: Vec::new(),
            failure: None,
        };
        let aside = match outcome {
            Ok(aside) => aside,
            Err(failure) => {
                record.done = "kept the old connection; the reopen is retried".to_owned();
                record.failure = Some(failure.clone());
                let mut state = self.lock();
                if state.reopen_failure_said.as_deref() != Some(failure.as_str()) {
                    tracing::error!(
                        found = %finding,
                        failure = %failure,
                        index_path = %self.sqlite.path().display(),
                        "the index files changed under this server, and reopening them failed; \
                         it keeps its old connection, retries at most every {}s, and says so \
                         again only if the cause changes",
                        self.policy.min_reopen_interval.as_secs().max(1)
                    );
                    state.reopen_failure_said = Some(failure.clone());
                } else {
                    tracing::debug!(failure = %failure, "reopening the index failed again");
                }
                state.last_reopen = Some(record);
                return Files::NotHeld(format!("{finding}; reopening failed: {failure}"));
            }
        };

        record.reopened = true;
        record.stale_sidecars_set_aside = !aside.reopened.stale_sidecars_moved_to.is_empty();
        record.set_aside = aside.done;
        record.previous = aside.previous.as_ref().map(|p| p.display().to_string());
        // What the checks found was the old connection's view.
        {
            let mut state = self.lock();
            state.outstanding_damage = None;
            state.last_integrity = None;
            state.reopen_failure_said = None;
        }
        let sweep = self.reconcile_and_record();
        let (carried, lost) = self.carry_snapshots(aside.reopened.salvaged);
        record.snapshots_carried = carried;
        record.snapshots_lost = lost;
        record.done = describe_reopen(&record);
        self.record_files(&self.sqlite.check_files());
        tracing::warn!(
            found = %finding,
            kind = %record.kind,
            done = %record.done,
            trigger,
            snapshots_carried = record.snapshots_carried,
            snapshots_lost = record.snapshots_lost.len(),
            index_path = %self.sqlite.path().display(),
            "the index files changed under this server; reopened the index on the file at its path"
        );
        let mut state = self.lock();
        if who == Trigger::Damage {
            state.reopens.push_back((Instant::now(), moved));
        }
        state.last_reopen = Some(record);
        self.refresh_reopen_recurring(&mut state);
        Files::Reopened(sweep)
    }

    /// Reopen on the file at the path, handling what the plain reopen
    /// refuses: the old file's sidecars at the path (moved aside under the
    /// vault lock) and a file that will not open (set aside under it).
    fn reopen_now(&self) -> Result<SetAside, String> {
        let plain = |reopened| SetAside {
            done: false,
            previous: None,
            reopened,
        };
        match self.sqlite.reopen() {
            Ok(reopened) => Ok(plain(reopened)),
            Err(IndexError::StaleSidecars(_)) => {
                let _lock = self.store.acquire_write_lock().map_err(|e| {
                    format!("acquiring the vault write lock to set stale sidecars aside: {e}")
                })?;
                match self.sqlite.reopen_setting_aside_stale_sidecars() {
                    Ok(reopened) => Ok(plain(reopened)),
                    Err(e) if e.is_corruption() => {
                        drop(_lock);
                        self.set_aside_unopenable(self.take_first_copy())
                            .map_err(|aside| {
                                format!("the file at the path will not open ({e}), and setting it aside failed: {aside}")
                            })
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            // The file at the path will not open: set it aside, as a
            // restart would, and start from a fresh one.
            Err(e) if e.is_corruption() => self
                .set_aside_unopenable(self.take_first_copy())
                .map_err(|aside| {
                    format!("the file at the path will not open ({e}), and setting it aside failed: {aside}")
                }),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Replace a file too damaged to open or to write over: under the vault
    /// write lock, unless a fresh connection now opens the path and checks
    /// it sound (another process repaired or replaced it meanwhile, as
    /// `cdno reindex`'s set-aside also asks), run the open's guards for an
    /// overridden index, then move it aside to `.previous` when
    /// `keep_copy` (the first damaged copy of a streak), or delete it when a
    /// first copy is already kept, and reopen the index on a fresh file at
    /// the path for the caller's reconciliation to fill. Other processes on
    /// this machine holding the old file find it moved: a server reopens on
    /// its next sweep, and a CLI command is short-lived. The caller holds
    /// `verifying`.
    fn set_aside_unopenable(&self, keep_copy: bool) -> Result<SetAside, IndexError> {
        let location = self.sqlite.location();
        let _lock = self.store.acquire_write_lock().map_err(|e| {
            IndexError::Update(format!(
                "acquiring the vault write lock to set the damaged index aside: {e}"
            ))
        })?;
        let root = std::path::PathBuf::from(self.sqlite.binding().unwrap_or_default());
        let repaired = SqliteIndex::open_location_unhealed(&location, &root)
            .is_ok_and(|probe| probe.quick_check().is_ok_and(|p| p.is_empty()));
        let mut previous = None;
        if !repaired {
            if let Some(root) = self.sqlite.binding() {
                SqliteIndex::ensure_discardable(&location.path)?;
                if let Some(bound) = SqliteIndex::bound_vault_root(&location.path)?
                    && bound != root
                {
                    return Err(IndexError::BoundToOtherVault {
                        index: location.path.display().to_string(),
                        bound,
                        root: root.to_owned(),
                    });
                }
            }
            if keep_copy {
                SqliteIndex::set_aside(&location)?;
                previous = Some(cdno_core::index::previous_copy_path(&location.path));
            } else {
                SqliteIndex::remove(&location.path);
            }
        }
        let reopened = match self.sqlite.reopen() {
            Err(IndexError::StaleSidecars(_)) => self.sqlite.reopen_setting_aside_stale_sidecars(),
            other => other,
        }?;
        Ok(SetAside {
            done: !repaired,
            previous,
            reopened,
        })
    }

    /// Carry the archival snapshots the old connection held into the
    /// reopened index (#1000), under the vault write lock: a row only when
    /// the reopened index has none for that path (its own rows are never
    /// overwritten), its note is indexed there, and the note on disk still
    /// verifies against it by lint's rule. Returns how many were carried and
    /// why each other one was not.
    fn carry_snapshots(
        &self,
        salvaged: Option<
            Result<
                Vec<(
                    cdno_core::path::VaultPath,
                    cdno_core::index::ArchivalSnapshot,
                )>,
                String,
            >,
        >,
    ) -> (usize, Vec<String>) {
        let rows = match salvaged {
            None => return (0, Vec::new()),
            Some(Err(e)) => {
                return (
                    0,
                    vec![format!(
                        "the old connection's snapshots could not be read: {e}"
                    )],
                );
            }
            Some(Ok(rows)) if rows.is_empty() => return (0, Vec::new()),
            Some(Ok(rows)) => rows,
        };
        let _lock = match self.store.acquire_write_lock() {
            Ok(lock) => lock,
            Err(e) => {
                return (
                    0,
                    rows.iter()
                        .map(|(p, _)| format!("{p}: the vault write lock was not available: {e}"))
                        .collect(),
                );
            }
        };
        let mut carried = 0;
        let mut lost = Vec::new();
        for (path, snapshot) in rows {
            match self.index.find_archival_snapshot(&path) {
                Ok(Some(_)) => continue,
                Ok(None) => {}
                Err(e) => {
                    lost.push(format!("{path}: {e}"));
                    continue;
                }
            }
            if !matches!(self.index.find_by_path(&path), Ok(Some(_))) {
                lost.push(format!(
                    "{path}: its note is not indexed in the reopened file"
                ));
                continue;
            }
            match self.store.read_file(&path) {
                Ok(content)
                    if cdno_domain::vault::archival_snapshot_verifies(&content, &snapshot) => {}
                Ok(_) => {
                    lost.push(format!("{path}: the note no longer verifies against it"));
                    continue;
                }
                Err(e) => {
                    lost.push(format!("{path}: {e}"));
                    continue;
                }
            }
            match self.index.record_archival_snapshot(&path, &snapshot) {
                Ok(()) => carried += 1,
                Err(e) => lost.push(format!("{path}: {e}")),
            }
        }
        (carried, lost)
    }

    /// Bring `reopen_recurring` up to date with the reopens in the window:
    /// one ERROR with the advice when it begins, one INFO when it ends.
    fn refresh_reopen_recurring(&self, state: &mut State) {
        let window = self.policy.recurrence_window;
        while state
            .reopens
            .front()
            .is_some_and(|(at, _)| at.elapsed() >= window)
        {
            state.reopens.pop_front();
        }
        let recurring = (state.reopens.len() >= self.policy.reopen_limit).then(|| {
            if state.reopens.iter().any(|(_, m)| *m == Moved::Sidecars) {
                Moved::Sidecars
            } else {
                Moved::Database
            }
        });
        match (state.reopen_recurring, recurring) {
            (None, Some(moved)) => tracing::error!(
                reopens = state.reopens.len(),
                window_secs = window.as_secs(),
                index_path = %self.sqlite.path().display(),
                "the index files keep changing under this server (it reopens them each time): {}",
                reopen_advice(moved)
            ),
            (Some(_), None) => {
                tracing::info!("the index files have stopped changing under this server")
            }
            _ => {}
        }
        state.reopen_recurring = recurring;
        publish_hint(state);
    }

    /// Run the integrity check of `kind` and record it. A check that
    /// cannot run at all is recorded as a failed one, naming the error.
    pub fn check(&self, kind: CheckKind) -> CheckRecord {
        let result = match kind {
            CheckKind::QuickCheck => self.sqlite.quick_check(),
            CheckKind::IntegrityCheck => self.sqlite.integrity_check(),
        };
        let problems = result.unwrap_or_else(|e| vec![format!("the check could not run: {e}")]);
        let record = CheckRecord {
            at: now(),
            kind,
            ok: problems.is_empty(),
            problems,
        };
        let mut state = self.lock();
        state.last_check = Some(record.clone());
        if kind == CheckKind::IntegrityCheck {
            state.last_integrity = Some((Instant::now(), record.clone()));
        }
        if !record.ok {
            state.outstanding_damage = state.outstanding_damage.max(Some(kind));
        } else {
            if state.outstanding_damage.is_none_or(|d| kind >= d) {
                state.outstanding_damage = None;
            }
            // A stop ends by itself when the file is found sound again by a
            // check as strong as the one that found the damage: another
            // process repaired it, or what was writing it has stopped.
            if state.recurring.is_some_and(|found| kind >= found) {
                state.recurring = None;
                tracing::info!(
                    check = ?kind,
                    "the index checks sound again; automatic index repair resumes"
                );
                publish_hint(&state);
            }
        }
        record
    }

    /// Whether automatic repair is stopped by recurring damage.
    fn repair_stopped(&self) -> bool {
        self.lock().recurring.is_some()
    }

    /// The latest `integrity_check`, when it ran less than
    /// `min_rebuild_interval` ago (at least a minute).
    fn recent_integrity(&self) -> Option<CheckRecord> {
        let reuse = self.policy.min_rebuild_interval.max(MIN_REBUILD_INTERVAL);
        self.lock()
            .last_integrity
            .as_ref()
            .filter(|(at, _)| at.elapsed() < reuse)
            .map(|(_, record)| record.clone())
    }

    /// Reconcile the index with the notes, log what failed, and record it
    /// as the latest sweep.
    fn reconcile_and_record(&self) -> SweepRecord {
        let record = match reconcile(&self.store, &self.index, &self.scope) {
            Ok(report) => {
                log_sweep(&report);
                sweep_record(&report, None)
            }
            Err(e) => {
                tracing::warn!(error = %e, "periodic reconciliation failed");
                sweep_record(&ReconciliationReport::default(), Some(e.to_string()))
            }
        };
        self.lock().last_sweep = Some(record.clone());
        record
    }

    /// One sweep: check the files are still the server's and reopen them if
    /// not, reconcile, log what failed, check, and rebuild if the check finds
    /// damage. When the files are not held and were not reopened (the reopen
    /// spacing, or a failed reopen), nothing else runs: a reconcile would
    /// write into, and a check would describe, a copy no other process sees.
    /// Blocking; the loop runs it on the blocking pool.
    pub fn sweep(&self) {
        let record = match self.verify_files("the sweep", Trigger::Damage) {
            Files::NotHeld(why) => {
                let mut skipped = sweep_record(&ReconciliationReport::default(), None);
                skipped.failure = Some(format!(
                    "skipped: this server does not hold the index files at its path ({why})"
                ));
                let mut state = self.lock();
                state.last_sweep = Some(skipped);
                self.refresh_reopen_recurring(&mut state);
                return;
            }
            Files::Reopened(record) => record,
            Files::Held => self.reconcile_and_record(),
        };
        self.refresh_reopen_recurring(&mut self.lock());
        let had_errors = record.errors > 0 || record.failure.is_some();

        // A sweep that could not index some notes gets the thorough check:
        // damage confined to an index b-tree (rows missing from
        // `idx_links_resolved`, as in #526) fails only writes, and
        // `quick_check` skips exactly that comparison.
        // At most hourly: a vault with one note that never parses would
        // otherwise pay a full scan on every sweep. Damage a thorough
        // check found and no repair has cleared yet (a failed rebuild)
        // keeps the thorough check on every sweep, so the repair is
        // retried rather than waiting out the hour behind a passing quick
        // check.
        let (integrity_due, outstanding) = {
            let state = self.lock();
            (
                state
                    .last_integrity
                    .as_ref()
                    .is_none_or(|(at, _)| at.elapsed() >= INTEGRITY_RECHECK),
                state.outstanding_damage,
            )
        };
        let mut kind = if had_errors && integrity_due {
            CheckKind::IntegrityCheck
        } else {
            CheckKind::QuickCheck
        };
        kind = kind.max(outstanding.unwrap_or(CheckKind::QuickCheck));
        let stopped = self.repair_stopped();
        // Stopped, the thorough check is only there to notice the file has
        // become sound again, so it keeps the hourly cadence rather than
        // scanning the whole file on every sweep.
        if stopped && kind == CheckKind::IntegrityCheck && !integrity_due {
            return;
        }
        let check = self.check(kind);
        if check.ok {
            return;
        }
        if stopped {
            // Said once, when the stop began; not once per sweep.
            tracing::debug!(
                problems = ?check.problems,
                "the index is still damaged; automatic repair is stopped"
            );
            return;
        }
        tracing::warn!(
            problems = ?check.problems,
            "the index failed its integrity check; rebuilding it from the notes"
        );
        self.rebuild_for_damage("the sweep's integrity check failed");
    }

    /// A tool call met a corruption error, or an index between connections:
    /// check the files are still the server's (reopening and reconciling
    /// them if not, subject to the reopen spacing), then run the thorough
    /// check and rebuild if it confirms the damage.
    pub fn on_corruption_seen(&self) {
        let trigger = "a tool call met a damaged index";
        let reopened = match self.verify_files(trigger, Trigger::Damage) {
            Files::NotHeld(_) => return,
            Files::Reopened(_) => true,
            Files::Held => false,
        };
        // A client retrying in a loop sends a signal per call; one full
        // scan a minute answers them all.
        if self.recent_integrity().is_some() {
            if self.lock().outstanding_damage.is_some() && !self.repair_stopped() {
                self.rebuild_for_damage(trigger);
            }
            return;
        }
        if self.repair_stopped() {
            // Nothing this signal could start; the stop was said when it
            // began, and the sweep notices when the file is sound again.
            tracing::debug!("a tool call met a damaged index; automatic repair is stopped");
            return;
        }
        let check = self.check(CheckKind::IntegrityCheck);
        if check.ok {
            if reopened {
                tracing::info!("after the reopen the index checks sound");
            } else {
                tracing::warn!(
                    "a tool call reported index corruption, but the integrity check found none; \
                     if this repeats, another machine or container may be writing the index file \
                     (give each its own with `CUADERNO_INDEX_PATH` / `--index-path`)"
                );
            }
            return;
        }
        tracing::warn!(
            problems = ?check.problems,
            "a tool call met a damaged index; rebuilding it from the notes"
        );
        self.rebuild_for_damage(trigger);
    }

    /// An automatic rebuild, logged.
    fn rebuild_for_damage(&self, trigger: &str) {
        match self.run_rebuild(trigger, Trigger::Damage) {
            Ok(record) if record.set_aside => tracing::warn!(
                notes = record.notes,
                errors = record.errors,
                index_path = %self.sqlite.path().display(),
                "the index file was too damaged to write over; it was set aside (kept with \
                 `.previous` appended when it is the first of a streak) and the index reopened \
                 on a fresh file, filled from the notes"
            ),
            Ok(record) if record.rebuilt => tracing::warn!(
                notes = record.notes,
                errors = record.errors,
                index_path = %self.sqlite.path().display(),
                "the index was rebuilt; the first damaged file of a streak is kept beside it, \
                 with `.previous` appended to its name"
            ),
            Ok(record) => tracing::error!(
                failure = record.failure.as_deref().unwrap_or(""),
                "rebuilding the index failed; the damaged index is still live"
            ),
            // Said once, when it was decided; a retrying client would
            // otherwise log a line per call.
            Err(RebuildRefused::DamageRecurring) => {}
            Err(refused) => tracing::debug!(reason = %refused, "index rebuild skipped"),
        }
    }

    /// Rebuild the index now, as `rebuild_index` asks, unless another
    /// rebuild is running or the last finished less than the policy's
    /// spacing ago. The files are checked first, and reopened (and the
    /// reopened file reconciled) when they are not the server's, without
    /// the reopen spacing; a reopen that fails refuses the rebuild, which
    /// would otherwise land in a copy nobody else sees, and a refusal after
    /// a reopen says that the reopen happened. It runs whatever the
    /// recurrence count says: the person asked. A rebuild that runs and
    /// fails is still `Ok`: its record says why, and the old index stays
    /// live. One that succeeds ends a damage stop; the count of earlier
    /// repairs stays, and at the limit it allows one further automatic
    /// repair before the next stop. Blocking.
    pub fn rebuild(&self, trigger: &str) -> Result<RebuildRecord, RebuildRefusal> {
        let reopen = match self.verify_files(trigger, Trigger::Requested) {
            Files::NotHeld(why) => {
                return Err(RebuildRefusal {
                    refused: RebuildRefused::FilesNotHeld(why),
                    reopen: self.lock().last_reopen.clone().map(Box::new),
                });
            }
            Files::Reopened(_) => self.lock().last_reopen.clone(),
            Files::Held => None,
        };
        match self.run_rebuild(trigger, Trigger::Requested) {
            Ok(mut record) => {
                record.reopen = reopen;
                self.lock().last_rebuild = Some(record.clone());
                Ok(record)
            }
            Err(refused) => Err(RebuildRefusal {
                refused,
                reopen: reopen.map(Box::new),
            }),
        }
    }

    fn run_rebuild(&self, trigger: &str, who: Trigger) -> Result<RebuildRecord, RebuildRefused> {
        if self.rebuilding.swap(true, Ordering::AcqRel) {
            return Err(RebuildRefused::InProgress);
        }
        let _running = ClearOnDrop(&self.rebuilding);
        let keep_copy = {
            let mut state = self.lock();
            if let Some(at) = state.last_rebuild_at {
                let since = at.elapsed();
                if since < self.policy.min_rebuild_interval {
                    return Err(RebuildRefused::TooSoon(
                        self.policy.min_rebuild_interval - since,
                    ));
                }
            }
            let window = self.policy.recurrence_window;
            while state
                .damage_rebuilds
                .front()
                .is_some_and(|at| at.elapsed() >= window)
            {
                state.damage_rebuilds.pop_front();
            }
            if who == Trigger::Damage {
                if state.recurring.is_some() {
                    return Err(RebuildRefused::DamageRecurring);
                }
                if state.damage_rebuilds.len() >= self.policy.recurrence_limit {
                    if state.grace {
                        state.grace = false;
                    } else {
                        state.recurring =
                            Some(state.outstanding_damage.unwrap_or(CheckKind::QuickCheck));
                        tracing::error!(
                            repairs = state.damage_rebuilds.len(),
                            window_secs = window.as_secs(),
                            index_path = %self.sqlite.path().display(),
                            "automatic index repair stopped: {DAMAGE_RECURRING_ADVICE}"
                        );
                        publish_hint(&state);
                        return Err(RebuildRefused::DamageRecurring);
                    }
                }
            }
            // The first copy of a streak is kept; later ones would overwrite
            // that evidence with a copy of the re-damage.
            let first_of_streak = state.copy_kept_at.is_none_or(|at| at.elapsed() >= window);
            if first_of_streak && state.outstanding_damage.is_some() {
                state.copy_kept_at = Some(Instant::now());
            }
            first_of_streak
        };

        let failed = |failure: String| RebuildRecord {
            at: now(),
            trigger: trigger.to_string(),
            rebuilt: false,
            notes: 0,
            errors: 0,
            snapshots_carried: 0,
            snapshots_lost: Vec::new(),
            failure: Some(failure),
            set_aside: false,
            reopen: None,
        };
        let record = match rebuild(&self.store, &self.sqlite, &self.scope, keep_copy) {
            Ok(mut report) => {
                // The record keeps one list: a failure to read any snapshot
                // at all is its one line, as before the report split it out.
                if let Some(e) = report.snapshots_unreadable.take() {
                    report
                        .snapshots_lost
                        .push(format!("the old index's snapshots could not be read: {e}"));
                }
                if !report.snapshots_lost.is_empty() {
                    tracing::warn!(
                        lost = ?report.snapshots_lost,
                        "some archival snapshots could not be carried into the rebuilt index; \
                         lint can no longer check those archived notes' frozen prefix"
                    );
                }
                // The rebuild's last pass is now the latest word on which
                // notes index; a sweep from before it would read stale.
                let errors = ReconciliationReport {
                    scanned: report.scanned,
                    ignored: report.ignored,
                    artefacts: report.artefacts,
                    errors: report.errors,
                    ..ReconciliationReport::default()
                };
                {
                    let mut state = self.lock();
                    state.last_sweep = Some(sweep_record(&errors, None));
                    self.after_repair(&mut state, who);
                }
                RebuildRecord {
                    at: now(),
                    trigger: trigger.to_string(),
                    rebuilt: true,
                    notes: report.notes,
                    errors: errors.errors.len(),
                    snapshots_carried: report.snapshots_carried,
                    snapshots_lost: report.snapshots_lost,
                    failure: None,
                    set_aside: false,
                    reopen: None,
                }
            }
            // Too damaged to write over in place (its header): what only a
            // restart could replace before #1000. Set it aside and reopen,
            // serialised with the file check so a sweep does not take this
            // server's own move for one made under it.
            Err(e) if e.is_corruption() => {
                let aside = {
                    let _one_at_a_time = self.lock_verifying();
                    self.set_aside_unopenable(keep_copy)
                };
                match aside {
                    Ok(aside) => {
                        let sweep = self.reconcile_and_record();
                        let (carried, mut lost) = self.carry_snapshots(aside.reopened.salvaged);
                        if aside.done {
                            lost.insert(
                                0,
                                format!(
                                    "the damaged file's archival snapshots could not be read \
                                     ({e}); it was set aside rather than rebuilt in place"
                                ),
                            );
                        }
                        let notes = self.index.list_all_paths().map_or(0, |p| p.len());
                        self.after_repair(&mut self.lock(), who);
                        self.record_files(&self.sqlite.check_files());
                        RebuildRecord {
                            at: now(),
                            trigger: trigger.to_string(),
                            rebuilt: true,
                            notes,
                            errors: sweep.errors,
                            snapshots_carried: carried,
                            snapshots_lost: lost,
                            failure: None,
                            set_aside: aside.done,
                            reopen: None,
                        }
                    }
                    Err(aside) => failed(format!(
                        "{e}; the file is too damaged to write over, and setting it aside \
                         failed: {aside}"
                    )),
                }
            }
            Err(e) => failed(e.to_string()),
        };
        let mut state = self.lock();
        state.last_rebuild = Some(record.clone());
        state.last_rebuild_at = Some(Instant::now());
        publish_hint(&state);
        Ok(record)
    }

    /// The bookkeeping of a repair that replaced the index.
    fn after_repair(&self, state: &mut State, who: Trigger) {
        state.outstanding_damage = None;
        // A thorough check of the replaced file says nothing about the new
        // one.
        state.last_integrity = None;
        match who {
            // Only a repair that replaced the index counts towards the
            // recurrence limit.
            Trigger::Damage => state.damage_rebuilds.push_back(Instant::now()),
            // The person's rebuild ends a stop, but keeps the history: at
            // the limit it buys one more automatic repair, not a fresh
            // count.
            Trigger::Requested => {
                state.recurring = None;
                state.grace = state.damage_rebuilds.len() >= self.policy.recurrence_limit;
            }
        }
    }

    /// The status `index_status` reports: a fresh file check and
    /// `quick_check`, plus the latest records. While damage only
    /// `integrity_check` sees is outstanding, a quick pass cannot report
    /// healthy over it, so the thorough check is reported instead: the one
    /// from the last minute when there is one, else one run now, which
    /// reads the whole file and holds the index while it does. While the
    /// server does not hold its files no check runs: it would describe the
    /// server's own copy, and a pass there must not end a damage stop. It
    /// reports; the sweep, or the next tool call to meet damage, reopens.
    /// Blocking.
    pub fn status(&self) -> IndexStatus {
        let files = self.check_files();
        let check = if !files.held {
            CheckRecord {
                at: now(),
                kind: CheckKind::QuickCheck,
                ok: false,
                problems: vec![format!(
                    "not checked: this server's connection does not hold the index files at its \
                     path ({}); it reopens them at its next sweep, or the next tool call that \
                     meets damage, at most every {}s",
                    files.found.as_deref().unwrap_or_default(),
                    self.policy.min_reopen_interval.as_secs().max(1)
                )],
            }
        } else {
            // Bound first: a guard in the scrutinee would live through the
            // arms, and `check` takes the state lock.
            let outstanding = self.lock().outstanding_damage;
            match outstanding {
                Some(CheckKind::IntegrityCheck) => self
                    .recent_integrity()
                    .unwrap_or_else(|| self.check(CheckKind::IntegrityCheck)),
                _ => self.check(CheckKind::QuickCheck),
            }
        };
        let mut state = self.lock();
        self.refresh_reopen_recurring(&mut state);
        let repair = repair_state(&state);
        let window = self.policy.recurrence_window;
        IndexStatus {
            healthy: files.held
                && check.ok
                && state.outstanding_damage.is_none()
                && state.recurring.is_none()
                && state.reopen_recurring.is_none(),
            repair,
            advice: match repair {
                RepairState::Automatic => None,
                RepairState::DamageRecurring => Some(DAMAGE_RECURRING_ADVICE.to_string()),
                RepairState::ReopenRecurring => state
                    .reopen_recurring
                    .map(|moved| reopen_advice(moved).to_string()),
            },
            damage_repair_stopped: state.recurring.is_some(),
            damage_rebuilds_recent: state
                .damage_rebuilds
                .iter()
                .filter(|at| at.elapsed() < window)
                .count(),
            reopens_recent: state.reopens.len(),
            index_path: self.sqlite.path().display().to_string(),
            files,
            notes_not_indexed: state.last_sweep.as_ref().map_or(0, |s| s.errors),
            check,
            last_sweep: state.last_sweep.clone(),
            last_rebuild: state.last_rebuild.clone(),
            last_reopen: state.last_reopen.clone(),
            damage_at_open: state.damage_at_open.clone(),
        }
    }

    /// Start the health loop with a sweep every `interval_secs` seconds, or
    /// with the sweeps off when it is `0`, and say which at start. Both
    /// servers start their loop here (#1002), so they share one interval
    /// default, one meaning of `0` and one loop. The handle lets a caller
    /// that is shutting down stop the loop from starting another sweep;
    /// dropping it leaves the loop running.
    pub fn start(self: &Arc<Self>, interval_secs: u64) -> tokio::task::JoinHandle<()> {
        if interval_secs > 0 {
            tracing::info!(
                interval_secs,
                "periodic reconciliation and index check enabled"
            );
            self.spawn_loop(Some(Duration::from_secs(interval_secs)))
        } else {
            tracing::warn!(
                "periodic reconciliation disabled ({SWEEP_INTERVAL_ENV} or \
                 --reconcile-interval-secs is 0); out-of-band edits will not appear in the index \
                 until restart"
            );
            // Still listen for a tool call that meets a damaged index.
            self.spawn_loop(None)
        }
    }

    /// Run [`sweep`](Self::sweep) every `every`, and
    /// [`on_corruption_seen`](Self::on_corruption_seen) whenever a tool
    /// call reports corruption. With `every` `None` (sweeps turned off)
    /// only the second runs, so a tool call that meets damage still starts
    /// a repair on every server.
    ///
    /// A sweep runs on the blocking pool, one at a time: the next tick
    /// waits for the running sweep to finish, and a tick missed meanwhile
    /// is delayed rather than replayed. The first sweep comes after a
    /// random [`first_sweep_delay`], between half an interval and one.
    /// Aborting the returned task stops
    /// further sweeps; a sweep already running finishes on its thread.
    ///
    /// Known benign race (PR #304 review, correctness finding 2): a sweep
    /// reads file state *before* taking the per-note write lock, so a
    /// tool-call write landing in that window can be shadowed by a stale
    /// index row, which the *next* sweep heals (mtime mismatch). Worst
    /// case is stale search results for one interval; no note is touched.
    pub fn spawn_loop(self: &Arc<Self>, every: Option<Duration>) -> tokio::task::JoinHandle<()> {
        let health = Arc::clone(self);
        tokio::spawn(async move {
            // Opening the vault has just reconciled and checked, so the first
            // sweep waits; how long is drawn between half an interval and a
            // whole one, so servers started together (sessions a client
            // restores at once) do not sweep in phase for ever.
            // An interval past the cap means "never" in practice; capping
            // it keeps every deadline the loop computes (now + delay, then
            // + interval per tick) representable, since adding to an
            // `Instant` panics on overflow and the panic would end this
            // task, the corruption listener with it.
            let mut interval = every.map(|every| {
                let every = every.min(MAX_SWEEP_INTERVAL);
                let now = tokio::time::Instant::now();
                let first = now
                    .checked_add(first_sweep_delay(every, random_u64()))
                    .unwrap_or(now + MAX_SWEEP_INTERVAL);
                let mut interval = tokio::time::interval_at(first, every);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                interval
            });
            loop {
                let corruption = match interval.as_mut() {
                    Some(interval) => tokio::select! {
                        _ = interval.tick() => false,
                        _ = CORRUPTION_SEEN.notified() => true,
                    },
                    None => {
                        CORRUPTION_SEEN.notified().await;
                        true
                    }
                };
                let h = Arc::clone(&health);
                let ran = tokio::task::spawn_blocking(move || {
                    if corruption {
                        h.on_corruption_seen();
                    } else {
                        h.sweep();
                    }
                })
                .await;
                if let Err(e) = ran {
                    tracing::warn!(error = %e, "index health task panicked");
                }
            }
        })
    }
}

/// The wait before the first sweep: half of `every` plus a share of the
/// other half picked by `random`, so always in `[every / 2, every)` (or
/// `every` itself when it is too short to split).
pub fn first_sweep_delay(every: Duration, random: u64) -> Duration {
    let half = every / 2;
    let spread = u64::try_from((every - half).as_millis()).unwrap_or(u64::MAX);
    if spread == 0 {
        return every;
    }
    half.saturating_add(Duration::from_millis(random % spread))
}

/// The longest sweep interval the loop uses: ten years, which is "never"
/// for a server process. A longer one (up to `u64::MAX` seconds, which a
/// configuration may give to mean never) is taken as this.
pub const MAX_SWEEP_INTERVAL: Duration = Duration::from_secs(10 * 365 * 24 * 3600);

/// A random number from the standard library's per-process hash seed: no
/// dependency, and different in every process, which is all the jitter
/// needs.
fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    hasher.finish()
}

fn repair_state(state: &State) -> RepairState {
    if state.recurring.is_some() {
        RepairState::DamageRecurring
    } else if state.reopen_recurring.is_some() {
        RepairState::ReopenRecurring
    } else {
        RepairState::Automatic
    }
}

/// The advice for reopens that keep recurring, by what they found: vanished
/// sidecars are the cross-kernel signature, a replaced database file is
/// nearly always something on this machine.
fn reopen_advice(moved: Moved) -> &'static str {
    match moved {
        Moved::Sidecars => REOPEN_RECURRING_ADVICE,
        Moved::Database => REPLACED_RECURRING_ADVICE,
    }
}

/// What a reopen did, in a sentence for its record and its log line.
fn describe_reopen(record: &ReopenRecord) -> String {
    let mut done = Vec::new();
    if record.stale_sidecars_set_aside {
        done.push(
            "moved the old file's -wal and -shm, left at the path, aside to `.stale-wal` /              `.stale-shm`"
                .to_owned(),
        );
    }
    if record.set_aside {
        done.push(match &record.previous {
            Some(previous) => format!("set the unopenable file at the path aside to {previous}"),
            None => "deleted the unopenable file at the path (the first damaged copy of this                      streak is already kept)"
                .to_owned(),
        });
    }
    done.push("reopened the index on the file at its path and reconciled it".to_owned());
    if record.snapshots_carried > 0 || !record.snapshots_lost.is_empty() {
        done.push(format!(
            "carried {} archival snapshot(s) from the old connection, {} lost",
            record.snapshots_carried,
            record.snapshots_lost.len()
        ));
    }
    done.join("; ")
}

/// Make [`damage_hint`] say what this server now does.
fn publish_hint(state: &State) {
    let byte = match (repair_state(state), state.reopen_recurring) {
        (RepairState::DamageRecurring, _) => 1,
        (RepairState::ReopenRecurring, Some(Moved::Sidecars)) => 2,
        (RepairState::ReopenRecurring, _) => 3,
        (RepairState::Automatic, _) => 0,
    };
    HINT_STATE.store(byte, Ordering::Release);
}

/// Clears the in-progress flag however the rebuild ends.
struct ClearOnDrop<'a>(&'a AtomicBool);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn now() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

fn sweep_record(report: &ReconciliationReport, failure: Option<String>) -> SweepRecord {
    SweepRecord {
        at: now(),
        scanned: report.scanned,
        added: report.added,
        updated: report.updated,
        removed: report.removed,
        errors: report.errors.len(),
        failing: report
            .errors
            .iter()
            .take(FAILING_NOTES_SHOWN)
            .map(|issue| FailingNote {
                path: issue.path.to_string(),
                reason: issue.reason.clone(),
            })
            .collect(),
        failure,
    }
}

/// Log a sweep. Failing notes are a warning and are named: a count alone
/// is what let a damaged index go unread for days.
fn log_sweep(report: &ReconciliationReport) {
    if !report.errors.is_empty() {
        let shown: Vec<String> = report
            .errors
            .iter()
            .take(FAILING_NOTES_SHOWN)
            .map(|issue| format!("{}: {}", issue.path, issue.reason))
            .collect();
        tracing::warn!(
            scanned = report.scanned,
            added = report.added,
            updated = report.updated,
            removed = report.removed,
            errors = report.errors.len(),
            failing = ?shown,
            "periodic reconciliation could not index some notes"
        );
    } else if report.added + report.updated + report.removed > 0 {
        tracing::info!(
            scanned = report.scanned,
            added = report.added,
            updated = report.updated,
            removed = report.removed,
            "periodic reconciliation applied out-of-band changes"
        );
    } else {
        tracing::debug!(
            scanned = report.scanned,
            "periodic reconciliation: nothing changed"
        );
    }
}
