//! Action write commands: `start_action` and `complete_action`.
//!
//! The write-command pattern every later module copies: run the
//! domain call on the blocking pool, record the touched paths in the
//! `WriteJournal` (so the watcher suppresses the echo), then emit a
//! precise `vault:changed { origin: self }` for the frontend's
//! invalidation map. Recording happens *after* the commit inside the
//! domain call returned — a failed write must not poison the journal.

use chrono::Local;
use tauri::Emitter;

use crate::error::CmdError;
use crate::events::{Origin, VAULT_CHANGED, VaultArea, VaultChanged};
use crate::state::AppState;
use crate::with_vault::with_vault;
use cdno_core::path::VaultPath;

/// Record `paths` as self-writes and emit the matching
/// `origin: self` change event. Shared by every write command.
pub(crate) fn record_and_emit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    paths: Vec<VaultPath>,
    areas: Vec<VaultArea>,
) {
    let path_strings = paths.iter().map(|p| p.to_string()).collect();
    state.journal.record(paths);
    let _ = app.emit(
        VAULT_CHANGED,
        VaultChanged {
            origin: Origin::SelfWrite,
            areas,
            paths: path_strings,
        },
    );
}

/// Log that work on `action` is starting (`started [[slug]] — …` in
/// today's daily note). The Home view's Start button.
#[tauri::command]
pub async fn start_action<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    project: String,
    action: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let daily = with_vault(&state.vault, move |vault| {
        vault.start_action(now, &project, &action)
    })
    .await??;
    record_and_emit(&app, &state, vec![daily], vec![VaultArea::Daily]);
    Ok(())
}

/// Complete the action bullet matching `action` on `project`
/// (case-insensitive substring; ambiguity comes back as
/// `CmdError::Ambiguous` and the UI shows a picker). Removes the
/// bullet and logs the completion to today's daily note.
#[tauri::command]
pub async fn complete_action<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    project: String,
    action: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let slug = project.clone();
    let project_path = with_vault(&state.vault, move |vault| {
        vault.complete_action(now, &slug, &action)
    })
    .await??;
    // complete_action also stages the daily log line in the same
    // transaction — both paths are ours.
    let daily = daily_path_today();
    record_and_emit(
        &app,
        &state,
        vec![project_path, daily],
        vec![VaultArea::Projects, VaultArea::Daily, VaultArea::Actions],
    );
    Ok(())
}

/// Today's daily-note path, for journalling writes that log as a side
/// effect. Mirrors the domain's `journal/<year>/daily/<date>.md` rule
/// via the shared relpath helper. `#[doc(hidden)] pub` so the IPC
/// integration tests can locate the note a write was expected to
/// touch.
#[doc(hidden)]
pub fn daily_path_today() -> VaultPath {
    let date = Local::now().date_naive();
    VaultPath::new(cdno_core::paths::daily_note_relpath(date))
        .expect("the daily relpath rule always yields a valid vault path")
}
