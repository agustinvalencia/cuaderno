//! The exit status of a write that wrote nothing and may simply be run
//! again (#1001, #1012).
//!
//! `cdno` exits 0 on success and 1 on a failure, clap exits 2 on a usage
//! error, and `cdno open` passes the editor's own status through. This
//! module adds one more: 75 for a write that wrote nothing because of the
//! moment rather than the command, so that running the same command again
//! is the remedy: a file it would replace changed while it was being
//! prepared, its vault write lock was broken under it and everything it had
//! written was put back, or the vault write lock stayed held by another
//! writer for the whole wait.

use cdno_core::error::{StoreError, TransactionError};
use cdno_domain::error::DomainError;

/// The status for a write that wrote nothing and may be run again (#1001,
/// #1012). 75 is `EX_TEMPFAIL` from `sysexits.h`, "try again later".
/// `cdno log` returns it for a changed file only after its own retries, so
/// a script that retries on it should bound its loop.
pub const CONCURRENT_CHANGE: i32 = 75;

/// The first error in `err`'s chain of causes (handlers wrap domain errors
/// in context) that a later run of the same command can get past.
fn try_again(err: &anyhow::Error) -> Option<&DomainError> {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<DomainError>())
        .find(|e| e.is_concurrent_change() || e.is_vault_busy())
}

/// The status `err` exits with when it is not the generic 1.
pub fn special_status(err: &anyhow::Error) -> Option<i32> {
    try_again(err).map(|_| CONCURRENT_CHANGE)
}

/// The `{code, message, details}` object `--json` prints for such an
/// error, in the shape the MCP server returns: `concurrent_change` with the
/// paths that changed (none, with `lock_lost`, when the lock was broken),
/// or `vault_busy` with the lock's holder and age. `None` for any other
/// error.
pub fn rejection(err: &anyhow::Error) -> Option<serde_json::Value> {
    let e = try_again(err)?;
    let (code, details) = match e {
        DomainError::Transaction(TransactionError::Conflict { paths }) => (
            "concurrent_change",
            serde_json::json!({
                "paths": paths.iter().map(ToString::to_string).collect::<Vec<_>>()
            }),
        ),
        DomainError::Transaction(TransactionError::LockLost { .. }) => (
            "concurrent_change",
            serde_json::json!({ "paths": [], "lock_lost": true }),
        ),
        DomainError::Store(StoreError::LockBusy {
            lock,
            owner,
            age_secs,
            stale_after_secs,
            ..
        }) => (
            "vault_busy",
            serde_json::json!({
                "lock": lock,
                "owner": owner,
                "age_secs": age_secs,
                "stale_after_secs": stale_after_secs,
            }),
        ),
        DomainError::Store(StoreError::LockTimeout(waited)) => (
            "vault_busy",
            serde_json::json!({ "lock": ".cuaderno/.lock", "waited_secs": waited.as_secs() }),
        ),
        _ => ("concurrent_change", serde_json::json!({ "paths": [] })),
    };
    Some(serde_json::json!({
        "code": code,
        "message": e.to_string(),
        "details": details,
    }))
}
