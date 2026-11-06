//! The freshness policy of `cdno note rename` (RFC 0007 §5.8 (2), D4).
//!
//! Policy, not facts: what git says comes from [`cdno_core::git`], and this
//! module only decides what to do about it. A rename refuses when the vault
//! is known to be behind its upstream (overridable with `--skip-sync-check`)
//! or when git is paused mid-operation (never overridable); when the state
//! cannot be determined it warns and proceeds; a vault that is not a git
//! repository skips the check; a dirty tree is never consulted. A dry run is
//! assessed exactly as the write is, so the preview never shows a plan the
//! write would refuse; it only never fetches.
//!
//! The CLI fetches first ([`fetch_quietly`], [`FETCH_TIMEOUT`]), before the
//! write lock is taken. A fetch killed at its deadline can, in a narrow
//! window, leave a stale `refs/remotes/*.lock` behind; the next fetch then
//! fails and reports it, which surfaces here as the failed-fetch warning
//! until the lock file is removed.
//!
//! The whole assessment runs again under the lock, as the guard
//! [`Vault::rename_note`](cdno_domain::Vault::rename_note) takes
//! ([`guard`]), with the same override and no fetch: a merge started, or a
//! fetch landed, between the check and the write is still refused.

use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;

use cdno_core::git::{GitError, GitState, fetch_quietly, git_state};
use cdno_domain::error::DomainError;

/// How long the pre-rename fetch may take before it is killed and the
/// rename goes ahead against the upstream as last fetched.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// What the CLI appends to a `behind_upstream` refusal: the remedy is the
/// interface's, the domain error carries the fact only.
pub const BEHIND_REMEDY: &str = "sync the vault first (pull the upstream's commits, e.g. with `git pull`), or pass `--skip-sync-check` to rename anyway";

/// One reading of the policy: the warnings to show (what could not be
/// determined, or what the override let through), and the refusal, if any.
/// The warnings are shown whether or not the rename is refused.
#[derive(Debug, Default)]
pub struct Assessment {
    pub warnings: Vec<String>,
    pub refusal: Option<DomainError>,
}

/// The policy over one reading of the repository facts. Pure: no git is
/// run here. The refusal is [`DomainError::GitOperationInProgress`] for a
/// paused operation (whatever `skip_sync_check` says), or
/// [`DomainError::BehindUpstream`] unless `skip_sync_check`.
pub fn assess(state: &GitState, skip_sync_check: bool) -> Assessment {
    let (paused, upstream) = match state {
        GitState::NotARepository => return Assessment::default(),
        GitState::Unknown(e) => {
            return Assessment {
                warnings: vec![format!(
                    "could not tell whether the vault is a git repository ({}); renaming without the sync check",
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
            "could not tell whether a git operation is paused ({}); renaming anyway",
            one_line(e)
        )),
    }
    let mut refusal = None;
    match upstream {
        Some(u) if u.behind > 0 => {
            if skip_sync_check {
                warnings.push(format!(
                    "the vault is {} commit{} behind its upstream; renaming anyway (--skip-sync-check)",
                    u.behind,
                    if u.behind == 1 { "" } else { "s" }
                ));
            } else {
                refusal = Some(DomainError::BehindUpstream { behind: u.behind });
            }
        }
        Some(_) => {}
        None => warnings.push(
            "the vault's branch has no upstream to compare with (or it could not be read); renaming without the sync check"
                .to_owned(),
        ),
    }
    Assessment { warnings, refusal }
}

/// The check before the lock. A git repository's upstream is fetched first
/// unless `dry_run` (a failure or a timeout is a warning, and the check
/// reads the upstream as last fetched) or the first reading already shows a
/// paused operation, which refuses whatever a fetch would say; then
/// [`assess`]. A dry run is assessed as the write is.
pub fn check(root: &Path, skip_sync_check: bool, dry_run: bool) -> Assessment {
    let mut state = git_state(root);
    let mut fetch_warning = None;
    let fetch = !dry_run
        && matches!(
            state,
            GitState::Repository {
                paused: Ok(None) | Err(_),
                ..
            }
        );
    if fetch {
        if let Err(e) = fetch_quietly(root, FETCH_TIMEOUT) {
            fetch_warning = Some(format!(
                "could not fetch the upstream ({}); checking against the last fetch",
                one_line(&e)
            ));
        }
        state = git_state(root);
    }
    let mut assessment = assess(&state, skip_sync_check);
    if let Some(w) = fetch_warning {
        assessment.warnings.insert(0, w);
    }
    assessment
}

/// The guard [`Vault::rename_note`](cdno_domain::Vault::rename_note) runs
/// under the lock: [`git_state`] and [`assess`] again, with the same
/// `skip_sync_check` and no fetch. A warning not among `shown` (those of the
/// check before the lock, already shown) is pushed to `fresh` for the
/// caller to show; the refusal, if any, is the guard's error.
pub fn guard<'a>(
    root: &'a Path,
    skip_sync_check: bool,
    shown: &'a [String],
    fresh: &'a RefCell<Vec<String>>,
) -> impl FnOnce() -> Result<(), DomainError> + 'a {
    move || {
        let assessment = assess(&git_state(root), skip_sync_check);
        fresh.borrow_mut().extend(
            assessment
                .warnings
                .into_iter()
                .filter(|w| !shown.contains(w)),
        );
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
