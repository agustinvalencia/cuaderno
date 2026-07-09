//! Config inspector (#365, PR1) + live reload (#365, PR2).
//!
//! `read_config`/`validate_config` are pure reads — no journal, no
//! events — mirroring the Templates read posture. `read_config` hands the
//! UI the verbatim file plus a content hash (the hash is inert until PR3's
//! compare-and-swap write). `validate_config` runs the exact composition
//! `Vault::new` performs (`toml::from_str` → `ignore_set` →
//! `TypeRegistry::validate`) against a candidate string, reporting
//! `Ok(())` or a structured `{ message, line?, col? }` — the same error
//! type the PR3 save gate reuses, so the dry-run and the real gate cannot
//! drift.
//!
//! `reload_config` is PR2's reload PLUMBING: it re-reads config.toml from
//! disk, rebuilds the `Vault` on the SAME store/index, and atomically
//! swaps it into `AppState`. No config WRITE happens here (that is PR3);
//! this command exists so a later save can apply a config edit live, and
//! so the swap is manually testable now.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::vault::{ConfigDocument, ConfigValidationError, validate_config_str};
use tauri::{Emitter, Manager};

use crate::error::CmdError;
use crate::events::{Origin, VAULT_CHANGED, VaultChanged};
use crate::state::AppState;
use crate::watcher::all_areas;
use crate::with_vault::with_vault;

/// Read the raw config document (content + hash). Public and
/// synchronous — the test seam, exercised directly over the Memory
/// doubles.
pub fn read_config_impl(vault: &Vault) -> Result<ConfigDocument, CmdError> {
    Ok(vault.read_config_raw()?)
}

/// The raw `.cuaderno/config.toml` text plus its content hash — the
/// inspector's read. A pure read: no journal, no emit.
#[tauri::command]
pub async fn read_config(state: tauri::State<'_, AppState>) -> Result<ConfigDocument, CmdError> {
    with_vault(&state.vault(), read_config_impl).await?
}

/// Dry-run the config validation `Vault::new` runs against `content`,
/// without touching the vault. `Ok(())` means the config would open;
/// `Err` carries a human-readable message (and, for a TOML syntax
/// error, the line/column). A pure read — it depends only on its input,
/// not on the open vault, so it never blocks the vault lock.
#[tauri::command]
pub async fn validate_config(content: String) -> Result<(), ConfigValidationError> {
    // The check is a small in-memory parse + validation pass; it doesn't
    // touch the store or index, so it runs inline rather than through
    // `with_vault` (there is no vault to borrow).
    validate_config_str(&content)
}

/// Reload `.cuaderno/config.toml` from disk and swap the live vault
/// (#365, PR2). The reload plumbing behind a later config save:
///
/// 1. Re-read the config from `state.root` and rebuild the `Vault` on the
///    SAME store/index — no SQLite reopen. `Vault::new` re-runs the full
///    open-time safety net (`ignore_set` + `TypeRegistry::validate` +
///    reconcile), so a config that would not open is caught here.
/// 2. On success, `ArcSwap::store` the new vault. Commands already running
///    against the old vault finish cleanly — each holds an owned `Arc`
///    snapshot from `state.vault()`, so the swap never pulls a vault out
///    from under an in-flight call.
/// 3. Emit an all-areas `vault:changed` so the frontend refetches
///    everything the new config might have changed (note types, schemas,
///    folders).
///
/// Belt-and-braces (non-negotiable, design §safety-invariants): on ANY
/// rebuild error the swap is SKIPPED and the error returned — the OLD
/// vault stays live, so a bad on-disk config can never leave the session
/// vault-less. This is the last safety net beneath PR3's pre-write
/// validate gate.
///
/// The blocking rebuild (`VaultConfig::load` + `Vault::new`, both
/// synchronous disk/SQLite work) runs on the blocking pool so it never
/// stalls the async runtime — same posture as `with_vault`.
///
/// NOTE (deferred to #365 PR4): the watcher thread holds an `IgnoreSet`
/// compiled at bootstrap in its deps. A reload that changes the `ignore`
/// globs does NOT refresh that matcher here — the watcher keeps using the
/// original set until PR4 makes it swappable. PR2 covers only the vault
/// swap; the reconcile inside `Vault::new` above already uses the fresh
/// ignore set, so the index itself stays correct.
pub async fn reload_vault_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), CmdError> {
    // Clone the handles the rebuild needs out of managed state before the
    // blocking hop — `tauri::State` is not `Send`, but these owned clones
    // (cheap `Arc`s + a `PathBuf`) are.
    let (store, index, root) = {
        let state = app.state::<AppState>();
        (state.store.clone(), state.index.clone(), state.root.clone())
    };

    let new_vault = tauri::async_runtime::spawn_blocking(move || -> Result<Vault, DomainError> {
        // Re-read config.toml from disk (a missing file falls back to the
        // default config, matching `open_vault`'s first launch behaviour).
        let config = VaultConfig::load(&root)?;
        // Rebuild on the retained store/index; discard the reconciliation
        // report (the swap doesn't surface scan counts).
        let (vault, _report) = Vault::new(store, index, config)?;
        Ok(vault)
    })
    .await
    .map_err(|e| {
        // A JoinError almost always means the rebuild closure panicked;
        // contain it, never leak the panic payload across the bridge.
        tracing::error!(error = %e, "vault reload panicked on the blocking pool");
        CmdError::Internal("internal error while reloading the config".to_owned())
    })??;

    // Rebuild succeeded — only now do we swap. Reaching this line means the
    // new config passed the same validation `Vault::new` runs at open, so
    // the swapped-in vault is sound by construction.
    app.state::<AppState>().vault.store(Arc::new(new_vault));

    // A config change can touch any view, so invalidate every area.
    if let Err(err) = app.emit(
        VAULT_CHANGED,
        VaultChanged {
            // This process performed the reload; there is no external
            // writer to distinguish, and no self-write path to journal
            // (the reload wrote no files — it only re-read config).
            origin: Origin::SelfWrite,
            areas: all_areas(),
            paths: Vec::new(),
        },
    ) {
        tracing::warn!(error = %err, "failed to emit vault:changed after a config reload");
    }
    Ok(())
}

/// Reload the vault's config live (#365, PR2). Thin `#[tauri::command]`
/// over [`reload_vault_config`]; returns unit on success, or a
/// `CmdError` (with the old vault left live) if the on-disk config will
/// not open.
#[tauri::command]
pub async fn reload_config<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), CmdError> {
    reload_vault_config(&app).await
}
