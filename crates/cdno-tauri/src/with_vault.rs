//! Run synchronous domain calls off the async command handlers.
//!
//! Port of `cdno-mcp`'s `CuadernoServer::with_vault` (GH #303, PR
//! #307): the domain is deliberately synchronous, commands are async
//! only because the IPC layer wants them to be, and every vault call
//! routes through the blocking pool so disk/SQLite work never sits on
//! the runtime workers. Uses `tauri::async_runtime::spawn_blocking`
//! — runtime-agnostic, unlike `tokio::task::spawn_blocking`, which
//! panics if a future sync command ever runs off-runtime (M1 review
//! condition).

use std::sync::Arc;

use cdno_domain::Vault;

use crate::error::CmdError;

/// Run `f` against the shared vault on the blocking pool.
///
/// The closure returns whatever the call site needs — usually a
/// `Result<T, DomainError>` the caller then lifts with `?` and
/// `From<DomainError>`.
pub async fn with_vault<R>(
    vault: &Arc<Vault>,
    f: impl FnOnce(&Vault) -> R + Send + 'static,
) -> Result<R, CmdError>
where
    R: Send + 'static,
{
    let vault = Arc::clone(vault);
    tauri::async_runtime::spawn_blocking(move || f(&vault))
        .await
        .map_err(|e| {
            // A JoinError almost always means the closure panicked.
            // Contain it as an error response, but never let the
            // panic payload (embedded in the JoinError's Display)
            // reach the client — log it, return a generic message.
            tracing::error!(error = %e, "command panicked on the blocking pool");
            CmdError::Internal("internal error while executing the command".to_owned())
        })
}
