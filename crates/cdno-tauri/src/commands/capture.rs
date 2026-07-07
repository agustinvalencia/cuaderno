//! Global-capture write commands plus the inbox read/discard/open
//! surface (M3). `capture_quick` and `log_quick` are the two verbs the
//! floating capture window fires; `list_inbox` / `discard_inbox_item`
//! back the inbox drawer; `open_in_editor` hands a note off to the
//! user's default editor.
//!
//! The write commands follow the module's established pattern (see
//! `actions.rs`): run the domain call on the blocking pool, then
//! `record_and_emit` a precise `origin: self` change event so the
//! watcher suppresses its own echo.

use chrono::Local;

use cdno_core::path::VaultPath;
use cdno_domain::InboxItem;
use tauri_plugin_opener::OpenerExt;

use crate::commands::actions::{daily_path_for, record_and_emit};
use crate::error::CmdError;
use crate::events::VaultArea;
use crate::state::AppState;
use crate::with_vault::with_vault;

/// Capture `text` into `inbox/` as a fresh note. The capture window's
/// Enter verb — the zero-friction landing place for a thought.
#[tauri::command]
pub async fn capture_quick<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    text: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    // The domain returns the vault-relative path of the note it wrote,
    // so we journal exactly what we touched (no path reconstruction).
    let path = with_vault(&state.vault, move |vault| {
        vault.capture_to_inbox(now, &text)
    })
    .await??;
    record_and_emit(&app, &state, vec![path], vec![VaultArea::Inbox]);
    Ok(())
}

/// Append `text` to today's daily-log section. The capture window's
/// Cmd/Ctrl+Enter verb — for a thought that belongs on the timeline
/// rather than in the triage queue.
#[tauri::command]
pub async fn log_quick<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    text: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let daily = with_vault(&state.vault, move |vault| {
        vault.log_to_daily_note(now, &text)
    })
    .await??;
    record_and_emit(&app, &state, vec![daily], vec![VaultArea::Daily]);
    Ok(())
}

/// Every uncategorised capture under `inbox/`, oldest first — the
/// inbox drawer's data. A pure read: no journal, no emit.
#[tauri::command]
pub async fn list_inbox(state: tauri::State<'_, AppState>) -> Result<Vec<InboxItem>, CmdError> {
    let items = with_vault(&state.vault, |vault| vault.list_inbox()).await??;
    Ok(items)
}

/// Hard-delete the inbox capture identified by `slug` (the filename
/// stem from [`list_inbox`]). The domain preserves the captured text
/// in today's daily-log line in the same commit, so both the deleted
/// note and the daily are ours to journal.
#[tauri::command]
pub async fn discard_inbox_item<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    slug: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    // Derive the journalled daily path from the SAME instant the domain
    // call received, so a discard a hair before midnight journals the
    // day it wrote to (the midnight TOCTOU — see actions.rs).
    let date = now.date();
    let path = with_vault(&state.vault, move |vault| {
        vault.discard_inbox_item(now, &slug)
    })
    .await??;
    let daily = daily_path_for(date);
    record_and_emit(
        &app,
        &state,
        vec![path, daily],
        vec![VaultArea::Inbox, VaultArea::Daily],
    );
    Ok(())
}

/// Open a vault note in the user's default editor (read-only intent —
/// this command only hands a path to the opener; it never writes).
/// `path` is vault-relative. This is the one file access that bypasses
/// the domain layer, so confinement is load-bearing and enforced in two
/// layers:
///
/// 1. **Lexical** ([`VaultPath::new`]): rejects absolute paths and `..`
///    escapes before the path is joined to the vault root.
/// 2. **Symlink-canonical**: the lexical check follows no links, so a
///    symlink *inside* the vault could still point outside it. We
///    canonicalise both the resolved path and the root and require the
///    former to sit under the latter — the guarantee is that the file
///    actually opened resolves inside the vault, not merely that its
///    spelling looked clean.
#[tauri::command]
pub async fn open_in_editor<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<(), CmdError> {
    let rel = VaultPath::new(&path).map_err(|e| CmdError::Invalid(e.to_string()))?;
    let abs = state.root.join(rel.as_path());
    // Mirrors FsVaultStore's confinement posture (`check_confinement`
    // in cdno-core), which we can't call directly: it's private, and
    // this open deliberately bypasses the store. Fail closed — a
    // canonicalize error (missing file, dangling symlink, or a root
    // that isn't on disk) is a refusal, never a fallthrough, so a
    // symlink can't smuggle us outside the vault.
    let outside = || CmdError::Invalid("path resolves outside the vault".to_owned());
    let canon = std::fs::canonicalize(&abs).map_err(|_| outside())?;
    let root_canon = std::fs::canonicalize(&state.root).map_err(|_| outside())?;
    if !canon.starts_with(&root_canon) {
        return Err(outside());
    }
    // Called from Rust, so the opener plugin's JS ACL never applies —
    // no `opener:*` capability is required for this path.
    app.opener()
        .open_path(canon.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to open note in the default editor");
            CmdError::Internal("could not open the note in an editor".to_owned())
        })?;
    Ok(())
}
