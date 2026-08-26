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
//! Two hazards, handled separately (PR #306 review, F1):
//!
//! - **In-flight temp files.** Atomic writes stage a temp sibling
//!   ([`cdno_core::store::WIP_TEMP_PREFIX`]) next to their target.
//!   These must never enter history. Handled *deterministically*, not
//!   by locking: the loop adds the prefix to `.git/info/exclude` once
//!   at startup, so `git status`/`add -A` ignore wip files regardless
//!   of any lock or platform.
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
use std::time::Duration;

use anyhow::{Context, Result};
use cdno_core::store::{FsVaultStore, VaultStore};

use crate::nudge::SharedNudge;

/// Consecutive hard failures (git not executable) before giving up.
const MAX_CONSECUTIVE_FAILURES: u32 = 5;

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

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut consecutive_failures: u32 = 0;
        loop {
            interval.tick().await;
            let repo = root.clone();
            let mode = mode.clone();
            let pass = tokio::task::spawn_blocking(move || checkpoint_once(&repo, &mode)).await;
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

/// One sweep under the vault write lock. `pub(crate)`-visible via the
/// `Pass` classification so the loop can distinguish transient from
/// fatal; the lock guarantees no half-applied transaction or temp
/// sibling is committed.
fn checkpoint_once(root: &Path, mode: &CheckpointMode) -> Pass {
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
        CheckpointMode::Commit => git_commit_if_dirty(root),
        CheckpointMode::NudgeOnly(nudge) => nudge_if_dirty(root, nudge),
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
fn git_commit_if_dirty(root: &Path) -> Result<Option<String>, CheckpointError> {
    let status = run_git(root, &["status", "--porcelain"])?;
    if !status.status.success() {
        return Err(CheckpointError::GitExit(format!(
            "git status: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        )));
    }
    if status.stdout.is_empty() {
        return Ok(None);
    }
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
fn nudge_if_dirty(root: &Path, nudge: &SharedNudge) -> Result<Option<String>, CheckpointError> {
    let status = run_git(root, &["status", "--porcelain"])?;
    if !status.status.success() {
        return Err(CheckpointError::GitExit(format!(
            "git status: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        )));
    }
    if status.stdout.is_empty() {
        return Ok(None);
    }
    let dirty_paths = status.stdout.iter().filter(|&&b| b == b'\n').count();
    nudge.touch();
    Ok(Some(format!(
        "nudged the sync agent ({dirty_paths} dirty path(s), nothing committed here)"
    )))
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

    #[test]
    fn clean_repo_yields_no_commit() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        // A brand-new repo with no files is clean.
        assert!(matches!(git_commit_if_dirty(dir.path()), Ok(None)));
    }

    #[test]
    fn dirty_repo_commits_and_returns_clean_after() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("note.md"), "hi").unwrap();

        let summary = git_commit_if_dirty(dir.path()).unwrap();
        assert!(summary.is_some(), "a dirty tree must commit");
        // Idempotent: immediately after, the tree is clean.
        assert!(matches!(git_commit_if_dirty(dir.path()), Ok(None)));
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

        let summary = git_commit_if_dirty(dir.path()).unwrap();
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

        let summary = nudge_if_dirty(dir.path(), &nudge).unwrap();
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

        assert!(nudge_if_dirty(dir.path(), &nudge).unwrap().is_none());
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
        match git_commit_if_dirty(dir.path()) {
            Err(CheckpointError::GitExit(_)) => {}
            other => panic!("expected GitExit outside a repo, got {other:?}"),
        }
    }

    impl std::fmt::Debug for CheckpointError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                CheckpointError::GitExit(m) => write!(f, "GitExit({m})"),
                CheckpointError::Exec(e) => write!(f, "Exec({e})"),
            }
        }
    }
}
