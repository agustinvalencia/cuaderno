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
//! The sweep takes the **vault write lock** around `add`+`commit`
//! (F1): a transaction applies its file ops one atomic rename at a
//! time while holding that lock, and atomic writes stage a temp
//! sibling in the target directory for the same window. Committing
//! under the lock therefore guarantees the tree is neither
//! half-applied (some ops of a multi-file transaction missing) nor
//! carrying an in-flight temp file. `status` is read under the lock
//! too so the dirty check matches what gets committed.
//!
//! Platform note: this relies on `flock(2)` being per-open-file-
//! description, so the checkpoint task and a transaction in the *same
//! server process* mutually exclude. That holds on **Linux** — the
//! deployment target (the server runs in a Linux container) and CI.
//! macOS does not reliably conflict same-process flock across
//! descriptors; it is not a deployment platform for the server, and
//! cross-process locking (host CLI vs container) is reliable
//! everywhere.
//!
//! A single git failure must **not** permanently disable the audit
//! trail (F2): non-zero `git` exits (`.git/index.lock` contention with
//! the operator's own git use is routine) and lock-acquisition
//! timeouts are transient — the loop logs and continues. Only a git
//! binary that cannot be executed at all, repeated `MAX_CONSECUTIVE`
//! times, stops the loop.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use cdno_core::store::{FsVaultStore, VaultStore};

/// Consecutive hard failures (git not executable) before giving up.
const MAX_CONSECUTIVE_FAILURES: u32 = 5;

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
pub fn spawn(root: PathBuf, every: Duration) {
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

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut consecutive_failures: u32 = 0;
        loop {
            interval.tick().await;
            let repo = root.clone();
            let pass = tokio::task::spawn_blocking(move || checkpoint_once(&repo)).await;
            match pass {
                Ok(Pass::Ok(Some(summary))) => {
                    consecutive_failures = 0;
                    tracing::info!(%summary, "git checkpoint committed");
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
fn checkpoint_once(root: &Path) -> Pass {
    // Serialise against all vault writers (this process's transactions
    // and any cross-process cdno CLI) via the same flock they take.
    let store = FsVaultStore::new(root);
    let _lock = match store.acquire_write_lock() {
        Ok(guard) => guard,
        // Lock contention is transient by construction: a writer holds
        // it briefly. Skip this tick; the next one will get it.
        Err(e) => return Pass::Transient(format!("vault write lock: {e}")),
    };

    match git_commit_if_dirty(root) {
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

    // Linux-only: this asserts the vault write lock serialises a
    // checkpoint against a *same-process* writer. `flock(2)` is
    // per-open-file-description on Linux, so two `File` handles in one
    // process conflict — which is exactly the server's situation
    // (checkpoint task vs transaction, same process). macOS does NOT
    // reliably conflict same-process flock across descriptors, so the
    // guarantee (and this test) is asserted on the deployment target
    // and CI (both Linux). Cross-process locking, used by the host
    // CLI vs the container, is reliable on both.
    #[cfg(target_os = "linux")]
    #[test]
    fn checkpoint_does_not_commit_while_the_vault_write_lock_is_held() {
        // F1: a checkpoint must never commit while a writer holds the
        // vault lock — that window is exactly when temp siblings and
        // half-applied multi-file transactions exist on disk. The pass
        // may either block for the lock or skip as transient; both are
        // correct. What must hold: NO commit lands while locked, and a
        // later pass commits once the lock is free.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::write(dir.path().join("seed.md"), "seed").unwrap();
        git_commit_if_dirty(dir.path()).unwrap();
        // Dirty the tree so a lock-free checkpoint WOULD commit.
        std::fs::write(dir.path().join("note.md"), "dirty").unwrap();

        let store = FsVaultStore::new(dir.path());
        let lock = store.acquire_write_lock().unwrap();

        // Run a pass in a thread (it may block until we release).
        let repo = dir.path().to_path_buf();
        let handle = std::thread::spawn(move || checkpoint_once(&repo));

        // While we hold the lock the pass blocks on it (Linux flock is
        // per-OFD): nothing is committed.
        std::thread::sleep(Duration::from_millis(300));
        let log = run_git(dir.path(), &["log", "--oneline"]).unwrap();
        assert!(
            !String::from_utf8_lossy(&log.stdout).contains("cdno-mcp checkpoint"),
            "checkpoint must not commit while the write lock is held"
        );

        // Release → the blocked pass acquires the lock and commits the
        // still-dirty tree.
        drop(lock);
        assert!(
            matches!(handle.join().unwrap(), Pass::Ok(Some(_))),
            "the unblocked checkpoint should commit"
        );
        let log = run_git(dir.path(), &["log", "--oneline"]).unwrap();
        assert!(
            String::from_utf8_lossy(&log.stdout).contains("cdno-mcp checkpoint"),
            "checkpoint must commit once the lock is released"
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
