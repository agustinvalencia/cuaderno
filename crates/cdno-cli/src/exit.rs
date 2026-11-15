//! Exit statuses other than the generic failure.
//!
//! Every failure exits 1, as a `main` returning an error does, except the
//! ones a script can act on without reading the message.

use cdno_domain::error::DomainError;

/// The status for a write refused because a note changed underneath it
/// while it was being prepared (#1001): nothing was written, and running
/// the same command again is safe. 75 is `EX_TEMPFAIL` from `sysexits.h`,
/// "try again later".
pub const CONCURRENT_CHANGE: i32 = 75;

/// The status `err` exits with when it is not the generic 1, found
/// anywhere in its chain of causes (handlers wrap domain errors in
/// context).
pub fn special_status(err: &anyhow::Error) -> Option<i32> {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<DomainError>())
        .any(DomainError::is_concurrent_change)
        .then_some(CONCURRENT_CHANGE)
}
