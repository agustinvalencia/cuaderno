//! Index health for a long-lived server: assess, repair, report (#526).
//!
//! The index is a cache of the notes, so it is the one thing in the vault
//! that may be repaired automatically: rebuilding it from the markdown
//! loses nothing. This module gives a running server that authority.
//! Before it, the periodic reconciliation could only log a count of the
//! notes it failed to index, and a damaged database stayed damaged until
//! somebody with a shell on the host deleted it.
//!
//! - **Assess.** Every sweep, on either server ([`IndexHealth::start`]
//!   starts the loop for both, #1002), reconciles, then runs `PRAGMA quick_check`
//!   (the thorough `integrity_check` when the sweep could not index some
//!   notes, at most once an hour, and whenever damage only the thorough
//!   check sees is still outstanding). Page-level damage can leave every
//!   read working while writes to the damaged pages fail, so the check, not
//!   a query error, is the signal that can be relied on. A tool call that
//!   meets a corruption error wakes the loop at once ([`corruption_seen`])
//!   for `integrity_check`.
//! - **Repair.** A failed check rebuilds the index from the notes and
//!   copies it into the live file in place
//!   ([`cdno_core::reconcile::rebuild`]), with no restart, so every process
//!   on this machine using the vault sees the repair. Rebuilds are
//!   serialised and spaced at least [`RepairPolicy::min_rebuild_interval`]
//!   apart.
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
//!   latest sweep, check and rebuild are kept for `index_status`.
//!
//! What it never does: touch a note. Automatic repair stops at derived
//! state; the notes are the source of truth, and `lint` reports on them
//! without changing them.
//!
//! A known blind spot, left to #1000: the check reads through this
//! server's one long-lived connection, so it reports the server's view of
//! the file, not the file. When the database or its sidecars are unlinked
//! or replaced under the server, the check can pass on a private copy no
//! other process sees, or fail on a file that is sound, and a rebuild then
//! lands in that private file or WAL too, so "every process on this machine
//! sees the repair" does not hold in that state either; detecting it, and
//! reopening the connection before rebuilding, is #1000's.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cdno_core::classify::NoteScope;
use cdno_core::index::{SqliteIndex, VaultIndex};
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
    /// The window over which damage-triggered rebuilds are counted.
    pub recurrence_window: Duration,
    /// How many damage-triggered rebuilds within the window mean the damage
    /// keeps coming back: the next damage is reported, not repaired.
    pub recurrence_limit: usize,
}

impl Default for RepairPolicy {
    fn default() -> Self {
        Self {
            min_rebuild_interval: MIN_REBUILD_INTERVAL,
            recurrence_window: Duration::from_secs(3600),
            recurrence_limit: 3,
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
        2 => RESTART_REQUIRED_HINT,
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

/// The advice when only a restart can repair the file.
pub const RESTART_REQUIRED_ADVICE: &str = "the index file is too damaged for the running server \
     to write over; restart the server, whose start replaces such a file. The notes are intact";

const RESTART_REQUIRED_HINT: &str = "the notes on disk are intact; the index (a cache of them) is \
     too damaged for the running server to repair. Tell the person the server needs a restart. \
     `index_status` shows the state.";

/// The index's health supervisor for one server process.
pub struct IndexHealth {
    store: Arc<dyn VaultStore>,
    index: Arc<dyn VaultIndex>,
    sqlite: Arc<SqliteIndex>,
    scope: Arc<NoteScope>,
    policy: RepairPolicy,
    state: Mutex<State>,
    rebuilding: AtomicBool,
}

#[derive(Default)]
struct State {
    last_sweep: Option<SweepRecord>,
    last_check: Option<CheckRecord>,
    last_rebuild: Option<RebuildRecord>,
    last_rebuild_at: Option<Instant>,
    damage_at_open: Option<Vec<String>>,
    /// The strongest check that has failed since the index was last known
    /// sound. Cleared by a passing check at least that strong, or by a
    /// rebuild. `quick_check` cannot see what `integrity_check` found, so
    /// a later quick pass must not clear it.
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
    /// Set when a rebuild failed because the file is too damaged to write
    /// over: automatic repair is off until a check passes (the file reads
    /// and is sound again), a requested rebuild succeeds, or the server
    /// restarts.
    restart_required: bool,
    /// One automatic repair allowed past the recurrence limit, granted by a
    /// requested rebuild that succeeded while at the limit. A requested
    /// rebuild buys one round, not a fresh count, so answering every stop
    /// with `rebuild_index` cannot loop the repair on a shared index.
    grace: bool,
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

/// What the server does about damage now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RepairState {
    /// It checks the index and repairs damage by itself.
    Automatic,
    /// The damage kept coming back after repairs, so it has stopped
    /// repairing by itself; `advice` says why and what to do.
    DamageRecurring,
    /// The file is too damaged for the running server to write over; only
    /// a restart can replace it.
    RestartRequired,
}

/// One rebuild of the index.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RebuildRecord {
    pub at: String,
    /// Why it ran: a failed sweep check, a corruption a tool call met, or
    /// a `rebuild_index` call.
    pub trigger: String,
    /// True when the fresh index was copied in.
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
    /// True when the failure means the live file is too damaged to be
    /// written over in place (its header, typically): a running server
    /// cannot repair it, and a restart, whose open replaces such a file,
    /// is needed. The notes are intact.
    pub restart_required: bool,
}

/// What `index_status` returns.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct IndexStatus {
    /// The verdict on the index itself: true when the check reported for
    /// this call found nothing, no stronger check has found damage since
    /// the index was last known sound, and the damage is not recurring.
    /// Notes that could not be indexed do not make it false: they are
    /// nearly always notes that do not parse, which no rebuild can fix. It
    /// describes this server's view of the file.
    pub healthy: bool,
    /// What the server does about damage now: `automatic`,
    /// `damage_recurring` or `restart_required`.
    pub repair: RepairState,
    /// What to do, when `repair` is not `automatic`.
    pub advice: Option<String>,
    /// Damage-triggered rebuilds within the last hour (the recurrence
    /// window).
    pub damage_rebuilds_recent: usize,
    /// The index database file this server uses, after the per-machine
    /// override was resolved (#998). Every file a rebuild writes (the
    /// fresh `.rebuild-*` index while it runs, the `.previous` copy of a
    /// damaged file) sits beside it.
    pub index_path: String,
    /// True when the latest rebuild failed in a way only a restart can fix:
    /// tell the person the server needs a restart. The notes are intact.
    pub restart_required: bool,
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
    /// An automatic rebuild only: the last one found the file too damaged
    /// to write over, so another cannot succeed until a restart.
    RestartRequired,
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
            Self::RestartRequired => write!(f, "{RESTART_REQUIRED_ADVICE}"),
        }
    }
}

/// Who asked for a rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    /// A failed check or a tool call's corruption: counted, and stopped
    /// when the damage recurs.
    Damage,
    /// `rebuild_index`: always runs, subject only to the spacing.
    Requested,
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
            // A passing check reads the header, so the file is no longer
            // too damaged to write over.
            if state.restart_required {
                state.restart_required = false;
                tracing::info!(
                    check = ?kind,
                    "the index checks sound again; a restart is no longer needed"
                );
                publish_hint(&state);
            }
        }
        record
    }

    /// Whether automatic repair is stopped (recurring damage, or a file only
    /// a restart can replace).
    fn repair_stopped(&self) -> bool {
        repair_state(&self.lock()) != RepairState::Automatic
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

    /// One sweep: reconcile, log what failed, check, and rebuild if the
    /// check finds damage. Blocking; the loop runs it on the blocking pool.
    pub fn sweep(&self) {
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
        let had_errors = record.errors > 0 || record.failure.is_some();
        self.lock().last_sweep = Some(record);

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

    /// A tool call met a corruption error: run the thorough check now and
    /// rebuild if it confirms the damage.
    pub fn on_corruption_seen(&self) {
        // A client retrying in a loop sends a signal per call; one full
        // scan a minute answers them all.
        if self.recent_integrity().is_some() {
            if self.lock().outstanding_damage.is_some() && !self.repair_stopped() {
                self.rebuild_for_damage("a tool call met a damaged index");
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
            tracing::warn!(
                "a tool call reported index corruption, but the integrity check found none; if \
                 this repeats, another machine or container may be writing the index file \
                 (give each its own with `CUADERNO_INDEX_PATH` / `--index-path`)"
            );
            return;
        }
        tracing::warn!(
            problems = ?check.problems,
            "a tool call met a damaged index; rebuilding it from the notes"
        );
        self.rebuild_for_damage("a tool call met a damaged index");
    }

    /// An automatic rebuild, logged.
    fn rebuild_for_damage(&self, trigger: &str) {
        match self.run_rebuild(trigger, Trigger::Damage) {
            Ok(record) if record.rebuilt => tracing::warn!(
                notes = record.notes,
                errors = record.errors,
                index_path = %self.sqlite.path().display(),
                "the index was rebuilt; the first damaged file of a streak is kept beside it, \
                 with `.previous` appended to its name"
            ),
            Ok(record) if record.restart_required => tracing::error!(
                failure = record.failure.as_deref().unwrap_or(""),
                "rebuilding the index failed; the damaged index is still live, and only a \
                 restart can replace it"
            ),
            Ok(record) => tracing::error!(
                failure = record.failure.as_deref().unwrap_or(""),
                "rebuilding the index failed; the damaged index is still live"
            ),
            // Said once, when it was decided; a retrying client would
            // otherwise log a line per call.
            Err(RebuildRefused::DamageRecurring | RebuildRefused::RestartRequired) => {}
            Err(refused) => tracing::debug!(reason = %refused, "index rebuild skipped"),
        }
    }

    /// Rebuild the index now, as `rebuild_index` asks, unless another
    /// rebuild is running or the last finished less than the policy's
    /// spacing ago. It runs whatever the recurrence count says: the person
    /// asked. A rebuild that runs and fails is still `Ok`: its record says
    /// why, and the old index stays live. One that succeeds ends a stop;
    /// the count of earlier repairs stays, and at the limit it allows one
    /// further automatic repair before the next stop. Blocking.
    pub fn rebuild(&self, trigger: &str) -> Result<RebuildRecord, RebuildRefused> {
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
                if state.restart_required {
                    return Err(RebuildRefused::RestartRequired);
                }
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
            Ok(report) => {
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
                    scanned: report.notes,
                    errors: report.errors,
                    ..ReconciliationReport::default()
                };
                {
                    let mut state = self.lock();
                    state.last_sweep = Some(sweep_record(&errors, None));
                    state.outstanding_damage = None;
                    // A thorough check of the replaced file says nothing
                    // about the new one.
                    state.last_integrity = None;
                    state.restart_required = false;
                    match who {
                        // Only a repair that copied in counts towards the
                        // recurrence limit.
                        Trigger::Damage => state.damage_rebuilds.push_back(Instant::now()),
                        // The person's rebuild ends a stop, but keeps the
                        // history: at the limit it buys one more automatic
                        // repair, not a fresh count.
                        Trigger::Requested => {
                            state.recurring = None;
                            state.grace =
                                state.damage_rebuilds.len() >= self.policy.recurrence_limit;
                        }
                    }
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
                    restart_required: false,
                }
            }
            Err(e) => RebuildRecord {
                at: now(),
                trigger: trigger.to_string(),
                rebuilt: false,
                notes: 0,
                errors: 0,
                snapshots_carried: 0,
                snapshots_lost: Vec::new(),
                failure: Some(e.to_string()),
                restart_required: e.is_corruption(),
            },
        };
        let mut state = self.lock();
        if record.restart_required {
            state.restart_required = true;
        }
        state.last_rebuild = Some(record.clone());
        state.last_rebuild_at = Some(Instant::now());
        publish_hint(&state);
        Ok(record)
    }

    /// The status `index_status` reports: a fresh `quick_check` plus the
    /// latest records. While damage only `integrity_check` sees is
    /// outstanding, a quick pass cannot report healthy over it, so the
    /// thorough check is reported instead: the one from the last minute
    /// when there is one, else one run now, which reads the whole file and
    /// holds the index while it does. Blocking.
    pub fn status(&self) -> IndexStatus {
        let outstanding = self.lock().outstanding_damage;
        let check = match outstanding {
            Some(CheckKind::IntegrityCheck) => self
                .recent_integrity()
                .unwrap_or_else(|| self.check(CheckKind::IntegrityCheck)),
            _ => self.check(CheckKind::QuickCheck),
        };
        let state = self.lock();
        let repair = repair_state(&state);
        IndexStatus {
            healthy: check.ok && state.outstanding_damage.is_none() && state.recurring.is_none(),
            repair,
            advice: match repair {
                RepairState::Automatic => None,
                RepairState::DamageRecurring => Some(DAMAGE_RECURRING_ADVICE.to_string()),
                RepairState::RestartRequired => Some(RESTART_REQUIRED_ADVICE.to_string()),
            },
            damage_rebuilds_recent: state
                .damage_rebuilds
                .iter()
                .filter(|at| at.elapsed() < self.policy.recurrence_window)
                .count(),
            index_path: self.sqlite.path().display().to_string(),
            restart_required: state.restart_required,
            notes_not_indexed: state.last_sweep.as_ref().map_or(0, |s| s.errors),
            check,
            last_sweep: state.last_sweep.clone(),
            last_rebuild: state.last_rebuild.clone(),
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
            let mut interval = every.map(|every| {
                let first = tokio::time::Instant::now() + first_sweep_delay(every, random_u64());
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
    let spread = (every - half).as_millis() as u64;
    if spread == 0 {
        return every;
    }
    half + Duration::from_millis(random % spread)
}

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
    if state.restart_required {
        RepairState::RestartRequired
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
        RepairState::RestartRequired => 2,
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
