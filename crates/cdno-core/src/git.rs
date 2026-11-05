//! Repository facts about a vault, read from `git` (RFC 0007 §5.8).
//!
//! Mechanics only: this module reports what git says and leaves every
//! decision (warn, refuse, proceed) to the interface that asks.
//!
//! **Network rule.** Nothing here touches the network except
//! [`fetch_quietly`]. [`git_state`] reports the upstream divergence *as of
//! the last fetch*; a caller that wants it fresh calls `fetch_quietly`
//! first, under a timeout of its own choosing.
//!
//! **Hang rule.** Every invocation runs with `GIT_TERMINAL_PROMPT=0`,
//! `GIT_OPTIONAL_LOCKS=0`, a null stdin and a deadline; a process that
//! outlives it is killed and surfaces as [`GitError::TimedOut`].

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use thiserror::Error;

/// Deadline for the local, read-only probes.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// What git says about a vault directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitState {
    /// `root` is not inside a git work tree (or git is unavailable).
    NotARepository,
    /// `root` is in a repository.
    Repository {
        /// An operation left paused mid-way, if any.
        paused: Option<PausedGitOp>,
        /// Divergence from the upstream as of the last fetch; `None` when
        /// there is no upstream or the probe failed.
        upstream: Option<Upstream>,
    },
}

/// Commits ahead of / behind the upstream, as of the last fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Upstream {
    pub ahead: u32,
    pub behind: u32,
}

/// A paused git operation found on disk: a human-readable description
/// for the log, and — when the operation has a single marker file or
/// directory an operator could remove to force it away — that marker's
/// `.git`-relative path, so a warning can name exactly what to remove
/// rather than gesturing at "some git state".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PausedGitOp {
    pub reason: String,
    pub marker: Option<&'static str>,
}

/// Failure to run a git command.
#[derive(Debug, Error)]
pub enum GitError {
    /// The `git` binary could not be found.
    #[error("git is not installed or not on PATH")]
    NotInstalled,
    /// `git` ran but exited non-zero.
    #[error("{command}: {stderr}")]
    Exit { command: String, stderr: String },
    /// `git` did not finish before its deadline and was killed.
    #[error("{command}: timed out")]
    TimedOut { command: String },
    /// Any other I/O failure while running git.
    #[error("running git: {0}")]
    Io(#[from] std::io::Error),
}

/// Read the repository facts for `root`. Never touches the network and
/// never errors: a failed probe degrades to the less informative answer.
///
/// The repository probe is `git rev-parse --is-inside-work-tree`, not a
/// `root/.git` existence check: in a linked worktree (or a submodule)
/// `.git` is a *file*, and a vault may also sit in a subdirectory of a
/// larger repository; asking git handles all three, and git is what the
/// later probes would consult anyway.
pub fn git_state(root: &Path) -> GitState {
    match run_git_with_timeout(root, &["rev-parse", "--is-inside-work-tree"], PROBE_TIMEOUT) {
        Ok(out) if out.trim() == "true" => {}
        _ => return GitState::NotARepository,
    }
    let paused = git_operation_in_progress(root).ok().flatten();
    let upstream = run_git_with_timeout(
        root,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
        PROBE_TIMEOUT,
    )
    .ok()
    .and_then(|out| parse_ahead_behind(&out));
    GitState::Repository { paused, upstream }
}

fn parse_ahead_behind(out: &str) -> Option<Upstream> {
    let mut it = out.split_whitespace();
    let ahead = it.next()?.parse().ok()?;
    let behind = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some(Upstream { ahead, behind })
}

/// Detect any git operation another actor left paused mid-way (GH #546;
/// scope widened past merge alone in panel review). Checked in order,
/// each entry catching a state the others miss:
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
/// `Ok(None)` means no paused operation was found. The marker checks
/// assume `root/.git` is a directory; where it is a file (a linked
/// worktree) only the unmerged-paths fallback can fire.
pub fn git_operation_in_progress(root: &Path) -> Result<Option<PausedGitOp>, GitError> {
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

    let unmerged = run_git_with_timeout(
        root,
        &["diff", "--name-only", "--diff-filter=U"],
        PROBE_TIMEOUT,
    )?;
    if unmerged.is_empty() {
        return Ok(None);
    }
    let n = unmerged.matches('\n').count();
    Ok(Some(PausedGitOp {
        reason: format!(
            "{n} unmerged path(s) present with no operation marker (e.g. a conflicted `git stash pop`)"
        ),
        marker: None,
    }))
}

/// `git fetch --quiet` for the current branch's upstream remote (git's
/// default remote when there is none). The only function in this module
/// that touches the network; killed and reported as
/// [`GitError::TimedOut`] if it outlives `timeout`.
pub fn fetch_quietly(root: &Path, timeout: Duration) -> Result<(), GitError> {
    run_git_with_timeout(root, &["fetch", "--quiet"], timeout).map(|_| ())
}

/// Run `git -C root <args>` and return its stdout.
///
/// `env_clear` (PR #306 security review, finding 3) stops an ambient
/// `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`/`GIT_CONFIG_*` from
/// redirecting the call away from the vault repo; `PATH` is re-supplied
/// so `git` is found, and `HOME`, `XDG_CONFIG_HOME` and `SSH_AUTH_SOCK`
/// so the user's git config and ssh agent work for the fetch.
///
/// Output is drained on threads so a chatty child cannot fill a pipe and
/// stall; the wait is a `try_wait` loop against a deadline, after which
/// the child is killed. The reader threads are not joined on the timeout
/// path: a grandchild (a transport helper) may still hold the pipes open.
fn run_git_with_timeout(root: &Path, args: &[&str], timeout: Duration) -> Result<String, GitError> {
    let command = format!("git {}", args.join(" "));
    let mut cmd = Command::new("git");
    cmd.env_clear();
    for var in ["PATH", "HOME", "XDG_CONFIG_HOME", "SSH_AUTH_SOCK"] {
        if let Some(v) = std::env::var_os(var) {
            cmd.env(var, v);
        }
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GitError::NotInstalled
        } else {
            GitError::Io(e)
        }
    })?;

    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out_h = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let err_h = drain(child.stderr.take().map(|p| Box::new(p) as _));

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(GitError::TimedOut { command });
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let stdout = out_h.join().unwrap_or_default();
    let stderr = err_h.join().unwrap_or_default();
    if !status.success() {
        return Err(GitError::Exit {
            command,
            stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}
