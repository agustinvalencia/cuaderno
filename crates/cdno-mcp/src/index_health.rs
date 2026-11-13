//! Index health for a long-lived server: assess, repair, report (#526).
//!
//! The index is a cache of the notes, so it is the one thing in the vault
//! that may be repaired automatically: rebuilding it from the markdown
//! loses nothing. This module gives a running server that authority.
//! Before it, the periodic reconciliation could only log a count of the
//! notes it failed to index, and a damaged database stayed damaged until
//! somebody with a shell on the host deleted it.
//!
//! - **Assess.** Every sweep reconciles, then runs `PRAGMA quick_check`.
//!   Page-level damage can leave every read working while writes to the
//!   damaged pages fail, so the check, not a query error, is the signal
//!   that can be relied on. A tool call that meets a corruption error
//!   wakes the loop at once ([`corruption_seen`]) for the thorough
//!   `integrity_check`.
//! - **Repair.** A failed check rebuilds the index into a fresh file and
//!   swaps it in behind the handle every tool call shares
//!   ([`SqliteIndex::rebuild_with`]), with no restart. Rebuilds are
//!   serialised and spaced at least [`MIN_REBUILD_INTERVAL`] apart, so a
//!   file that keeps getting damaged costs one rebuild per interval, not
//!   a spin.
//! - **Report.** Failing notes are logged with their reasons, and the
//!   latest sweep, check and rebuild are kept for `index_status`.
//!
//! What it never does: touch a note. Automatic repair stops at derived
//! state; the notes are the source of truth, and `lint` reports on them
//! without changing them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cdno_core::classify::NoteScope;
use cdno_core::index::{SqliteIndex, VaultIndex};
use cdno_core::reconcile::{ReconciliationReport, rebuild, reconcile};
use cdno_core::store::VaultStore;
use schemars::JsonSchema;
use serde::Serialize;
use tokio::sync::Notify;

/// The shortest time between two rebuilds, automatic or asked for.
pub const MIN_REBUILD_INTERVAL: Duration = Duration::from_secs(60);

/// How many failing notes a sweep names, in the log and in the record.
/// The first few show what is wrong; all of them would bury it.
pub const FAILING_NOTES_SHOWN: usize = 5;

/// Woken when a tool call meets a corruption error. Process-wide because
/// the errors are converted in free functions with no server at hand
/// (`util::into_mcp_error`), and one process serves one vault. A wake
/// with no loop listening is kept as a permit, so the next loop to wait
/// still sees it.
static CORRUPTION_SEEN: Notify = Notify::const_new();

/// Tell the health loop that a tool call met a damaged index, so it
/// checks now rather than at the next sweep.
pub(crate) fn corruption_seen() {
    CORRUPTION_SEEN.notify_one();
}

/// The index's health supervisor for one server process.
pub struct IndexHealth {
    store: Arc<dyn VaultStore>,
    index: Arc<dyn VaultIndex>,
    sqlite: Arc<SqliteIndex>,
    scope: Arc<NoteScope>,
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
    pub at: String,
    /// `quick_check` (every sweep) or `integrity_check` (thorough: also
    /// compares each index with its table).
    pub kind: CheckKind,
    /// True when the check found nothing wrong.
    pub ok: bool,
    /// What it found, at most twenty lines; empty when `ok`.
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    QuickCheck,
    IntegrityCheck,
}

/// One rebuild of the index.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RebuildRecord {
    pub at: String,
    /// Why it ran: a failed sweep check, a corruption a tool call met, or
    /// a `rebuild_index` call.
    pub trigger: String,
    /// True when the fresh index was swapped in.
    pub rebuilt: bool,
    /// Notes the rebuilt index holds.
    pub notes: usize,
    /// Notes the rebuild could not index (see the next sweep for which).
    pub errors: usize,
    /// Archival snapshots carried over from the old index (the record
    /// `lint` checks archived action notes against; not in the notes).
    pub snapshots_carried: usize,
    /// Snapshot rows the old index's damage kept from being carried.
    /// Each costs only `lint`'s frozen-prefix check on that note.
    pub snapshots_lost: Vec<String>,
    /// Why the rebuild failed, when it did. The old index is then still
    /// the live one.
    pub failure: Option<String>,
}

/// What `index_status` returns.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct IndexStatus {
    /// The verdict: true when the check run for this call found nothing
    /// and the latest sweep indexed every note.
    pub healthy: bool,
    /// The check run for this call.
    pub check: CheckRecord,
    pub last_sweep: Option<SweepRecord>,
    pub last_rebuild: Option<RebuildRecord>,
    /// What the server found wrong with the index when it started, before
    /// it set the file aside and rebuilt it; null when nothing was.
    pub damage_at_open: Option<Vec<String>>,
}

/// Why a rebuild did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildRefused {
    /// Another rebuild is running now.
    InProgress,
    /// The last one finished too recently; try again after this long.
    TooSoon(Duration),
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
        }
    }
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
            state: Mutex::new(State::default()),
            rebuilding: AtomicBool::new(false),
        }
    }

    /// Build from what [`open_vault_checked`](cdno_domain::bootstrap::open_vault_checked)
    /// returned, keeping what it found at open.
    pub fn from_opened(opened: &cdno_domain::bootstrap::OpenedVault) -> Self {
        let health = Self::new(
            opened.store.clone(),
            opened.index.clone(),
            opened.sqlite.clone(),
            opened.scope.clone(),
        );
        health.lock().damage_at_open = opened.damage_at_open.clone();
        health
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
        self.lock().last_check = Some(record.clone());
        record
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
        self.lock().last_sweep = Some(record);

        let check = self.check(CheckKind::QuickCheck);
        if !check.ok {
            tracing::warn!(
                problems = ?check.problems,
                "the index failed its integrity check; rebuilding it from the notes"
            );
            self.rebuild_logged("the sweep's integrity check failed");
        }
    }

    /// A tool call met a corruption error: run the thorough check now and
    /// rebuild if it confirms the damage.
    pub fn on_corruption_seen(&self) {
        let check = self.check(CheckKind::IntegrityCheck);
        if check.ok {
            tracing::info!(
                "a tool call reported index corruption, but the integrity check found none"
            );
            return;
        }
        tracing::warn!(
            problems = ?check.problems,
            "a tool call met a damaged index; rebuilding it from the notes"
        );
        self.rebuild_logged("a tool call met a damaged index");
    }

    fn rebuild_logged(&self, trigger: &str) {
        match self.rebuild(trigger) {
            Ok(record) if record.rebuilt => tracing::warn!(
                notes = record.notes,
                errors = record.errors,
                "the index was rebuilt; the damaged file is kept as index.db.previous"
            ),
            Ok(record) => tracing::error!(
                failure = record.failure.as_deref().unwrap_or(""),
                "rebuilding the index failed; the damaged index is still live"
            ),
            Err(refused) => tracing::info!(reason = %refused, "index rebuild skipped"),
        }
    }

    /// Rebuild the index now, unless another rebuild is running or the
    /// last finished less than [`MIN_REBUILD_INTERVAL`] ago. A rebuild
    /// that runs and fails is still `Ok`: its record says why, and the old
    /// index stays live. Blocking.
    pub fn rebuild(&self, trigger: &str) -> Result<RebuildRecord, RebuildRefused> {
        if self.rebuilding.swap(true, Ordering::AcqRel) {
            return Err(RebuildRefused::InProgress);
        }
        let _running = ClearOnDrop(&self.rebuilding);
        if let Some(at) = self.lock().last_rebuild_at {
            let since = at.elapsed();
            if since < MIN_REBUILD_INTERVAL {
                return Err(RebuildRefused::TooSoon(MIN_REBUILD_INTERVAL - since));
            }
        }

        let record = match rebuild(&self.store, &self.sqlite, &self.scope) {
            Ok(report) => {
                if !report.snapshots_lost.is_empty() {
                    tracing::warn!(
                        lost = ?report.snapshots_lost,
                        "some archival snapshots could not be carried into the rebuilt index; \
                         lint can no longer check those archived notes' frozen prefix"
                    );
                }
                RebuildRecord {
                    at: now(),
                    trigger: trigger.to_string(),
                    rebuilt: true,
                    notes: report.reconciliation.added,
                    errors: report.reconciliation.errors.len(),
                    snapshots_carried: report.snapshots_carried,
                    snapshots_lost: report.snapshots_lost,
                    failure: None,
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
            },
        };
        let mut state = self.lock();
        state.last_rebuild = Some(record.clone());
        state.last_rebuild_at = Some(Instant::now());
        Ok(record)
    }

    /// The status `index_status` reports: a fresh `quick_check` plus the
    /// latest records. Blocking.
    pub fn status(&self) -> IndexStatus {
        let check = self.check(CheckKind::QuickCheck);
        let state = self.lock();
        let sweep_clean = state
            .last_sweep
            .as_ref()
            .is_none_or(|s| s.errors == 0 && s.failure.is_none());
        IndexStatus {
            healthy: check.ok && sweep_clean,
            check,
            last_sweep: state.last_sweep.clone(),
            last_rebuild: state.last_rebuild.clone(),
            damage_at_open: state.damage_at_open.clone(),
        }
    }

    /// Run [`sweep`](Self::sweep) every `every`, and
    /// [`on_corruption_seen`](Self::on_corruption_seen) whenever a tool
    /// call reports corruption.
    ///
    /// Known benign race (PR #304 review, correctness finding 2): a sweep
    /// reads file state *before* taking the per-note write lock, so a
    /// tool-call write landing in that window can be shadowed by a stale
    /// index row, which the *next* sweep heals (mtime mismatch). Worst
    /// case is stale search results for one interval; no note is touched.
    pub fn spawn_loop(self: &Arc<Self>, every: Duration) {
        let health = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(every);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // The first tick fires immediately; skip it, since opening the
            // vault has just reconciled and checked.
            interval.tick().await;
            loop {
                let corruption = tokio::select! {
                    _ = interval.tick() => false,
                    _ = CORRUPTION_SEEN.notified() => true,
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
        });
    }
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
