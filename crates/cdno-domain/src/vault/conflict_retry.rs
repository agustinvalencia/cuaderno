//! A bounded retry for a commit refused because a file changed underneath
//! it (#1001).
//!
//! A refused commit wrote nothing (`TransactionError::Conflict` is
//! returned only before any file op, or after a clean rollback), every
//! verb writes in one transaction, and the comparison is against the
//! transaction's own reads. So running a verb again re-reads the vault and
//! applies the change to what is there now, exactly as its no-conflict
//! path would have. The transaction cannot do this by itself: re-applying
//! means re-running the domain operation that composed the write.
//!
//! **Retried** (every input is the arguments and the vault, and the write
//! adds to a note): `log_to_daily_note`, `note_to_daily`,
//! `upsert_daily_section` with `append`, `capture_to_inbox`,
//! `discard_inbox_item`, the action verbs `add_action`, `start_action`,
//! `start_unplanned_action`, `switch_action`, `switch_unplanned_action`,
//! `pause_action`, `resume_action`, `complete_action` and `drop_action`, the
//! milestone verbs `add_milestone`, `complete_milestone` and
//! `drop_milestone`, the waiting-on verbs `add_waiting_on` and
//! `resolve_waiting_on`, and `add_tracking_entry`. Nearly every mutating
//! verb appends a history line to today's daily note, so that note is where
//! conflicts arise, whichever verb is called.
//!
//! **Not retried**, returning the conflict at once:
//! - the verbs that replace content a caller may have composed from what it
//!   read earlier: `upsert_*_section` without `append`,
//!   `update_project_state`, `set_core_question`, `set_frontmatter` and
//!   `revise_note`. Retrying them would be as safe as their no-conflict
//!   path, but a conflict says the note is being written right now, which
//!   is the moment to let the caller look again. Whether the caller's own
//!   earlier read is stale is a different question this does not answer
//!   (#563);
//! - `rename_note`, which writes a plan the caller may have approved by its
//!   hash;
//! - the create and lifecycle verbs (`create_*`, `promote_action`,
//!   `hand_off_project`, parking, activating, completing and dropping
//!   projects, commitments, questions, portfolio filing, migration). They
//!   are as safe to retry and are left out only to keep this change
//!   reviewable.
//!
//! The docs that list these (`docs/concurrent-writers.md`, the MCP overview,
//! troubleshooting) follow this list.

use std::time::Duration;

use crate::error::DomainError;

use super::Vault;

/// How many times a retried operation is attempted in all before the
/// conflict is returned to the caller.
pub const CONFLICT_ATTEMPTS: u32 = 5;

/// The pause before each retry, in milliseconds, before jitter: the first
/// retry is almost immediate (most conflicts are one other write landing),
/// the later ones long enough for a commit across a VM file share to
/// finish. `RETRY_PAUSES_MS[n]` precedes attempt `n + 2`.
const RETRY_PAUSES_MS: [u64; (CONFLICT_ATTEMPTS - 1) as usize] = [5, 25, 100, 250];

impl Vault {
    /// Run `op`, and again while it fails with a concurrent change, up to
    /// [`CONFLICT_ATTEMPTS`] attempts in all, pausing per
    /// [`RETRY_PAUSES_MS`] plus a little jitter, so two writers that
    /// collided do not collide again in step. Each attempt opens (and
    /// drops) its own transaction, so the write lock is never held across
    /// a pause. Every retried conflict is logged at debug, a final one at
    /// warn.
    pub(in crate::vault) fn retry_on_conflict<T>(
        &self,
        mut op: impl FnMut() -> Result<T, DomainError>,
    ) -> Result<T, DomainError> {
        let mut attempt = 1;
        loop {
            match op() {
                Err(e) if e.is_concurrent_change() && attempt < CONFLICT_ATTEMPTS => {
                    tracing::debug!(
                        paths = %conflict_paths(&e),
                        attempt,
                        "write refused: a file changed underneath it; running it again"
                    );
                    std::thread::sleep(retry_pause(attempt));
                    attempt += 1;
                }
                Err(e) if e.is_concurrent_change() => {
                    tracing::warn!(
                        paths = %conflict_paths(&e),
                        attempts = attempt,
                        "write refused: a file kept changing underneath it; giving up"
                    );
                    return Err(e);
                }
                result => return result,
            }
        }
    }
}

/// The paths a concurrent change names, for the log.
fn conflict_paths(e: &DomainError) -> String {
    match e {
        DomainError::Transaction(cdno_core::error::TransactionError::Conflict { paths }) => paths
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

/// The pause before retry `attempt` (the first retry is `attempt` 1): its
/// entry in [`RETRY_PAUSES_MS`], plus up to a fifth more, varied by the
/// clock's low bits so two processes do not retry in step.
fn retry_pause(attempt: u32) -> Duration {
    let index = (attempt as usize - 1).min(RETRY_PAUSES_MS.len() - 1);
    let base = RETRY_PAUSES_MS[index];
    let jitter = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::from(d.subsec_nanos()) % (base / 5 + 1));
    Duration::from_millis(base + jitter)
}
