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
//! normal state between checkpoints). A dry run is assessed exactly as the
//! write is.
//!
//! What `behind` means depends on the deployment. A server that commits its
//! own checkpoints and never fetches sees `behind` only as the last human
//! fetch left it, so the check rarely blocks. Behind a sync actor that
//! fetches, merges under the vault's lock and pushes, `behind > 0` is
//! either transient (its fetch landed, its merge has not) or a sign the
//! actor has stalled; a conflicted merge leaves `MERGE_HEAD`, refused
//! regardless. The whole assessment runs again under the lock ([`guard`]),
//! which is what the actor will see next.
//!
//! Policy only: the facts are [`cdno_core::git`]'s.

use std::cell::RefCell;
use std::path::Path;

use cdno_core::git::{GitError, GitState, git_state};
use cdno_domain::error::DomainError;

/// What the tool appends to a `behind_upstream` refusal (T26 renders it):
/// the remedy is the interface's, the domain error carries the fact only.
pub const BEHIND_REMEDY: &str = "retry in a few seconds; if still behind, the sync is stalled: tell the person rather than skip the check";

/// One reading of the policy: the warnings (what could not be determined,
/// or what the override let through, one sentence each, for the tool's
/// response and the server log), and the refusal, if any. The warnings are
/// reported whether or not the rename is refused.
#[derive(Debug, Default)]
pub struct Assessment {
    pub warnings: Vec<String>,
    pub refusal: Option<DomainError>,
}

/// The policy over one reading of the repository facts. Pure: no git is
/// run. The refusal is [`DomainError::GitOperationInProgress`] for a paused
/// operation (whatever `skip_sync_check` says), or
/// [`DomainError::BehindUpstream`] unless `skip_sync_check`.
pub fn assess(state: &GitState, skip_sync_check: bool) -> Assessment {
    let (paused, upstream) = match state {
        GitState::NotARepository => return Assessment::default(),
        GitState::Unknown(e) => {
            return Assessment {
                warnings: vec![format!(
                    "could not tell whether the vault is a git repository ({}); renamed without the sync check",
                    one_line(e)
                )],
                refusal: None,
            };
        }
        GitState::Repository { paused, upstream } => (paused, upstream),
    };
    let mut warnings = Vec::new();
    match paused {
        Ok(Some(op)) => {
            return Assessment {
                warnings,
                refusal: Some(DomainError::GitOperationInProgress {
                    reason: op.reason.clone(),
                }),
            };
        }
        Ok(None) => {}
        Err(e) => warnings.push(format!(
            "could not tell whether a git operation is paused ({}); renamed anyway",
            one_line(e)
        )),
    }
    let mut refusal = None;
    match upstream {
        Some(u) if u.behind > 0 => {
            if skip_sync_check {
                warnings.push(format!(
                    "the vault is {} commit{} behind its upstream as last fetched; renamed anyway (skip_sync_check)",
                    u.behind,
                    if u.behind == 1 { "" } else { "s" }
                ));
            } else {
                refusal = Some(DomainError::BehindUpstream { behind: u.behind });
            }
        }
        Some(_) => {}
        None => warnings.push(
            "the vault's branch has no upstream to compare with (or it could not be read); renamed without the sync check"
                .to_owned(),
        ),
    }
    Assessment { warnings, refusal }
}

/// The check before the lock: [`assess`] over [`git_state`], which reads
/// the upstream as last fetched and never touches the network. The same
/// for a dry run and a write.
pub fn check(root: &Path, skip_sync_check: bool) -> Assessment {
    assess(&git_state(root), skip_sync_check)
}

/// The guard `Vault::rename_note` runs under the lock: [`check`] again,
/// with the same `skip_sync_check`. A warning not among `shown` (the first
/// check's, already reported) is pushed to `fresh` and logged; the refusal,
/// if any, is the guard's error.
pub fn guard<'a>(
    root: &'a Path,
    skip_sync_check: bool,
    shown: &'a [String],
    fresh: &'a RefCell<Vec<String>>,
) -> impl FnOnce() -> Result<(), DomainError> + 'a {
    move || {
        let assessment = check(root, skip_sync_check);
        for warning in assessment.warnings {
            if !shown.contains(&warning) {
                tracing::warn!("{warning}");
                fresh.borrow_mut().push(warning);
            }
        }
        match assessment.refusal {
            Some(refusal) => Err(refusal),
            None => Ok(()),
        }
    }
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
