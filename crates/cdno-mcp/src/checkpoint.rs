//! Git-checkpoint recoverability loop for `cdno-mcp-server` (GH #303).
//!
//! Every mutation a prompt-injected or buggy session could make is
//! captured as a git commit, so it is diffable and revertible — the
//! only meaningful damage limit once write tools are exposed. It is a
//! commit-if-dirty *sweep*, not a per-tool hook: zero changes to the
//! tool handlers, and out-of-band edits (CLI, editors, sync) join the
//! audit trail too. Per-write attribution already lives in-content —
//! every cdno write logs a line to the daily note.
//!
//! # Correctness (PR #306 review)
//!
//! Three hazards, handled separately (PR #306 review, F1; GH #546):
//!
//! - **In-flight temp files.** Atomic writes stage a temp sibling
//!   ([`cdno_core::store::WIP_TEMP_PREFIX`]) next to their target.
//!   These must never enter history. Handled *deterministically*, not
//!   by locking: the loop adds the prefix to `.git/info/exclude` once
//!   at startup, so `git status`/`add -A` ignore wip files regardless
//!   of any lock or platform.
//! - **A git operation paused mid-way by someone else** (GH #546; scope
//!   widened past merge alone in panel review of the first version). A
//!   tree dirty because a merge, cherry-pick, revert, `git am`, or
//!   rebase is unresolved is not a tree the sweep may act on: `git add
//!   -A` stages unmerged (`UU`) paths without complaint, and `git
//!   commit` while `.git/MERGE_HEAD` (or the sibling markers below)
//!   exists does not make an ordinary commit — it *concludes* that
//!   operation, using the sweep's generic message in place of whatever
//!   the real actor intended, and can embed raw `<<<<<<<` conflict
//!   markers as note content, served to clients as if it were real
//!   content. A concluded cherry-pick/revert/rebase also leaves the
//!   real actor's next `--continue` looking at git state it no longer
//!   recognises (`git cherry-pick --continue` failing with "no
//!   cherry-pick or revert in progress", a rebase left on a detached
//!   HEAD). Reachable whenever another actor (an external sync agent,
//!   or an operator's own git use) touches the same working tree the
//!   sweep watches.
//!
//!   Checked before staging, on every tick, via [`git_operation_in_progress`]:
//!   `.git/MERGE_HEAD`, `.git/CHERRY_PICK_HEAD`, `.git/REVERT_HEAD`,
//!   `.git/rebase-merge`/`.git/rebase-apply` (the latter also covers
//!   `git am`, which reuses it), and — as a fallback for an operation
//!   with no head marker of its own, namely a conflicted `git stash
//!   pop` — `git diff --name-only --diff-filter=U`. Each operation's
//!   own marker is checked directly rather than inferred from `git
//!   status`, because "every conflict resolved and staged, but
//!   `--continue`/commit not yet run" has **no** unmerged paths and
//!   would otherwise be missed. (`git merge --squash` is deliberately
//!   not covered: it sets no marker because it *intends* the caller to
//!   make an ordinary commit.)
//!
//!   Any hit is treated as transient (retry next tick, does not count
//!   toward [`MAX_CONSECUTIVE_FAILURES`]) in both [`CheckpointMode::Commit`]
//!   and [`CheckpointMode::NudgeOnly`]: the operation belongs to whoever
//!   started it. The first tick that finds one logs at `warn`; later
//!   ticks of the same still-unresolved state log at `debug` — **except**
//!   every [`RE_WARN_INTERVAL_SECS`], when it warns again. A once-only
//!   warning would decay into silence for a *stale* marker left behind
//!   by a crashed process, in a vault that otherwise keeps churning:
//!   before this guard existed such a tree would (wrongly) get
//!   committed; after it, a bare once-per-episode warning would mean
//!   the recovery trail stops **silently** and forever, which is worse.
//!   The periodic re-warn keeps a stuck state visible without spamming
//!   the ordinary case of a conflict a human resolves within a tick or
//!   two.
//! - **Half-applied multi-file transactions.** A transaction applies
//!   its ops one atomic rename at a time while holding the vault write
//!   lock. The checkpoint takes the **same lock** around
//!   `status`+`add`+`commit`, which serialises it against a
//!   *different-process* writer (the host `cdno` CLI) — the realistic
//!   concurrent writer. Rust's file lock is per-process on Linux, so
//!   it does **not** serialise the checkpoint against the server's
//!   *own* tool-call transactions in the same process; that residual
//!   window is bounded and self-correcting (transactions are
//!   sub-millisecond, checkpoints are seconds apart, and any partial
//!   snapshot is superseded by a complete one at the next tick).
//!   Tightening it fully would need an in-process write gate in
//!   cdno-core — deferred as it is not load-bearing for a
//!   single-operator server.
//!
//! A single git failure must **not** permanently disable the audit
//! trail (F2): non-zero `git` exits (`.git/index.lock` contention with
//! the operator's own git use is routine) and lock-acquisition
//! timeouts are transient — the loop logs and continues. Only a git
//! binary that cannot be executed at all, repeated `MAX_CONSECUTIVE`
//! times, stops the loop.
//!
//! # Stall watchdog (GH #548)
//!
//! Every outcome above describes a tick that **ran**. A tick that never
//! runs — `spawn_blocking` unable to obtain a thread on a process at
//! the host's thread/PID limit is the observed way in — produces no
//! outcome at all: the loop waits on the join handle, the interval
//! never fires again, and the recovery trail stops in total silence.
//! Nothing else in this server exposes that: the HTTP endpoint stays
//! up and tool calls keep answering, so no external probe can tell.
//! [`await_tick`] therefore reports at `error` when a tick overruns
//! [`stall_timeout`], and keeps saying so until it completes.
//!
//! # Modes (GH #541)
//!
//! Three, and the third is the only one that is not just an interval:
//!
//! | mode | who commits | how it is selected |
//! |---|---|---|
//! | interval | this server | the default; `--git-checkpoint-interval-secs N` |
//! | disabled | nobody | `--git-checkpoint-interval-secs 0` — [`spawn`] is never called |
//! | nudge-only | an external sync agent | [`CheckpointMode::NudgeOnly`] |
//!
//! What must survive all three is the #303 property: every mutation
//! ends up in a commit somebody can diff and revert. Only the *actor*
//! changes, so [`spawn`] logs which actor is expected — loudly, and at
//! `warn` for nudge-only, because there the trail depends on a process
//! this one cannot see.
//!
//! Only `cdno-mcp-server` spawns a sweep at all; the stdio binary has
//! none, so none of this configuration surface exists there.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cdno_core::store::{FsVaultStore, VaultStore};

use crate::nudge::SharedNudge;

/// Consecutive hard failures (git not executable) before giving up.
const MAX_CONSECUTIVE_FAILURES: u32 = 5;

/// How many sweep intervals a single tick may overrun before the loop
/// declares it stalled. Three, so a stall report always means at least
/// two ticks' worth of checkpoints have already been missed and it is
/// never one slow `git status` on a cold cache.
const STALL_MULTIPLIER: u32 = 3;

/// Floor under [`stall_timeout`]. A very short configured interval
/// (`--git-checkpoint-interval-secs 1`, common in tests and in
/// impatient deployments) must not turn an ordinary `git add`+`commit`
/// on a large tree into a stall report.
const MIN_STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// How long one tick may run before [`await_tick`] reports it stalled,
/// and how often it repeats that report while the tick stays stuck.
fn stall_timeout(every: Duration) -> Duration {
    std::cmp::max(every.saturating_mul(STALL_MULTIPLIER), MIN_STALL_TIMEOUT)
}

/// What the sweep does when it finds the tree dirty (GH #541).
///
/// The disabled mode is not a variant: it is the absence of a sweep,
/// expressed at the call site by not calling [`spawn`] at all.
///
/// Whichever mode is chosen, the #303 property must hold — every
/// mutation ends up in a commit that can be diffed and reverted. The
/// modes differ only in **which actor** makes that commit, which is why
/// [`spawn`] says so in the startup log rather than leaving an operator
/// to infer it from a flag.
#[derive(Clone)]
pub enum CheckpointMode {
    /// This server commits: `add -A` + `commit` on every dirty sweep.
    /// The out-of-the-box behaviour, and self-sufficient — no second
    /// actor need exist.
    Commit,
    /// This server commits **nothing** and instead touches the
    /// sync-nudge sentinel (GH #540) whenever the tree is dirty, so an
    /// external sync agent commits promptly.
    ///
    /// For deployments where an agent already owns the repository's
    /// history: per-minute checkpoint commits would fight it, producing
    /// two git actors in one working tree and burying the agent's
    /// coalesced, unit-of-thought commits under machine noise. The cost
    /// is that the recovery trail now depends on that agent actually
    /// running — hence the startup warning.
    NudgeOnly(SharedNudge),
}

/// Outcome of one sweep, distinguishing transient trouble (keep
/// looping) from a hard, likely-permanent fault (count toward giving
/// up).
enum Pass {
    /// A commit was made (summary) or the tree was clean (`None`).
    Ok(Option<String>),
    /// Transient: git exited non-zero (e.g. `index.lock` contention)
    /// or the vault write lock timed out. Retry next tick.
    Transient(String),
    /// Hard: `git` could not be executed at all.
    Fatal(anyhow::Error),
}

/// Spawn the periodic checkpoint loop. No-op (with a warning) when the
/// vault is not a real git repository — a `.git` *file* (worktree /
/// submodule pointer) is refused too, since `git -C` would then commit
/// an external repo (PR #306 security review, finding 3).
pub fn spawn(root: PathBuf, every: Duration, mode: CheckpointMode) {
    match std::fs::symlink_metadata(root.join(".git")) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            tracing::warn!(
                vault_root = %root.display(),
                "`.git` is not a directory (gitfile/worktree pointer) — checkpoints \
                 disabled to avoid committing an external repository"
            );
            return;
        }
        Err(_) => {
            tracing::warn!(
                vault_root = %root.display(),
                "vault is not a git repository — checkpoints disabled; \
                 remote writes will have NO commit-level recovery trail"
            );
            return;
        }
    }

    // Make in-flight atomic-write temp files invisible to git, once,
    // before the first tick — so no sweep can ever stage one.
    if let Err(e) = ensure_wip_excluded(&root) {
        tracing::warn!(error = %e, "could not write .git/info/exclude; wip temp files may be committed");
    }

    // Say plainly WHO is expected to commit. The #303 recovery trail is
    // only as real as the actor that writes it, and in nudge-only mode
    // that actor is not this process — an operator reading the log
    // must not have to infer that from a flag name.
    match &mode {
        CheckpointMode::Commit => tracing::info!(
            every_secs = every.as_secs(),
            "git checkpoint sweep: THIS SERVER commits the recovery trail (mode=commit)"
        ),
        CheckpointMode::NudgeOnly(nudge) => tracing::warn!(
            every_secs = every.as_secs(),
            sentinel = %nudge.path().display(),
            "git checkpoint sweep: mode=nudge-only — this server commits NOTHING. An EXTERNAL \
             sync agent must watch the sentinel and commit; if none is running, remote writes \
             have no commit-level recovery trail"
        ),
    }

    let stall = stall_timeout(every);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut consecutive_failures: u32 = 0;
        // Shared across ticks (not reset per-call): holds the unix
        // timestamp of the last paused-git-operation warning, `0` when
        // none is outstanding, so `warn_paused_op` can log once per
        // episode plus a periodic re-warn rather than once per tick —
        // see `warn_paused_op`.
        let paused_op_warned = Arc::new(AtomicU64::new(0));
        loop {
            interval.tick().await;
            let repo = root.clone();
            let mode = mode.clone();
            let paused_op_warned = paused_op_warned.clone();
            let tick = tokio::task::spawn_blocking(move || {
                checkpoint_once(&repo, &mode, &paused_op_warned)
            });
            let pass = await_tick(tick, stall).await;
            match pass {
                Ok(Pass::Ok(Some(summary))) => {
                    consecutive_failures = 0;
                    tracing::info!(%summary, "git checkpoint sweep acted");
                }
                Ok(Pass::Ok(None)) => {
                    consecutive_failures = 0;
                    tracing::debug!("git checkpoint: vault clean");
                }
                Ok(Pass::Transient(reason)) => {
                    // Do NOT count toward giving up — this self-heals.
                    tracing::debug!(%reason, "git checkpoint skipped this tick (transient)");
                }
                Ok(Pass::Fatal(e)) => {
                    consecutive_failures += 1;
                    tracing::warn!(
                        error = %e,
                        consecutive = consecutive_failures,
                        "git checkpoint hard failure"
                    );
                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        tracing::error!(
                            "git checkpoint disabled after {MAX_CONSECUTIVE_FAILURES} consecutive \
                             failures — remote writes now have no commit-level recovery trail"
                        );
                        return;
                    }
                }
                Err(e) => tracing::warn!(error = %e, "git checkpoint task panicked"),
            }
        }
    });
}

/// Await one sweep tick, shouting if it does not finish (GH #548).
///
/// The loop's outcomes — commit, clean, transient, fatal — all describe
/// a tick that **ran**. A tick that never runs produces none of them:
/// [`spawn`]'s loop simply waits on the join handle forever, the
/// interval never ticks again, and the recovery trail stops without a
/// single line of log. The realistic way to get there is
/// `spawn_blocking` unable to obtain a thread, on a process at the
/// host's thread/PID limit.
///
/// That silence is the whole problem: the sweep is the one part of this
/// server whose failure is otherwise invisible from outside, because the
/// HTTP endpoint stays up and every tool call still answers, so no
/// external probe can tell the difference.
///
/// Two deliberate non-behaviours:
///
/// - **The tick is never cancelled or superseded.** Starting a second
///   sweep while the first is stuck would put two actors on the vault
///   write lock and, in the thread-exhaustion case, consume another
///   blocking thread that is already unobtainable. One tick in flight,
///   loudly reported, is the honest state.
/// - **No cause is claimed.** The message names thread exhaustion as
///   the *suspicion* and a wedged `git` as the alternative, and says
///   this process cannot distinguish them. What it does assert is the
///   consequence, which it does know: nothing here is committing.
///
/// Repeats every `stall` while the tick stays stuck, so any log window
/// shows the problem rather than only the window containing the first
/// report; a stall that resolves logs the recovery.
async fn await_tick(
    mut tick: tokio::task::JoinHandle<Pass>,
    stall: Duration,
) -> Result<Pass, tokio::task::JoinError> {
    let started = Instant::now();
    let mut reported = false;
    loop {
        // `&mut` so a timeout leaves the handle intact: the tick keeps
        // running and is awaited again on the next pass.
        match tokio::time::timeout(stall, &mut tick).await {
            Ok(result) => {
                if reported {
                    tracing::warn!(
                        stalled_secs = started.elapsed().as_secs(),
                        "git checkpoint sweep recovered: the stalled tick completed and \
                         checkpoints are running again"
                    );
                }
                return result;
            }
            Err(_elapsed) => {
                reported = true;
                tracing::error!(
                    stalled_secs = started.elapsed().as_secs(),
                    threshold_secs = stall.as_secs(),
                    "git checkpoint sweep STALLED: a tick has not completed, no further tick can \
                     start, and so NOTHING in this process is committing — writes are no longer \
                     being recorded. The sweep runs on tokio's blocking pool, so the likeliest \
                     cause is that no blocking thread is available (thread/PID exhaustion); a \
                     wedged `git` invocation looks identical from here and this process cannot \
                     tell the two apart. Check this process's OS thread count against the host's \
                     limit."
                );
            }
        }
    }
}

/// One sweep under the vault write lock. `pub(crate)`-visible via the
/// `Pass` classification so the loop can distinguish transient from
/// fatal; the lock guarantees no half-applied transaction or temp
/// sibling is committed.
fn checkpoint_once(root: &Path, mode: &CheckpointMode, paused_op_warned: &AtomicU64) -> Pass {
    // Serialise against all vault writers (this process's transactions
    // and any cross-process cdno CLI) via the same flock they take.
    let store = FsVaultStore::new(root);
    let _lock = match store.acquire_write_lock() {
        Ok(guard) => guard,
        // Lock contention is transient by construction: a writer holds
        // it briefly. Skip this tick; the next one will get it.
        Err(e) => return Pass::Transient(format!("vault write lock: {e}")),
    };

    let outcome = match mode {
        CheckpointMode::Commit => git_commit_if_dirty(root, paused_op_warned),
        CheckpointMode::NudgeOnly(nudge) => nudge_if_dirty(root, nudge, paused_op_warned),
    };
    match outcome {
        Ok(summary) => Pass::Ok(summary),
        Err(CheckpointError::GitExit(msg)) => Pass::Transient(msg),
        Err(CheckpointError::Exec(e)) => Pass::Fatal(e),
    }
}

enum CheckpointError {
    /// `git` ran but exited non-zero (transient: lock contention, …).
    GitExit(String),
    /// `git` could not be executed (binary absent → fatal).
    Exec(anyhow::Error),
}

/// Commit everything if the tree is dirty. `Ok(None)` when clean.
/// Assumes the caller holds the vault write lock.
fn git_commit_if_dirty(
    root: &Path,
    paused_op_warned: &AtomicU64,
) -> Result<Option<String>, CheckpointError> {
    let status = run_git(root, &["status", "--porcelain"])?;
    if !status.status.success() {
        return Err(CheckpointError::GitExit(format!(
            "git status: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        )));
    }
    if status.stdout.is_empty() {
        // Clean: any paused-operation episode has ended (concluded by
        // someone else, or aborted) — rearm the warning.
        paused_op_warned.store(0, Ordering::Relaxed);
        return Ok(None);
    }

    // GH #546: a dirty tree caused by someone else's paused git
    // operation must never be staged or committed here — see the
    // module doc.
    if let Some(op) = git_operation_in_progress(root)? {
        warn_paused_op(paused_op_warned, &op);
        return Err(CheckpointError::GitExit(format!(
            "skipping sweep, {}",
            op.reason
        )));
    }
    paused_op_warned.store(0, Ordering::Relaxed);

    let dirty_paths = status.stdout.iter().filter(|&&b| b == b'\n').count();

    let add = run_git(root, &["add", "-A"])?;
    if !add.status.success() {
        return Err(CheckpointError::GitExit(format!(
            "git add: {}",
            String::from_utf8_lossy(&add.stderr).trim()
        )));
    }

    let message = format!("cdno-mcp checkpoint ({dirty_paths} path(s))");
    let commit = run_git(
        root,
        &[
            "-c",
            "user.name=cdno-mcp",
            "-c",
            "user.email=cdno-mcp@localhost",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            &message,
        ],
    )?;
    if !commit.status.success() {
        return Err(CheckpointError::GitExit(format!(
            "git commit: {}",
            String::from_utf8_lossy(&commit.stderr).trim()
        )));
    }
    Ok(Some(message))
}

/// Nudge-only sweep (GH #541): look, never touch the history.
///
/// Reads `git status --porcelain` exactly as the committing sweep does
/// — including the `.git/info/exclude` rule, so an in-flight atomic-write
/// temp sibling never counts as dirty and cannot produce a spurious
/// nudge — and touches the sentinel when anything is dirty. `Ok(None)`
/// when clean.
///
/// Deliberately unconditional on *what* is dirty: coalescing (does this
/// change deserve a commit yet?) is the external agent's judgement, and
/// duplicating it here would put the decision in two places. The
/// sentinel is a hint, and a redundant hint costs the agent one wakeup.
///
/// Assumes the caller holds the vault write lock, so the tree it
/// observes is not mid-transaction.
///
/// Also skips while a git operation is paused (GH #546), same as the
/// committing sweep. Nudging does not write history itself, but the
/// external agent it wakes typically does — and this sweep has no
/// evidence either way about whether that agent's own commit path
/// guards against concluding someone else's paused
/// merge/cherry-pick/revert/rebase, so it is not a safe assumption to
/// make on the agent's behalf. Skipping costs nothing here: the tree
/// stays dirty and is nudged normally on the next tick once the
/// operation ends or is aborted, so the only effect is that the
/// "nothing committed here" framing of a nudge is never sent while the
/// tree is genuinely unsafe to act on.
fn nudge_if_dirty(
    root: &Path,
    nudge: &SharedNudge,
    paused_op_warned: &AtomicU64,
) -> Result<Option<String>, CheckpointError> {
    let status = run_git(root, &["status", "--porcelain"])?;
    if !status.status.success() {
        return Err(CheckpointError::GitExit(format!(
            "git status: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        )));
    }
    if status.stdout.is_empty() {
        paused_op_warned.store(0, Ordering::Relaxed);
        return Ok(None);
    }

    if let Some(op) = git_operation_in_progress(root)? {
        warn_paused_op(paused_op_warned, &op);
        return Err(CheckpointError::GitExit(format!(
            "not nudging, {}",
            op.reason
        )));
    }
    paused_op_warned.store(0, Ordering::Relaxed);

    let dirty_paths = status.stdout.iter().filter(|&&b| b == b'\n').count();
    nudge.touch();
    Ok(Some(format!(
        "nudged the sync agent ({dirty_paths} dirty path(s), nothing committed here)"
    )))
}

/// A paused git operation found on disk: a human-readable description
/// for the log, and — when the operation has a single marker file or
/// directory an operator could remove to force it away — that marker's
/// `.git`-relative path, so the warning can name exactly what to remove
/// rather than gesturing at "some git state".
struct PausedGitOp {
    reason: String,
    marker: Option<&'static str>,
}

/// Detect any git operation another actor left paused mid-way (GH #546;
/// scope widened past merge alone in panel review — see the module
/// doc). Checked in order, each entry catching a state the others miss:
///
/// - `.git/MERGE_HEAD`, `.git/CHERRY_PICK_HEAD`, `.git/REVERT_HEAD` —
///   a merge, cherry-pick, or revert stopped short of a commit. Checked
///   as marker files directly, **not** inferred from unmerged paths:
///   "every conflict resolved and staged by hand, `--continue`/commit
///   not yet run" leaves zero unmerged paths and would otherwise slip
///   through.
/// - `.git/rebase-merge`, `.git/rebase-apply` (directories) — a rebase
///   in progress; `git am` reuses `rebase-apply` and is covered by the
///   same check.
/// - `git diff --name-only --diff-filter=U`, as a fallback for an
///   operation with no head marker of its own — a conflicted `git
///   stash pop` is the practical example. Asking git directly here is
///   less error-prone than re-deriving the unmerged porcelain codes
///   (`DD AU UD UA DU AA UU`) by hand.
///
/// `git merge --squash` is deliberately not covered: it sets no marker
/// because it *intends* the caller to make an ordinary commit, so there
/// is nothing here to protect it from.
///
/// `Ok(None)` means no paused operation was found. Assumes `root/.git`
/// is a directory, which [`spawn`] already guarantees before any sweep
/// runs.
fn git_operation_in_progress(root: &Path) -> Result<Option<PausedGitOp>, CheckpointError> {
    let git_dir = root.join(".git");

    for (marker, label) in [
        ("MERGE_HEAD", "a merge"),
        ("CHERRY_PICK_HEAD", "a cherry-pick"),
        ("REVERT_HEAD", "a revert"),
    ] {
        if git_dir.join(marker).exists() {
            return Ok(Some(PausedGitOp {
                reason: format!("{label} is in progress (.git/{marker} present)"),
                marker: Some(marker),
            }));
        }
    }
    for marker in ["rebase-merge", "rebase-apply"] {
        if git_dir.join(marker).is_dir() {
            return Ok(Some(PausedGitOp {
                reason: format!("a rebase is in progress (.git/{marker} present)"),
                marker: Some(marker),
            }));
        }
    }

    let unmerged = run_git(root, &["diff", "--name-only", "--diff-filter=U"])?;
    if !unmerged.status.success() {
        return Err(CheckpointError::GitExit(format!(
            "git diff --diff-filter=U: {}",
            String::from_utf8_lossy(&unmerged.stderr).trim()
        )));
    }
    if unmerged.stdout.is_empty() {
        return Ok(None);
    }
    let n = unmerged.stdout.iter().filter(|&&b| b == b'\n').count();
    Ok(Some(PausedGitOp {
        reason: format!(
            "{n} unmerged path(s) present with no operation marker (e.g. a conflicted `git stash pop`)"
        ),
        marker: None,
    }))
}

/// How long to wait before repeating the paused-operation warning while
/// the same state persists (panel review of the first version of
/// GH #546's fix). Warning only once per episode meant a *stale* marker
/// — left behind by a crashed process, in a vault that otherwise keeps
/// churning — got exactly one warning ever and then silently stopped
/// recording forever: worse than the bug the guard fixes, since before
/// it existed that tree would at least (wrongly) get committed. 15
/// minutes: long enough that an ordinary conflict, resolved by a human
/// within a tick or two, never sees a second warning; short enough that
/// a marker still present after it is unusual and worth saying again
/// rather than decaying into a `debug` line nobody sees by default.
const RE_WARN_INTERVAL_SECS: u64 = 15 * 60;

fn epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Pure decision, kept separate from the clock so it is unit-testable
/// without sleeping: `0` means "not warned yet this episode" (fresh, or
/// just rearmed by a clean tree) and always warns.
fn due_for_rewarn(last_warned_secs: u64, now_secs: u64) -> bool {
    last_warned_secs == 0 || now_secs.saturating_sub(last_warned_secs) >= RE_WARN_INTERVAL_SECS
}

/// Log a paused-operation finding: `warn` the first time in an episode
/// and again every [`RE_WARN_INTERVAL_SECS`] while it persists, `debug`
/// on every tick in between. `last_warned` is rearmed to `0` by the
/// caller the instant the tree is next seen clean or operation-free, so
/// a *new* episode always warns immediately.
fn warn_paused_op(last_warned: &AtomicU64, op: &PausedGitOp) {
    let now = epoch_secs();
    if due_for_rewarn(last_warned.load(Ordering::Relaxed), now) {
        last_warned.store(now, Ordering::Relaxed);
        let guidance = match op.marker {
            Some(marker) => format!(
                "if this is stale (e.g. left behind by a crashed process), remove .git/{marker} \
                 to let checkpoints resume"
            ),
            None => {
                "resolve and stage the conflicted path(s) to let checkpoints resume".to_string()
            }
        };
        tracing::warn!(
            reason = %op.reason,
            guidance = %guidance,
            "git checkpoint sweep: paused git operation found — skipping this tick, nothing is \
             being committed while this persists"
        );
    } else {
        tracing::debug!(
            reason = %op.reason,
            "git checkpoint sweep: paused git operation still present — skipping (recently warned)"
        );
    }
}

/// Idempotently add the atomic-write temp-file prefix to the repo's
/// local `.git/info/exclude` (not the tracked `.gitignore`, so the
/// user's ignore file is untouched). Ensures `git add -A` never stages
/// an in-flight temp sibling.
fn ensure_wip_excluded(root: &Path) -> Result<()> {
    use std::io::Write;
    let pattern = format!("{}*", cdno_core::store::WIP_TEMP_PREFIX);
    let info = root.join(".git").join("info");
    std::fs::create_dir_all(&info).context("creating .git/info")?;
    let exclude = info.join("exclude");
    let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    // Match at any depth: bare pattern (git applies it per directory).
    if existing.lines().any(|l| l.trim() == pattern) {
        return Ok(());
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude)
        .context("opening .git/info/exclude")?;
    writeln!(f, "{pattern}").context("appending to .git/info/exclude")?;
    Ok(())
}

/// Run `git -C root <args>` with a scrubbed environment.
///
/// `env_clear` (PR #306 security review, finding 3) stops an ambient
/// `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`/`GIT_CONFIG_*` from
/// redirecting the checkpoint away from the vault repo; `PATH` is
/// re-supplied so the `git` binary is still found. A failure to spawn
/// (binary absent) is `Exec`; a non-zero exit is surfaced by the
/// caller as `GitExit`.
fn run_git(root: &Path, args: &[&str]) -> Result<std::process::Output, CheckpointError> {
    let mut cmd = std::process::Command::new("git");
    cmd.env_clear();
    if let Some(path) = std::env::var_os("PATH") {
        cmd.env("PATH", path);
    }
    cmd.arg("-C").arg(root).args(args);
    cmd.output()
        .context("running git (is it installed in this environment?)")
        .map_err(CheckpointError::Exec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nudge::SENTINEL_FILE_NAME;
    use tempfile::TempDir;

    fn init_repo(dir: &Path) {
        for args in [
            &["init", "-q"][..],
            &["config", "user.name", "t"],
            &["config", "user.email", "t@t"],
        ] {
            assert!(run_git(dir, args).unwrap().status.success());
        }
    }

    /// A fresh, never-warned paused-op warning state, for tests that
    /// don't care about the warning cadence.
    fn latch() -> AtomicU64 {
        AtomicU64::new(0)
    }

    fn git_ok(dir: &Path, args: &[&str]) {
        assert!(
            run_git(dir, args).unwrap().status.success(),
            "git {args:?} failed"
        );
    }

    fn git_output(dir: &Path, args: &[&str]) -> String {
        String::from_utf8(run_git(dir, args).unwrap().stdout).unwrap()
    }

    fn current_branch(dir: &Path) -> String {
        git_output(dir, &["symbolic-ref", "--short", "HEAD"])
            .trim()
            .to_string()
    }

    #[test]
    fn clean_repo_yields_no_commit() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        // A brand-new repo with no files is clean.
        assert!(matches!(
            git_commit_if_dirty(dir.path(), &latch()),
            Ok(None)
        ));
    }

    #[test]
    fn dirty_repo_commits_and_returns_clean_after() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("note.md"), "hi").unwrap();

        let summary = git_commit_if_dirty(dir.path(), &latch()).unwrap();
        assert!(summary.is_some(), "a dirty tree must commit");
        // Idempotent: immediately after, the tree is clean.
        assert!(matches!(
            git_commit_if_dirty(dir.path(), &latch()),
            Ok(None)
        ));
    }

    #[test]
    fn in_flight_temp_files_are_never_committed() {
        // F1 (temp-file hazard): an atomic write's in-progress temp
        // sibling must never enter history, independent of any lock.
        // `ensure_wip_excluded` adds the prefix to .git/info/exclude,
        // so `git add -A` skips it even when a real change commits.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        ensure_wip_excluded(dir.path()).unwrap();

        // A real note plus a wip temp sibling, both dirty.
        std::fs::write(dir.path().join("note.md"), "real").unwrap();
        let wip = format!("{}abcd", cdno_core::store::WIP_TEMP_PREFIX);
        std::fs::write(dir.path().join(&wip), "garbage-in-flight").unwrap();

        let summary = git_commit_if_dirty(dir.path(), &latch()).unwrap();
        assert!(summary.is_some(), "the real note should commit");

        // The committed tree contains the note but not the wip file.
        let tracked = run_git(dir.path(), &["ls-files"]).unwrap();
        let tracked = String::from_utf8_lossy(&tracked.stdout);
        assert!(tracked.contains("note.md"), "note.md must be committed");
        assert!(
            !tracked.contains(&wip),
            "the in-flight temp file must never be committed: {tracked}"
        );
    }

    #[test]
    fn nudge_only_mode_touches_the_sentinel_and_commits_nothing() {
        // GH #541: the whole point of the mode. A dirty tree must
        // produce a sentinel touch and leave history untouched, so the
        // external agent stays the only git actor in the repo.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let head_before = run_git(dir.path(), &["rev-parse", "--verify", "HEAD"]).unwrap();
        assert!(
            !head_before.status.success(),
            "a fresh repo has no commits yet"
        );

        let sentinel = dir.path().join(".git").join(SENTINEL_FILE_NAME);
        let nudge: SharedNudge =
            std::sync::Arc::new(crate::nudge::SyncNudge::new(sentinel.clone()));
        std::fs::write(dir.path().join("note.md"), "dirty").unwrap();

        let summary = nudge_if_dirty(dir.path(), &nudge, &latch()).unwrap();
        assert!(summary.is_some(), "a dirty tree must nudge");
        assert!(sentinel.exists(), "the sentinel must be touched");

        let log = run_git(dir.path(), &["rev-parse", "--verify", "HEAD"]).unwrap();
        assert!(
            !log.status.success(),
            "nudge-only must create no commit, but HEAD now resolves"
        );
        // And the tree is still dirty — nothing was staged either.
        let status = run_git(dir.path(), &["status", "--porcelain"]).unwrap();
        assert!(
            String::from_utf8_lossy(&status.stdout).contains("note.md"),
            "nudge-only must not stage anything"
        );
    }

    #[test]
    fn nudge_only_mode_leaves_a_clean_tree_alone() {
        // No dirt, no nudge: an agent woken on every tick would be
        // exactly the polling this replaces.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let sentinel = dir.path().join(".git").join(SENTINEL_FILE_NAME);
        let nudge: SharedNudge =
            std::sync::Arc::new(crate::nudge::SyncNudge::new(sentinel.clone()));

        assert!(
            nudge_if_dirty(dir.path(), &nudge, &latch())
                .unwrap()
                .is_none()
        );
        assert!(
            !sentinel.exists(),
            "a clean tree must not touch the sentinel"
        );
    }

    #[test]
    fn ensure_wip_excluded_is_idempotent() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        ensure_wip_excluded(dir.path()).unwrap();
        ensure_wip_excluded(dir.path()).unwrap();
        let exclude =
            std::fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap_or_default();
        let pattern = format!("{}*", cdno_core::store::WIP_TEMP_PREFIX);
        assert_eq!(
            exclude.lines().filter(|l| l.trim() == pattern).count(),
            1,
            "the exclude rule must be written exactly once"
        );
    }

    #[test]
    fn non_repo_is_a_transient_git_exit_not_a_fatal() {
        // `git status` outside a repo exits non-zero — the loop must
        // treat this as transient (GitExit), never Fatal, so one bad
        // tick can't permanently disable checkpointing (F2). (`spawn`
        // separately gates on `.git` existing; this asserts the
        // classification for defence in depth.)
        let dir = TempDir::new().unwrap();
        match git_commit_if_dirty(dir.path(), &latch()) {
            Err(CheckpointError::GitExit(_)) => {}
            other => panic!("expected GitExit outside a repo, got {other:?}"),
        }
    }

    /// Set up two branches that edit the same line of the same file, then
    /// start a merge and let it stop conflicted. Returns the path to the
    /// conflicted file and the name of the branch merged in.
    fn seed_conflicted_merge(dir: &Path) {
        let base_branch = String::from_utf8(
            run_git(dir, &["symbolic-ref", "--short", "HEAD"])
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();

        std::fs::write(dir.join("note.md"), "base\n").unwrap();
        assert!(run_git(dir, &["add", "-A"]).unwrap().status.success());
        assert!(
            run_git(dir, &["commit", "-m", "base"])
                .unwrap()
                .status
                .success()
        );

        assert!(
            run_git(dir, &["checkout", "-b", "feature"])
                .unwrap()
                .status
                .success()
        );
        std::fs::write(dir.join("note.md"), "feature change\n").unwrap();
        assert!(
            run_git(dir, &["commit", "-am", "feature"])
                .unwrap()
                .status
                .success()
        );

        assert!(
            run_git(dir, &["checkout", &base_branch])
                .unwrap()
                .status
                .success()
        );
        std::fs::write(dir.join("note.md"), "base change\n").unwrap();
        assert!(
            run_git(dir, &["commit", "-am", "base change"])
                .unwrap()
                .status
                .success()
        );

        let merge = run_git(dir, &["merge", "feature"]).unwrap();
        assert!(
            !merge.status.success(),
            "the merge must conflict for the test to be meaningful"
        );
        assert!(
            dir.join(".git").join("MERGE_HEAD").exists(),
            "test setup: MERGE_HEAD must exist after a conflicted merge"
        );
    }

    #[test]
    fn conflicted_merge_is_skipped_not_committed() {
        // GH #546: a tree dirty because of someone else's unresolved
        // merge must never be swept. `git add -A` would stage the
        // unmerged path and `git commit` would conclude the merge,
        // embedding raw `<<<<<<<` conflict markers as note content.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        seed_conflicted_merge(dir.path());

        match git_commit_if_dirty(dir.path(), &latch()) {
            Err(CheckpointError::GitExit(msg)) => {
                assert!(
                    msg.contains("merge"),
                    "message should name the merge: {msg}"
                );
            }
            other => panic!("expected a transient skip for a merge in progress, got {other:?}"),
        }

        // The merge is still unresolved: not concluded, not staged.
        assert!(
            dir.path().join(".git").join("MERGE_HEAD").exists(),
            "the sweep must not conclude someone else's merge"
        );
        let content = std::fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(
            content.contains("<<<<<<<"),
            "conflict markers must remain unresolved, got: {content}"
        );
        let status = run_git(dir.path(), &["status", "--porcelain"]).unwrap();
        assert!(
            String::from_utf8_lossy(&status.stdout).contains("UU"),
            "the path must remain unmerged, not staged by the sweep"
        );
    }

    #[test]
    fn merge_head_without_unmerged_paths_is_still_skipped() {
        // Second signal: a merge that stopped with every conflict
        // resolved and staged by hand, but not yet committed, has zero
        // unmerged paths — only `.git/MERGE_HEAD` still marks it as an
        // in-progress merge belonging to someone else. Must be skipped
        // exactly like a live conflict; checking only `diff-filter=U`
        // would miss this state.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        seed_conflicted_merge(dir.path());

        // Resolve by hand and stage, but do not commit.
        std::fs::write(dir.path().join("note.md"), "resolved\n").unwrap();
        assert!(
            run_git(dir.path(), &["add", "-A"])
                .unwrap()
                .status
                .success()
        );
        let unmerged = run_git(dir.path(), &["diff", "--name-only", "--diff-filter=U"]).unwrap();
        assert!(
            unmerged.stdout.is_empty(),
            "test setup: all conflicts should be resolved and staged"
        );
        assert!(dir.path().join(".git").join("MERGE_HEAD").exists());

        match git_commit_if_dirty(dir.path(), &latch()) {
            Err(CheckpointError::GitExit(_)) => {}
            other => panic!(
                "expected a transient skip for MERGE_HEAD with no unmerged paths, got {other:?}"
            ),
        }
        assert!(
            dir.path().join(".git").join("MERGE_HEAD").exists(),
            "the merge must still be unconcluded"
        );
    }

    #[test]
    fn ordinary_dirty_tree_still_commits_with_the_merge_guard_present() {
        // The merge guard must not silently disable checkpointing: an
        // everyday dirty tree (no merge involved) commits exactly as
        // before.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("note.md"), "ordinary edit").unwrap();

        let summary = git_commit_if_dirty(dir.path(), &latch()).unwrap();
        assert!(
            summary.is_some(),
            "an ordinary dirty tree must still commit"
        );
        assert!(matches!(
            git_commit_if_dirty(dir.path(), &latch()),
            Ok(None)
        ));
    }

    #[test]
    fn nudge_only_mode_does_not_nudge_during_a_merge() {
        // Same guard, nudge-only mode: waking the external agent during
        // an unresolved merge invites it to act on a conflicted tree,
        // which is exactly the hazard this issue closes on the
        // committing side.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        seed_conflicted_merge(dir.path());

        let sentinel = dir.path().join(".git").join(SENTINEL_FILE_NAME);
        let nudge: SharedNudge =
            std::sync::Arc::new(crate::nudge::SyncNudge::new(sentinel.clone()));

        match nudge_if_dirty(dir.path(), &nudge, &latch()) {
            Err(CheckpointError::GitExit(_)) => {}
            other => panic!("expected a transient skip for a merge in progress, got {other:?}"),
        }
        assert!(
            !sentinel.exists(),
            "the sweep must not nudge during someone else's merge"
        );
    }

    #[test]
    fn merge_warning_fires_once_per_episode() {
        // Keep-the-log-useful requirement: the state must record a warn
        // timestamp on first detection and hold it across repeated ticks
        // of the same still-unresolved merge, then reset once the tree
        // is clean again.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        seed_conflicted_merge(dir.path());

        let warned = latch();
        assert_eq!(warned.load(Ordering::Relaxed), 0);
        assert!(git_commit_if_dirty(dir.path(), &warned).is_err());
        let first_warn = warned.load(Ordering::Relaxed);
        assert_ne!(
            first_warn, 0,
            "first detection must record a warn timestamp"
        );

        // A second tick against the same unresolved merge, still inside
        // the re-warn interval, must not move the timestamp.
        assert!(git_commit_if_dirty(dir.path(), &warned).is_err());
        assert_eq!(warned.load(Ordering::Relaxed), first_warn);

        // Resolve and commit to conclude the merge properly (as the
        // rightful owner would), then confirm a clean tree rearms it.
        std::fs::write(dir.path().join("note.md"), "resolved\n").unwrap();
        git_ok(dir.path(), &["add", "-A"]);
        git_ok(dir.path(), &["commit", "--no-edit"]);
        assert!(matches!(git_commit_if_dirty(dir.path(), &warned), Ok(None)));
        assert_eq!(
            warned.load(Ordering::Relaxed),
            0,
            "a clean tree must rearm the state for the next episode"
        );
    }

    #[test]
    fn due_for_rewarn_boundaries() {
        // Pure-function coverage for the periodic re-warn decision: `0`
        // (never warned) always fires; inside the interval it doesn't;
        // at or past the interval it does again.
        assert!(due_for_rewarn(0, 1_000));
        assert!(!due_for_rewarn(1_000, 1_000));
        assert!(!due_for_rewarn(1_000, 1_000 + RE_WARN_INTERVAL_SECS - 1));
        assert!(due_for_rewarn(1_000, 1_000 + RE_WARN_INTERVAL_SECS));
    }

    #[test]
    fn stale_paused_state_re_warns_instead_of_going_silent_forever() {
        // Panel-review scenario this exists for: a marker left behind by
        // a crashed process persists across every tick, forever. A bare
        // once-per-episode latch would warn exactly once and then never
        // again — silently losing the recovery trail's visibility. This
        // proves the warning fires again once the interval has passed,
        // by fast-forwarding the recorded timestamp rather than
        // sleeping for real.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        seed_conflicted_merge(dir.path());

        let state = latch();
        assert!(git_commit_if_dirty(dir.path(), &state).is_err());
        let first_warn = state.load(Ordering::Relaxed);
        assert_ne!(first_warn, 0);

        // Still within the interval: must not re-warn.
        assert!(git_commit_if_dirty(dir.path(), &state).is_err());
        assert_eq!(state.load(Ordering::Relaxed), first_warn);

        // Simulate the interval having elapsed since the last warning.
        state.store(first_warn - RE_WARN_INTERVAL_SECS, Ordering::Relaxed);
        assert!(git_commit_if_dirty(dir.path(), &state).is_err());
        assert!(
            state.load(Ordering::Relaxed) >= first_warn,
            "a stale marker must re-warn once the interval elapses, not stay silent forever"
        );
    }

    /// Seed a conflicting cherry-pick: two branches touch the same
    /// line, `git cherry-pick` of the feature branch's tip stops with
    /// `.git/CHERRY_PICK_HEAD` set and the path unmerged.
    fn seed_conflicted_cherry_pick(dir: &Path) {
        let base_branch = current_branch(dir);

        std::fs::write(dir.join("note.md"), "base\n").unwrap();
        git_ok(dir, &["add", "-A"]);
        git_ok(dir, &["commit", "-m", "base"]);

        git_ok(dir, &["checkout", "-b", "feature"]);
        std::fs::write(dir.join("note.md"), "feature change\n").unwrap();
        git_ok(dir, &["commit", "-am", "feature"]);
        let feature_sha = git_output(dir, &["rev-parse", "HEAD"]);

        git_ok(dir, &["checkout", &base_branch]);
        std::fs::write(dir.join("note.md"), "base change\n").unwrap();
        git_ok(dir, &["commit", "-am", "base change"]);

        let cp = run_git(dir, &["cherry-pick", feature_sha.trim()]).unwrap();
        assert!(
            !cp.status.success(),
            "the cherry-pick must conflict for the test to be meaningful"
        );
        assert!(
            dir.join(".git").join("CHERRY_PICK_HEAD").exists(),
            "test setup: CHERRY_PICK_HEAD must exist after a conflicted cherry-pick"
        );
    }

    /// Seed a conflicting revert: three commits on one line, reverting
    /// the middle one against the tip's divergent content stops with
    /// `.git/REVERT_HEAD` set and the path unmerged.
    fn seed_conflicted_revert(dir: &Path) {
        std::fs::write(dir.join("note.md"), "v1\n").unwrap();
        git_ok(dir, &["add", "-A"]);
        git_ok(dir, &["commit", "-m", "v1"]);

        std::fs::write(dir.join("note.md"), "v2\n").unwrap();
        git_ok(dir, &["commit", "-am", "v2"]);
        let v2_sha = git_output(dir, &["rev-parse", "HEAD"]);

        std::fs::write(dir.join("note.md"), "v3\n").unwrap();
        git_ok(dir, &["commit", "-am", "v3"]);

        let revert = run_git(dir, &["revert", "--no-edit", v2_sha.trim()]).unwrap();
        assert!(
            !revert.status.success(),
            "the revert must conflict for the test to be meaningful"
        );
        assert!(
            dir.join(".git").join("REVERT_HEAD").exists(),
            "test setup: REVERT_HEAD must exist after a conflicted revert"
        );
    }

    #[test]
    fn conflicted_cherry_pick_is_skipped_not_committed() {
        // GH #546 (panel review): a plain commit during a paused
        // cherry-pick does not just make an ordinary commit — it
        // concludes the cherry-pick with the sweep's generic message,
        // and a later `git cherry-pick --continue` fails with "no
        // cherry-pick or revert in progress".
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        seed_conflicted_cherry_pick(dir.path());

        match git_commit_if_dirty(dir.path(), &latch()) {
            Err(CheckpointError::GitExit(msg)) => {
                assert!(
                    msg.contains("cherry-pick"),
                    "message should name the cherry-pick: {msg}"
                );
            }
            other => panic!("expected a transient skip for a paused cherry-pick, got {other:?}"),
        }
        assert!(
            dir.path().join(".git").join("CHERRY_PICK_HEAD").exists(),
            "the sweep must not conclude someone else's cherry-pick"
        );
    }

    #[test]
    fn cherry_pick_head_without_unmerged_paths_is_still_skipped() {
        // The reviewer's repro: resolve and stage the conflict by hand,
        // but do NOT run `--continue`. No unmerged paths remain — only
        // `.git/CHERRY_PICK_HEAD` still marks the cherry-pick as paused
        // — so this is the state a diff-filter=U-only check would miss.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        seed_conflicted_cherry_pick(dir.path());

        std::fs::write(dir.path().join("note.md"), "resolved\n").unwrap();
        git_ok(dir.path(), &["add", "-A"]);
        let unmerged = run_git(dir.path(), &["diff", "--name-only", "--diff-filter=U"]).unwrap();
        assert!(
            unmerged.stdout.is_empty(),
            "test setup: the conflict should be resolved and staged"
        );
        assert!(dir.path().join(".git").join("CHERRY_PICK_HEAD").exists());

        match git_commit_if_dirty(dir.path(), &latch()) {
            Err(CheckpointError::GitExit(msg)) => {
                assert!(
                    msg.contains("cherry-pick"),
                    "message should name the cherry-pick: {msg}"
                );
            }
            other => panic!(
                "expected a transient skip for CHERRY_PICK_HEAD with no unmerged paths, got {other:?}"
            ),
        }
        assert!(
            dir.path().join(".git").join("CHERRY_PICK_HEAD").exists(),
            "the sweep must not conclude someone else's cherry-pick"
        );
    }

    #[test]
    fn conflicted_revert_is_skipped_not_committed() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        seed_conflicted_revert(dir.path());

        match git_commit_if_dirty(dir.path(), &latch()) {
            Err(CheckpointError::GitExit(msg)) => {
                assert!(
                    msg.contains("revert"),
                    "message should name the revert: {msg}"
                );
            }
            other => panic!("expected a transient skip for a paused revert, got {other:?}"),
        }
        assert!(
            dir.path().join(".git").join("REVERT_HEAD").exists(),
            "the sweep must not conclude someone else's revert"
        );
    }

    #[test]
    fn revert_head_without_unmerged_paths_is_still_skipped() {
        // Symmetric with cherry-pick: resolved and staged, `--continue`
        // (i.e. `git revert --continue`) never run.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        seed_conflicted_revert(dir.path());

        std::fs::write(dir.path().join("note.md"), "resolved\n").unwrap();
        git_ok(dir.path(), &["add", "-A"]);
        let unmerged = run_git(dir.path(), &["diff", "--name-only", "--diff-filter=U"]).unwrap();
        assert!(
            unmerged.stdout.is_empty(),
            "test setup: the conflict should be resolved and staged"
        );
        assert!(dir.path().join(".git").join("REVERT_HEAD").exists());

        match git_commit_if_dirty(dir.path(), &latch()) {
            Err(CheckpointError::GitExit(msg)) => {
                assert!(
                    msg.contains("revert"),
                    "message should name the revert: {msg}"
                );
            }
            other => panic!(
                "expected a transient skip for REVERT_HEAD with no unmerged paths, got {other:?}"
            ),
        }
        assert!(
            dir.path().join(".git").join("REVERT_HEAD").exists(),
            "the sweep must not conclude someone else's revert"
        );
    }

    #[test]
    fn rebase_state_without_unmerged_paths_is_still_skipped() {
        // Rebase has no MERGE_HEAD at all — it marks itself with
        // `.git/rebase-merge` or `.git/rebase-apply` — and a same-state
        // commit here would be absorbed by `--continue` as "already
        // applied", replacing the original message and leaving HEAD
        // detached mid-rebase.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let base_branch = current_branch(dir.path());

        std::fs::write(dir.path().join("note.md"), "base\n").unwrap();
        git_ok(dir.path(), &["add", "-A"]);
        git_ok(dir.path(), &["commit", "-m", "base"]);

        git_ok(dir.path(), &["checkout", "-b", "feature"]);
        std::fs::write(dir.path().join("note.md"), "feature change\n").unwrap();
        git_ok(dir.path(), &["commit", "-am", "feature"]);

        git_ok(dir.path(), &["checkout", &base_branch]);
        std::fs::write(dir.path().join("note.md"), "base change\n").unwrap();
        git_ok(dir.path(), &["commit", "-am", "base change"]);

        git_ok(dir.path(), &["checkout", "feature"]);
        let rebase = run_git(dir.path(), &["rebase", &base_branch]).unwrap();
        assert!(
            !rebase.status.success(),
            "the rebase must conflict for the test to be meaningful"
        );
        let rebase_merge = dir.path().join(".git").join("rebase-merge");
        let rebase_apply = dir.path().join(".git").join("rebase-apply");
        assert!(
            rebase_merge.is_dir() || rebase_apply.is_dir(),
            "test setup: a conflicted rebase must leave a state directory"
        );

        // Resolve and stage, but do not `--continue`.
        std::fs::write(dir.path().join("note.md"), "resolved\n").unwrap();
        git_ok(dir.path(), &["add", "-A"]);

        match git_commit_if_dirty(dir.path(), &latch()) {
            Err(CheckpointError::GitExit(msg)) => {
                assert!(
                    msg.contains("rebase"),
                    "message should name the rebase: {msg}"
                );
            }
            other => panic!("expected a transient skip for a paused rebase, got {other:?}"),
        }
        assert!(
            rebase_merge.is_dir() || rebase_apply.is_dir(),
            "the sweep must not conclude someone else's rebase"
        );
    }

    impl std::fmt::Debug for CheckpointError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                CheckpointError::GitExit(m) => write!(f, "GitExit({m})"),
                CheckpointError::Exec(e) => write!(f, "Exec({e})"),
            }
        }
    }

    // -----------------------------------------------------------------
    // Stall watchdog (GH #548)
    // -----------------------------------------------------------------

    /// Collects the events a `tracing` subscriber sees, so a test can
    /// assert on the log line itself rather than on a proxy counter —
    /// "produces an error-level line rather than silence" *is* the
    /// behaviour under test.
    #[derive(Clone, Default)]
    struct Captured(Arc<std::sync::Mutex<Vec<(tracing::Level, String)>>>);

    impl Captured {
        fn errors(&self) -> Vec<String> {
            self.matching(tracing::Level::ERROR)
        }

        fn matching(&self, level: tracing::Level) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter(|(l, _)| *l == level)
                .map(|(_, m)| m.clone())
                .collect()
        }
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Captured {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            struct Message(String);
            impl tracing::field::Visit for Message {
                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    if field.name() == "message" {
                        self.0 = format!("{value:?}");
                    }
                }
            }
            let mut message = Message(String::new());
            event.record(&mut message);
            self.0
                .lock()
                .unwrap()
                .push((*event.metadata().level(), message.0));
        }
    }

    /// Run `f` with every `tracing` event it emits on this thread
    /// captured.
    fn with_captured_logs<T>(f: impl FnOnce() -> T) -> (T, Captured) {
        use tracing_subscriber::layer::SubscriberExt;

        let captured = Captured::default();
        let subscriber = tracing_subscriber::registry().with(captured.clone());
        let out = tracing::subscriber::with_default(subscriber, f);
        (out, captured)
    }

    /// A runtime whose blocking pool holds exactly `blocking_threads`
    /// threads.
    ///
    /// Current-thread on purpose: `block_on` drives the future on the
    /// calling thread, which is where [`with_captured_logs`]'s
    /// thread-local dispatcher applies. On a multi-thread runtime
    /// [`await_tick`] would run on a worker and its events would escape
    /// capture, leaving a test that passes for the wrong reason.
    fn runtime(blocking_threads: usize) -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(blocking_threads)
            .build()
            .unwrap()
    }

    #[test]
    fn stall_timeout_is_a_multiple_of_the_interval_above_the_floor() {
        assert_eq!(
            stall_timeout(Duration::from_secs(60)),
            Duration::from_secs(180)
        );
    }

    #[test]
    fn stall_timeout_never_drops_below_the_floor() {
        // A one-second sweep interval must not make a perfectly ordinary
        // `git add`+`commit` look like a stall.
        assert_eq!(stall_timeout(Duration::from_secs(1)), MIN_STALL_TIMEOUT);
        assert_eq!(stall_timeout(Duration::from_millis(1)), MIN_STALL_TIMEOUT);
    }

    /// The other half of the discrimination: a sweep that does real
    /// work for a while is not a stall. The tick deliberately takes
    /// long enough that a watchdog which ignored its threshold — or
    /// used one anywhere near a single sweep's duration — would fire.
    #[test]
    fn a_tick_that_takes_real_work_time_logs_nothing() {
        let rt = runtime(4);
        let (pass, captured) = with_captured_logs(|| {
            rt.block_on(async {
                let tick = tokio::task::spawn_blocking(|| {
                    std::thread::sleep(Duration::from_millis(250));
                    Pass::Ok(None)
                });
                await_tick(tick, Duration::from_secs(30)).await
            })
        });

        assert!(matches!(pass, Ok(Pass::Ok(None))));
        assert!(
            captured.0.lock().unwrap().is_empty(),
            "the watchdog must be silent in normal operation, got {:?}",
            captured.0.lock().unwrap()
        );
    }

    /// The #548 failure itself: the sweep's `spawn_blocking` can get no
    /// thread, so the tick never runs. Reproduced by bounding the
    /// blocking pool to one thread and occupying it — which is what a
    /// process at the host's thread limit looks like from inside tokio.
    #[test]
    fn a_tick_that_cannot_get_a_blocking_thread_is_reported_at_error() {
        let rt = runtime(1);
        let (pass, captured) = with_captured_logs(|| {
            rt.block_on(async {
                // Occupy the only blocking thread, and wait until it has
                // actually been taken so the tick below is queued rather
                // than racing for it.
                let (release, blocked) = std::sync::mpsc::channel::<()>();
                let (occupied, taken) = std::sync::mpsc::channel::<()>();
                let blocker = tokio::task::spawn_blocking(move || {
                    occupied.send(()).unwrap();
                    let _ = blocked.recv();
                });
                taken.recv().unwrap();

                // This one has nowhere to run.
                let tick = tokio::task::spawn_blocking(|| Pass::Ok(None));

                // Free the pool well after the stall threshold, so the
                // watchdog has to speak first and the test still ends.
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    let _ = release.send(());
                });

                let pass = await_tick(tick, Duration::from_millis(80)).await;
                blocker.await.unwrap();
                pass
            })
        });

        // The tick did eventually run, once a thread came free.
        assert!(matches!(pass, Ok(Pass::Ok(None))));

        let errors = captured.errors();
        assert!(
            !errors.is_empty(),
            "a starved tick must produce an error-level line, got {:?}",
            captured.0.lock().unwrap()
        );
        let first = &errors[0];
        assert!(
            first.contains("STALLED"),
            "the line must name the condition: {first}"
        );
        assert!(
            first.contains("no longer being recorded"),
            "the line must state the consequence, not just the symptom: {first}"
        );

        // And the recovery is reported too, so a log window that starts
        // after the stall does not read as an unresolved outage.
        let recovered = captured.matching(tracing::Level::WARN);
        assert!(
            recovered.iter().any(|m| m.contains("recovered")),
            "a stall that ends must say so, got {recovered:?}"
        );
    }
}
