//! The freshness policy of the `rename_note` tool (RFC 0007 §5.8 (2), D4).
//!
//! The same policy as `cdno note rename`'s, with two differences that are
//! the servers' own: **nothing here ever fetches** (a server uses the
//! upstream as last fetched, by whatever syncs the vault), and the one
//! override is the tool's `skip_sync_check: true`. A rename refuses when the
//! vault is known to be behind its upstream (overridable) or when git is
//! paused mid-operation (never overridable); when the state cannot be
//! determined it warns and proceeds; a vault that is not a git repository
//! skips the check; a dirty tree is never consulted (on the server it is the
//! normal state between checkpoints).
//!
//! Policy only: the facts are [`cdno_core::git`]'s.

use std::path::Path;

use cdno_core::git::{GitError, GitState, git_operation_in_progress, git_state};
use cdno_domain::error::DomainError;

/// What a `behind_upstream` refusal tells the agent to do.
pub const BEHIND_REMEDY: &str = "let the vault sync first (its upstream has commits this clone has not merged), or call again with `skip_sync_check: true` to rename anyway";

/// The outcome of a check that lets the rename go ahead.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cleared {
    /// What could not be determined, or what the override let through, one
    /// sentence each: for the tool's response and the server log.
    pub warnings: Vec<String>,
    /// Whether the vault is a git repository whose paused check should run
    /// again under the lock ([`paused_guard`]).
    pub recheck_paused: bool,
}

/// The policy over one reading of the repository facts: `Err` is the
/// refusal ([`DomainError::GitOperationInProgress`], or
/// [`DomainError::BehindUpstream`] unless `skip_sync_check`), `Ok` the
/// warnings and whether to re-check under the lock. Pure: no git is run.
pub fn assess(state: &GitState, skip_sync_check: bool) -> Result<Cleared, DomainError> {
    let (paused, upstream) = match state {
        GitState::NotARepository => return Ok(Cleared::default()),
        GitState::Unknown(e) => {
            return Ok(Cleared {
                warnings: vec![format!(
                    "could not tell whether the vault is a git repository ({}); renamed without the sync check",
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
                "the vault is {} commit{} behind its upstream as last fetched; renamed anyway (skip_sync_check)",
                u.behind,
                if u.behind == 1 { "" } else { "s" }
            ));
        }
        Some(_) => {}
        None => warnings.push(
            "the vault's branch has no upstream to compare with (or it could not be read); renamed without the sync check"
                .to_owned(),
        ),
    }
    Ok(Cleared {
        warnings,
        recheck_paused: true,
    })
}

/// The check before the lock: [`assess`] over [`git_state`], which reads
/// the upstream as last fetched and never touches the network.
pub fn check(root: &Path, skip_sync_check: bool) -> Result<Cleared, DomainError> {
    assess(&git_state(root), skip_sync_check)
}

/// The paused check again, over one reading of
/// [`git_operation_in_progress`]: a paused operation refuses, a failed
/// probe is logged and lets the rename go ahead.
pub fn recheck(
    paused: Result<Option<cdno_core::git::PausedGitOp>, GitError>,
) -> Result<(), DomainError> {
    match paused {
        Ok(Some(op)) => Err(DomainError::GitOperationInProgress { reason: op.reason }),
        Ok(None) => Ok(()),
        Err(e) => {
            tracing::warn!("{}", paused_unknown(&e));
            Ok(())
        }
    }
}

/// The guard `Vault::rename_note` runs under the lock: [`recheck`] for a
/// merge (or rebase, ...) started after [`check`]. A no-op unless
/// `recheck_paused`.
pub fn paused_guard(
    root: &Path,
    recheck_paused: bool,
) -> impl FnOnce() -> Result<(), DomainError> + '_ {
    move || {
        if !recheck_paused {
            return Ok(());
        }
        recheck(git_operation_in_progress(root))
    }
}

fn paused_unknown(e: &GitError) -> String {
    format!(
        "could not tell whether a git operation is paused ({}); renamed anyway",
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
