//! The exit status of a write refused because its note changed underneath
//! it (#1001).
//!
//! `cdno` exits 0 on success and 1 on a failure, clap exits 2 on a usage
//! error, and `cdno open` passes the editor's own status through. This
//! module adds one more: 75 for a write that wrote nothing because a file
//! it would replace changed while it was being prepared, the one failure
//! where running the same command again is the remedy.

use cdno_domain::error::DomainError;

/// The status for a write refused because a note changed underneath it
/// while it was being prepared (#1001): nothing was written, and running
/// the same command again is safe. 75 is `EX_TEMPFAIL` from `sysexits.h`,
/// "try again later". `cdno log` returns it only after its own retries, so
/// a script that retries on it should bound its loop.
pub const CONCURRENT_CHANGE: i32 = 75;

/// The concurrent change in `err`'s chain of causes (handlers wrap domain
/// errors in context), if there is one.
fn concurrent_change(err: &anyhow::Error) -> Option<&DomainError> {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<DomainError>())
        .find(|e| e.is_concurrent_change())
}

/// The status `err` exits with when it is not the generic 1.
pub fn special_status(err: &anyhow::Error) -> Option<i32> {
    concurrent_change(err).map(|_| CONCURRENT_CHANGE)
}

/// The `{code, message, details}` object `--json` prints for a concurrent
/// change, the shape the MCP server returns (`concurrent_change`, with the
/// paths that changed), or `None` for any other error.
pub fn rejection(err: &anyhow::Error) -> Option<serde_json::Value> {
    let e = concurrent_change(err)?;
    let paths: Vec<String> = match e {
        DomainError::Transaction(cdno_core::error::TransactionError::Conflict { paths }) => {
            paths.iter().map(ToString::to_string).collect()
        }
        _ => Vec::new(),
    };
    Some(serde_json::json!({
        "code": "concurrent_change",
        "message": e.to_string(),
        "details": { "paths": paths },
    }))
}
