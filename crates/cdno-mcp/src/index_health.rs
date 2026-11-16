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
//!    the same handle, and the index reconciled; then the checks below run
//!    on the file as it is. A reopen is logged at WARN with what was found.
//!    It fixes the view, not damage, so it does not count towards the
//!    damage recurrence below; but files that keep moving mean something
//!    else is managing this index file, so reopens are counted too, and
//!    after [`RepairPolicy::reopen_limit`] of them in the window the server
//!    stops reopening by itself and says so once, with the same advice.
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
//!    next sweep and reopen too. Rebuilds are serialised and spaced at least
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
//!   allows one more automatic repair, not a fresh count) or a restart. The
//!   reopen stop works the same way, and ends at a `rebuild_index` (which
//!   reopens first, and allows one more automatic reopen) or a restart.
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
    /// How many automatic reopens within the window mean something else
    /// keeps deleting or replacing the index's files: the next time they are
    /// found gone is reported, not reopened (#1000).
    pub reopen_limit: usize,
}

impl Default for RepairPolicy {
    fn default() -> Self {
        Self {
            min_rebuild_interval: MIN_REBUILD_INTERVAL,
            recurrence_window: Duration::from_secs(3600),
            recurrence_limit: 3,
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

/// Tell the health loop that a tool call met a damaged index, so it
/// checks now rather than at the next sweep.
pub fn corruption_seen() {
    CORRUPTION_SEEN.notify_one();
}

/// The sentence appended to every client error caused by a damaged index,
/// true to what this server will do about it now.
pub fn damage_hint() -> &'static str {
    match HINT_STATE.load(Ordering::Acquire) {
        1 => DAMAGE_RECURRING_HINT,
        2 => REOPEN_RECURRING_HINT,
        _ => REPAIRING_HINT,
    }
}

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

/// The advice when the index's files keep being deleted or replaced under
/// the server, in the log, in `index_status` and in the client hint.
pub const REOPEN_RECURRING_ADVICE: &str = "the index's files keep being deleted or replaced \
     under this server, so something else is managing this index file, most likely another \
     machine or a container sharing the vault folder; give this machine its own index with \
     `CUADERNO_INDEX_PATH` (or `--index-path` on cdno-mcp-server)";

const REOPEN_RECURRING_HINT: &str = "the notes on disk are intact; the index's files (a cache \
     of them) keep being deleted or replaced under the server, so it has stopped reopening them \
     by itself and may answer from a copy no other process sees: something else is managing the \
     index file, most likely another machine or a container sharing the vault folder. Tell the \
     person to give this machine its own index (`CUADERNO_INDEX_PATH`, or `--index-path` on \
     cdno-mcp-server). `index_status` shows the state; `rebuild_index` reopens it once.";

/// The index's health supervisor for one server process.
pub struct IndexHealth {
    store: Arc<dyn VaultStore>,
    index: Arc<dyn VaultIndex>,
    sqlite: Arc<SqliteIndex>,
    scope: Arc<NoteScope>,
    policy: RepairPolicy,
    state: Mutex<State>,
    rebuilding: AtomicBool,
    /// Held while the files are checked and, if need be, reopened, so a
    /// sweep and a tool call's signal that find the same move reopen once.
    verifying: Mutex<()>,
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
    /// When each automatic reopen in the recurrence window happened. Failed
    /// reopens are not counted, as failed rebuilds are not.
    reopens: VecDeque<Instant>,
    /// Set when the files kept being found gone after reopens: automatic
    /// reopening is off until a requested rebuild reopens them, or the
    /// server restarts.
    reopen_stopped: bool,
    /// One automatic reopen allowed past the limit, granted as `grace` is.
    reopen_grace: bool,
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
    /// Set when the pass failed as a whole rather than note by note.
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
    /// lines; empty when `ok`.
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
    /// True when the database file, its `-wal` and its `-shm` are the ones
    /// the connection has open. False means the server reads and writes a
    /// copy no other process sees, until it reopens.
    pub held: bool,
    /// What was found when they are not: the file deleted, moved aside or
    /// replaced, or a sidecar deleted or replaced.
    pub found: Option<String>,
}

/// One reopen of the index on the files at its path.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ReopenRecord {
    pub at: String,
    /// What asked: the sweep, a tool call that met a damaged index, or a
    /// `rebuild_index` call.
    pub trigger: String,
    /// What the file check found that called for it.
    pub found: String,
    /// True when the index now holds the files at its path.
    pub reopened: bool,
    /// Why the reopen failed, when it did (a file bound to another vault,
    /// one that is not cuaderno's index, an I/O error).
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
    /// The index's files kept being deleted or replaced under the server,
    /// so it has stopped reopening them by itself; `advice` says why and
    /// what to do.
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
    /// True when the live file was too damaged to be written over in place
    /// (its header, typically), so it was moved aside to `.previous` under
    /// the vault write lock and the index reopened on a fresh file, filled
    /// from the notes. No restart is needed; other servers on this machine
    /// reopen on their next sweep. Its archival snapshots are lost.
    pub set_aside: bool,
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
    /// still holds the files at `index_path`. When it does not, `check`
    /// describes the server's own copy, not the file.
    pub files: FilesRecord,
    /// The check reported for this call: a fresh `quick_check`, or, while
    /// damage only `integrity_check` sees is outstanding, a thorough check
    /// from the last minute or run for this call.
    pub check: CheckRecord,
    /// How many notes the latest sweep (or rebuild) could not index, listed
    /// in `last_sweep.failing`. Usually malformed notes: `lint` names them,
    /// and fixing the note fixes it.
    pub notes_not_indexed: usize,
    pub last_sweep: Option<SweepRecord>,
    pub last_rebuild: Option<RebuildRecord>,
    /// The latest reopen: when, why, what was found and whether it worked.
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

/// Who asked for a rebuild or a reopen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    /// The server itself: a sweep, a failed check, or a tool call's
    /// corruption. Counted, and stopped when it recurs.
    Damage,
    /// `rebuild_index`: always runs, subject only to the spacing.
    Requested,
}

/// What looking at the files found, before anything acts on the
/// connection's view of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Files {
    /// The connection holds the files at the path.
    Held,
    /// It did not, and the index was reopened on the files at the path.
    Reopened,
    /// It does not, and was not reopened: the reopen stop, or a reopen
    /// that failed.
    NotHeld,
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

    /// Look at whether the connection still holds the files at the index's
    /// path ([`SqliteIndex::check_files`]), and record it.
    pub fn check_files(&self) -> FilesRecord {
        let found = self.sqlite.check_files();
        let record = FilesRecord {
            at: now(),
            held: found.is_held(),
            found: found.finding().map(str::to_owned),
        };
        self.lock().last_files = Some(record.clone());
        record
    }

    /// The first response to trouble (#1000): check that the connection
    /// still holds the files at the path, and when it does not, reopen it on
    /// them, unless reopens keep recurring and this is automatic. A file at
    /// the path that will not open is set aside under the vault write lock
    /// and the index reopened on a fresh one. Logged at WARN per reopen.
    fn verify_files(&self, trigger: &str, who: Trigger) -> Files {
        let _one_at_a_time = self
            .verifying
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let found = self.sqlite.check_files();
        let files = FilesRecord {
            at: now(),
            held: found.is_held(),
            found: found.finding().map(str::to_owned),
        };
        self.lock().last_files = Some(files);
        let Some(finding) = found.finding().map(str::to_owned) else {
            if who == Trigger::Requested {
                self.end_reopen_stop();
            }
            return Files::Held;
        };
        if who == Trigger::Damage {
            let mut state = self.lock();
            let window = self.policy.recurrence_window;
            while state
                .reopens
                .front()
                .is_some_and(|at| at.elapsed() >= window)
            {
                state.reopens.pop_front();
            }
            if state.reopen_stopped {
                // Said once, when the stop began.
                tracing::debug!(found = %finding, "the index files are not held; reopening is stopped");
                return Files::NotHeld;
            }
            if state.reopens.len() >= self.policy.reopen_limit {
                if state.reopen_grace {
                    state.reopen_grace = false;
                } else {
                    state.reopen_stopped = true;
                    tracing::error!(
                        reopens = state.reopens.len(),
                        window_secs = window.as_secs(),
                        found = %finding,
                        index_path = %self.sqlite.path().display(),
                        "automatic index reopening stopped: {REOPEN_RECURRING_ADVICE}"
                    );
                    publish_hint(&state);
                    return Files::NotHeld;
                }
            }
        }

        let reopened = match self.sqlite.reopen() {
            // The file at the path will not open: set it aside, as a restart
            // would, and start from a fresh one.
            Err(e) if e.is_corruption() => self
                .set_aside_and_reopen()
                .map_err(|aside| format!("{e}; setting it aside failed too: {aside}")),
            other => other.map_err(|e| e.to_string()),
        };
        let mut state = self.lock();
        let record = ReopenRecord {
            at: now(),
            trigger: trigger.to_owned(),
            found: finding.clone(),
            reopened: reopened.is_ok(),
            failure: reopened.as_ref().err().cloned(),
        };
        state.last_reopen = Some(record);
        let outcome = match reopened {
            Ok(()) => {
                tracing::warn!(
                    found = %finding,
                    trigger,
                    index_path = %self.sqlite.path().display(),
                    "the index files changed under this server; reopened the index on the file \
                     at its path"
                );
                // What the checks found was the old connection's view.
                state.outstanding_damage = None;
                state.last_integrity = None;
                state.last_files = Some(FilesRecord {
                    at: now(),
                    held: true,
                    found: None,
                });
                match who {
                    Trigger::Damage => state.reopens.push_back(Instant::now()),
                    Trigger::Requested => {
                        state.reopen_stopped = false;
                        state.reopen_grace = state.reopens.len() >= self.policy.reopen_limit;
                    }
                }
                Files::Reopened
            }
            Err(failure) => {
                tracing::error!(
                    found = %finding,
                    failure = %failure,
                    index_path = %self.sqlite.path().display(),
                    "the index files changed under this server, and reopening them failed; it \
                     keeps its old connection"
                );
                Files::NotHeld
            }
        };
        publish_hint(&state);
        outcome
    }

    /// A requested rebuild found the files held: it ends a reopen stop as it
    /// ends a damage stop, allowing one more automatic reopen at the limit.
    fn end_reopen_stop(&self) {
        let mut state = self.lock();
        if state.reopen_stopped {
            state.reopen_stopped = false;
            state.reopen_grace = state.reopens.len() >= self.policy.reopen_limit;
            publish_hint(&state);
        }
    }

    /// Replace a live file too damaged to open or to write over: under the
    /// vault write lock, move it aside to `.previous` (the open's guards
    /// first, for an overridden index) and reopen the index on a fresh file
    /// at the path, for the caller's reconciliation to fill. Done before
    /// only at a restart. When the path already names another file (another
    /// process replaced it meanwhile), only the reopen. Other processes on
    /// this machine holding the old file find it moved: a server reopens on
    /// its next sweep, and a CLI command is short-lived.
    fn set_aside_and_reopen(&self) -> Result<(), IndexError> {
        let location = self.sqlite.location();
        let _lock = self.store.acquire_write_lock().map_err(|e| {
            IndexError::Update(format!(
                "acquiring the vault write lock to set the damaged index aside: {e}"
            ))
        })?;
        if !matches!(self.sqlite.check_files(), FileCheck::DatabaseMoved(_)) {
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
            SqliteIndex::set_aside(&location)?;
        }
        self.sqlite.reopen()
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
    /// damage. Blocking; the loop runs it on the blocking pool.
    pub fn sweep(&self) {
        let files = self.verify_files("the sweep", Trigger::Damage);
        let record = self.reconcile_and_record();
        if files == Files::NotHeld {
            // The check would describe a copy no other process sees, and a
            // rebuild would land in it.
            return;
        }
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

    /// A tool call met a corruption error: check the files are still the
    /// server's (reopening and reconciling them if not), then run the
    /// thorough check and rebuild if it confirms the damage.
    pub fn on_corruption_seen(&self) {
        let trigger = "a tool call met a damaged index";
        let reopened = match self.verify_files(trigger, Trigger::Damage) {
            Files::NotHeld => return,
            Files::Reopened => {
                self.reconcile_and_record();
                true
            }
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
                "the index file was too damaged to write over; it was set aside with `.previous` \
                 appended to its name and the index reopened on a fresh file, filled from the notes"
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
    /// spacing ago. The files are checked first, and reopened when they are
    /// not the server's, whatever the reopen count says; a reopen that fails
    /// refuses the rebuild, which would otherwise land in a copy nobody else
    /// sees. It runs whatever the recurrence count says: the person asked.
    /// A rebuild that runs and fails is still `Ok`: its record says why, and
    /// the old index stays live. One that succeeds ends a stop; the count of
    /// earlier repairs stays, and at the limit it allows one further
    /// automatic repair before the next stop. Blocking.
    pub fn rebuild(&self, trigger: &str) -> Result<RebuildRecord, RebuildRefused> {
        if self.verify_files(trigger, Trigger::Requested) == Files::NotHeld {
            let why = self
                .lock()
                .last_reopen
                .as_ref()
                .and_then(|r| r.failure.clone())
                .unwrap_or_default();
            return Err(RebuildRefused::FilesNotHeld(why));
        }
        self.run_rebuild(trigger, Trigger::Requested)
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
                }
            }
            // Too damaged to write over in place (its header): what only a
            // restart could replace before #1000. Set it aside and reopen.
            Err(e) if e.is_corruption() => match self.set_aside_and_reopen() {
                Ok(()) => {
                    let sweep = self.reconcile_and_record();
                    let notes = self.index.list_all_paths().map_or(0, |p| p.len());
                    self.after_repair(&mut self.lock(), who);
                    RebuildRecord {
                        at: now(),
                        trigger: trigger.to_string(),
                        rebuilt: true,
                        notes,
                        errors: sweep.errors,
                        snapshots_carried: 0,
                        snapshots_lost: vec![format!(
                            "the damaged file's archival snapshots could not be read ({e}); it \
                             was set aside rather than rebuilt in place"
                        )],
                        failure: None,
                        set_aside: true,
                    }
                }
                Err(aside) => RebuildRecord {
                    at: now(),
                    trigger: trigger.to_string(),
                    rebuilt: false,
                    notes: 0,
                    errors: 0,
                    snapshots_carried: 0,
                    snapshots_lost: Vec::new(),
                    failure: Some(format!(
                        "{e}; the file is too damaged to write over, and setting it aside failed: \
                         {aside}"
                    )),
                    set_aside: false,
                },
            },
            Err(e) => RebuildRecord {
                at: now(),
                trigger: trigger.to_string(),
                rebuilt: false,
                notes: 0,
                errors: 0,
                snapshots_carried: 0,
                snapshots_lost: Vec::new(),
                failure: Some(e.to_string()),
                set_aside: false,
            },
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
    /// reads the whole file and holds the index while it does. It reports
    /// files that are not the server's; the sweep, or the next tool call to
    /// meet damage, reopens them. Blocking.
    pub fn status(&self) -> IndexStatus {
        let files = self.check_files();
        let outstanding = self.lock().outstanding_damage;
        let check = match outstanding {
            Some(CheckKind::IntegrityCheck) => self
                .recent_integrity()
                .unwrap_or_else(|| self.check(CheckKind::IntegrityCheck)),
            _ => self.check(CheckKind::QuickCheck),
        };
        let state = self.lock();
        let repair = repair_state(&state);
        let window = self.policy.recurrence_window;
        IndexStatus {
            healthy: files.held
                && check.ok
                && state.outstanding_damage.is_none()
                && state.recurring.is_none()
                && !state.reopen_stopped,
            repair,
            advice: match repair {
                RepairState::Automatic => None,
                RepairState::DamageRecurring => Some(DAMAGE_RECURRING_ADVICE.to_string()),
                RepairState::ReopenRecurring => Some(REOPEN_RECURRING_ADVICE.to_string()),
            },
            damage_rebuilds_recent: state
                .damage_rebuilds
                .iter()
                .filter(|at| at.elapsed() < window)
                .count(),
            reopens_recent: state
                .reopens
                .iter()
                .filter(|at| at.elapsed() < window)
                .count(),
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
    if state.reopen_stopped {
        RepairState::ReopenRecurring
    } else if state.recurring.is_some() {
        RepairState::DamageRecurring
    } else {
        RepairState::Automatic
    }
}

/// Make [`damage_hint`] say what this server now does.
fn publish_hint(state: &State) {
    let byte = match repair_state(state) {
        RepairState::Automatic => 0,
        RepairState::DamageRecurring => 1,
        RepairState::ReopenRecurring => 2,
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
