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
//! outlives it is killed, with its whole process group (transport helpers
//! included), and surfaces as [`GitError::TimedOut`].

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use thiserror::Error;

/// Deadline for the local, read-only probes.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// What git says about a vault directory.
#[derive(Debug)]
pub enum GitState {
    /// `root` is not inside a git work tree: git said so.
    NotARepository,
    /// Git could not answer (a timeout, a refused `safe.directory`, git
    /// not installed, a broken config, ...). Whether `root` is a
    /// repository is unknown; callers warn and proceed (RFC 0007 §5.8).
    Unknown(GitError),
    /// `root` is in a repository.
    Repository {
        /// An operation left paused mid-way, if any. An `Err` means the
        /// probe failed, so whether one is paused is *unknown*; callers
        /// must not read it as "not paused" (RFC 0007 §5.8: cannot be
        /// determined → warn and proceed).
        paused: Result<Option<PausedGitOp>, GitError>,
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
/// never errors: a git that cannot answer is [`GitState::Unknown`]. A failed upstream probe is `None` (unknown or none, the
/// caller warns either way); a failed paused probe stays visible as an
/// `Err` in `paused`.
///
/// The repository probe is `git rev-parse --is-inside-work-tree`, not a
/// `root/.git` existence check: in a linked worktree (or a submodule)
/// `.git` is a *file*, and a vault may also sit in a subdirectory of a
/// larger repository; asking git handles all three. Only git's own "not a
/// git repository" (or `false`) is `NotARepository`; any other failure is
/// `Unknown`.
pub fn git_state(root: &Path) -> GitState {
    match run_git_with_timeout(root, &["rev-parse", "--is-inside-work-tree"], PROBE_TIMEOUT) {
        Ok(out) if out.trim() == "true" => {}
        // `false`: inside a `.git` directory or a bare repository.
        Ok(_) => return GitState::NotARepository,
        Err(GitError::Exit { stderr, .. }) if stderr.contains("not a git repository") => {
            return GitState::NotARepository;
        }
        Err(e) => return GitState::Unknown(e),
    }
    let paused = git_operation_in_progress(root);
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
/// `Ok(None)` means no paused operation was found.
pub fn git_operation_in_progress(root: &Path) -> Result<Option<PausedGitOp>, GitError> {
    // Ask git where the git dir is: `root/.git` is wrong for a vault in a
    // subdirectory of a repository and for a linked worktree (whose
    // per-worktree markers live under the main `.git/worktrees/<name>`).
    // Fall back to `root/.git` if git cannot say.
    let git_dir = run_git_with_timeout(root, &["rev-parse", "--absolute-git-dir"], PROBE_TIMEOUT)
        .ok()
        .map(|out| std::path::PathBuf::from(out.trim()))
        .unwrap_or_else(|| root.join(".git"));

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

/// Environment variables passed through to git. Everything else is
/// cleared (PR #306 security review, finding 3: an ambient `GIT_DIR`,
/// `GIT_WORK_TREE`, `GIT_INDEX_FILE` or `GIT_CONFIG_*` must not redirect
/// the call away from the vault repo). `HOME`/`XDG_CONFIG_HOME` carry the
/// user's git config, the rest what a fetch needs to reach a remote:
/// the ssh agent and command, and the proxy settings. `GIT_ASKPASS` and
/// `SSH_ASKPASS` are deliberately absent: nothing may prompt.
const PASSTHROUGH_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "XDG_CONFIG_HOME",
    "SSH_AUTH_SOCK",
    "GIT_SSH_COMMAND",
    "http_proxy",
    "https_proxy",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "no_proxy",
    "NO_PROXY",
    "ALL_PROXY",
];

/// How long to wait for the drain threads once git has exited or been
/// killed, before giving up on whatever a descendant still holds open.
const DRAIN_GRACE: Duration = Duration::from_secs(1);

/// Kill the child's whole process group (it leads one: see the spawn),
/// so a transport helper (`git-remote-https`, `ssh`) dies with it. On
/// non-unix targets only the child itself is killed.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // SAFETY: killpg on a pid we spawned as a group leader.
        unsafe {
            libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
        }
    }
    let _ = child.kill();
}

/// Run `git -C root <args>` and return its stdout.
///
/// The child leads its own process group, so on the deadline the whole
/// tree is killed (a killed `git` alone orphans its transport helper,
/// which keeps running and keeps the pipes open); a group that is not the
/// terminal's foreground group also cannot prompt on the tty. Output is
/// drained on threads so a chatty child cannot fill a pipe and stall; the
/// wait is a `try_wait` loop against a deadline. Draining is itself
/// bounded: if a descendant outlives git and holds a pipe, the group is
/// killed and whatever was read is used.
fn run_git_with_timeout(root: &Path, args: &[&str], timeout: Duration) -> Result<String, GitError> {
    let command = format!("git {}", args.join(" "));
    let mut cmd = Command::new("git");
    cmd.env_clear();
    for var in PASSTHROUGH_ENV {
        if let Some(v) = std::env::var_os(var) {
            cmd.env(var, v);
        }
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GitError::NotInstalled
        } else {
            GitError::Io(e)
        }
    })?;

    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    };
    let out_rx = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let err_rx = drain(child.stderr.take().map(|p| Box::new(p) as _));

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Err(e) => {
                kill_tree(&mut child);
                let _ = child.wait();
                return Err(GitError::Io(e));
            }
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                kill_tree(&mut child);
                let _ = child.wait();
                // The group is dead, so the pipes close; take the readers
                // down with a bound rather than leave them behind.
                let _ = out_rx.recv_timeout(DRAIN_GRACE);
                let _ = err_rx.recv_timeout(DRAIN_GRACE);
                return Err(GitError::TimedOut { command });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    // A descendant that outlived git and still holds a pipe means the
    // output may be incomplete: empty output would read as "nothing
    // found", so this is a timeout, and the group goes too.
    let mut collect = |rx: mpsc::Receiver<Vec<u8>>| match rx.recv_timeout(DRAIN_GRACE) {
        Ok(buf) => Ok(buf),
        Err(_) => {
            kill_tree(&mut child);
            Err(GitError::TimedOut {
                command: command.clone(),
            })
        }
    };
    let stdout = collect(out_rx)?;
    let stderr = collect(err_rx)?;
    if !status.success() {
        return Err(GitError::Exit {
            command,
            stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}
