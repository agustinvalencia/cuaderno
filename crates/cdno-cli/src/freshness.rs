//! The freshness policy of `cdno note rename` (RFC 0007 §5.8 (2), D4).
//!
//! Policy, not facts: what git says comes from [`cdno_core::git`], and this
//! module only decides what to do about it. A rename refuses when the vault
//! is known to be behind its upstream (overridable with `--skip-sync-check`)
//! or when git is paused mid-operation (never overridable); when the state
//! cannot be determined it warns and proceeds; a vault that is not a git
//! repository skips the check; a dirty tree is never consulted.
//!
//! The CLI fetches first ([`fetch_quietly`], [`FETCH_TIMEOUT`]), before the
//! write lock is taken; a dry run never fetches. The paused check runs again
//! under the lock as the guard [`Vault::rename_note`](cdno_domain::Vault::rename_note)
//! takes ([`paused_guard`]), so a merge started between the check and the
//! write is still refused.

use std::path::Path;
use std::time::Duration;

use cdno_core::git::{GitError, GitState, fetch_quietly, git_operation_in_progress, git_state};
use cdno_domain::error::DomainError;

/// How long the pre-rename fetch may take before it is killed and the
/// rename goes ahead against the upstream as last fetched.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// What a `behind_upstream` refusal tells the person to do.
pub const BEHIND_REMEDY: &str = "sync the vault first (pull the upstream's commits, e.g. with `git pull`), or pass `--skip-sync-check` to rename anyway";

/// The outcome of a check that lets the rename go ahead.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cleared {
    /// One line each, for stderr: what could not be determined, or what an
    /// override let through.
    pub warnings: Vec<String>,
    /// Whether the vault is a git repository whose paused check should run
    /// again under the lock ([`paused_guard`]).
    pub recheck_paused: bool,
}

/// The policy over one reading of the repository facts: `Err` is the
/// refusal (`git_operation_in_progress`, or `behind_upstream` unless
/// `skip_sync_check`), `Ok` the warnings to print and whether to re-check
/// under the lock. Pure: no git is run here.
pub fn assess(state: &GitState, skip_sync_check: bool) -> Result<Cleared, DomainError> {
    let (paused, upstream) = match state {
        GitState::NotARepository => return Ok(Cleared::default()),
        GitState::Unknown(e) => {
            return Ok(Cleared {
                warnings: vec![format!(
                    "could not tell whether the vault is a git repository ({}); renaming without the sync check",
                    one_line(e)
                )],
                recheck_paused: false,
            });
        }
        GitState::Repository { paused, upstream } => (paused, upstream),
    };
    let mut warnings = Vec::new();
    match paused {
        Ok(Some(op)) => {
            return Err(DomainError::GitOperationInProgress {
                reason: op.reason.clone(),
            });
        }
        Ok(None) => {}
        Err(e) => warnings.push(paused_unknown(e)),
    }
    match upstream {
        Some(u) if u.behind > 0 => {
            if !skip_sync_check {
                return Err(DomainError::BehindUpstream {
                    behind: u.behind,
                    remedy: BEHIND_REMEDY.to_owned(),
                });
            }
            warnings.push(format!(
                "the vault is {} commit{} behind its upstream; renaming anyway (--skip-sync-check)",
                u.behind,
                if u.behind == 1 { "" } else { "s" }
            ));
        }
        Some(_) => {}
        None => warnings.push(
            "the vault's branch has no upstream to compare with (or it could not be read); renaming without the sync check"
                .to_owned(),
        ),
    }
    Ok(Cleared {
        warnings,
        recheck_paused: true,
    })
}

/// The check before the lock. Unless `dry_run`, a git repository's upstream
/// is fetched first (a failure or a timeout is a warning, and the check
/// reads the upstream as last fetched); then [`assess`]. A dry run writes
/// nothing, so it is never refused: what would refuse the write is
/// reported as a warning instead.
pub fn check(root: &Path, skip_sync_check: bool, dry_run: bool) -> Result<Cleared, DomainError> {
    let mut state = git_state(root);
    let mut fetch_warning = None;
    if !dry_run && matches!(state, GitState::Repository { .. }) {
        if let Err(e) = fetch_quietly(root, FETCH_TIMEOUT) {
            fetch_warning = Some(format!(
                "could not fetch the upstream ({}); checking against the last fetch",
                one_line(&e)
            ));
        }
        state = git_state(root);
    }
    let assessed = assess(&state, skip_sync_check);
    let mut cleared = match assessed {
        Ok(cleared) => cleared,
        Err(refusal) if dry_run => Cleared {
            warnings: vec![format!("the rename itself would be refused: {refusal}")],
            recheck_paused: false,
        },
        Err(refusal) => return Err(refusal),
    };
    if let Some(w) = fetch_warning {
        cleared.warnings.insert(0, w);
    }
    Ok(cleared)
}

/// The guard [`Vault::rename_note`](cdno_domain::Vault::rename_note) runs
/// under the lock: the paused check again, for a merge (or rebase, ...)
/// started after [`check`]. A paused operation refuses; a probe that fails
/// warns on stderr and lets the rename go ahead. A no-op unless `recheck`.
pub fn paused_guard(root: &Path, recheck: bool) -> impl FnOnce() -> Result<(), DomainError> + '_ {
    move || {
        if !recheck {
            return Ok(());
        }
        match git_operation_in_progress(root) {
            Ok(Some(op)) => Err(DomainError::GitOperationInProgress { reason: op.reason }),
            Ok(None) => Ok(()),
            Err(e) => {
                eprintln!("warning: {}", paused_unknown(&e));
                Ok(())
            }
        }
    }
}

fn paused_unknown(e: &GitError) -> String {
    format!(
        "could not tell whether a git operation is paused ({}); renaming anyway",
        one_line(e)
    )
}

/// A git error on one line: git's stderr can run to several.
fn one_line(e: &GitError) -> String {
    let text = e.to_string();
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_owned()
}
