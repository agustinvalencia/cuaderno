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
//! The verbs that retry are those whose every input is their arguments and
//! the vault and whose write adds to a note: the log and notes entries,
//! captures and discards, section appends, the action, milestone,
//! waiting-on and tracking verbs. Nearly every mutating verb appends a
//! history line to today's daily note, so that note is where conflicts
//! arise, whichever verb is called.
//!
//! The verbs that replace content a caller may have composed from what it
//! read earlier (`upsert_*_section` without append, `update_project_state`,
//! `set_frontmatter`, `revise_note`) do not retry, and neither does
//! `rename_note`, which writes from a plan the caller may have approved by
//! its hash. Retrying them would be as safe as their no-conflict path,
//! but a conflict says the note is being written right now, which is the
//! moment to let the caller look again. Whether the caller's own earlier
//! read is stale is a different question that this does not answer (#563).
//! The create and lifecycle verbs (projects, questions, portfolios,
//! commitments, stewardships, closures) do not retry yet either; they are
//! as safe to retry, and are left out only to keep this change small.

use std::time::Duration;

use crate::error::DomainError;

use super::Vault;

/// How many times a retried operation is attempted in all before the
/// conflict is returned to the caller.
pub const CONFLICT_ATTEMPTS: u32 = 4;

impl Vault {
    /// Run `op`, and again while it fails with a concurrent change, up to
    /// [`CONFLICT_ATTEMPTS`] attempts in all. The pause before each retry
    /// grows a little, so two writers that collided do not collide again in
    /// lockstep; each attempt opens (and drops) its own transaction, so the
    /// write lock is never held across the pause.
    pub(in crate::vault) fn retry_on_conflict<T>(
        &self,
        mut op: impl FnMut() -> Result<T, DomainError>,
    ) -> Result<T, DomainError> {
        let mut attempt = 1;
        loop {
            match op() {
                Err(e) if e.is_concurrent_change() && attempt < CONFLICT_ATTEMPTS => {
                    std::thread::sleep(retry_pause(attempt));
                    attempt += 1;
                }
                result => return result,
            }
        }
    }
}

/// The pause before retry `attempt`: a few milliseconds, growing with the
/// attempt and varied by the clock's low bits so two processes do not
/// retry in step.
fn retry_pause(attempt: u32) -> Duration {
    let jitter = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::from(d.subsec_nanos()) % 5);
    Duration::from_millis(u64::from(attempt) * 5 + jitter)
}
