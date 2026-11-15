//! A bounded retry for the operations that may safely run again when a
//! commit was refused because a file changed underneath it (#1001).
//!
//! A refused commit wrote nothing, so running the whole operation again
//! re-reads the file and re-applies the change to what is there now. The
//! transaction cannot do this by itself: re-applying means re-running the
//! domain operation that composed the write. Only operations whose every
//! input is their arguments and the vault use it; an operation whose
//! content the caller composed from an earlier read of its own (a section
//! replacement, a revision) does not, because the caller must re-read and
//! re-derive, and the error tells it so.

use std::time::Duration;

use crate::error::DomainError;

use super::Vault;

/// How many times an operation is attempted in all before the conflict is
/// returned to the caller.
pub(in crate::vault) const CONFLICT_ATTEMPTS: u32 = 4;

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
