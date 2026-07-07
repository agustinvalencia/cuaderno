//! Action write commands: `start_action` and `complete_action`.
//!
//! The write-command pattern every later module copies: run the
//! domain call on the blocking pool, record the touched paths in the
//! `WriteJournal` (so the watcher suppresses the echo), then emit a
//! precise `vault:changed { origin: self }` for the frontend's
//! invalidation map. Recording happens *after* the commit inside the
//! domain call returned — a failed write must not poison the journal.

use chrono::{Local, NaiveDate};
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
    // A dropped invalidation event leaves the frontend showing stale
    // data until the next reload — rare (the channel is in-process) but
    // silent, so surface it in the log rather than swallowing the Err.
    if let Err(err) = app.emit(
        VAULT_CHANGED,
        VaultChanged {
            origin: Origin::SelfWrite,
            areas,
            paths: path_strings,
        },
    ) {
        tracing::warn!(error = %err, "failed to emit vault:changed after a write");
    }
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
    // Derive the journalled daily path from the SAME instant the domain
    // call received, not a fresh `Local::now()` afterwards: a completion
    // that lands a hair before midnight must journal the day it wrote to,
    // not the day the read-back happens (the midnight TOCTOU).
    let date = now.date();
    let slug = project.clone();
    let project_path = with_vault(&state.vault, move |vault| {
        vault.complete_action(now, &slug, &action)
    })
    .await??;
    // complete_action stages the daily log line in the same transaction,
    // so the daily is ours to journal. When the completed bullet
    // wikilinks an action note, the domain ALSO archives
    // `actions/<slug>.md` to `actions/_done/<year>/<slug>.md` in that
    // same transaction — those paths are ours too, but the domain
    // returns only the project path, so we cannot journal them here. The
    // watcher will echo those archive writes back as external changes and
    // trigger a redundant refetch. Fixing this needs the domain to return
    // its full touched-path set — see #315.
    let daily = daily_path_for(date);
    record_and_emit(
        &app,
        &state,
        vec![project_path, daily],
        vec![VaultArea::Projects, VaultArea::Daily, VaultArea::Actions],
    );
    Ok(())
}

/// The daily-note path for `date`, for journalling writes that log to
/// the daily as a side effect. Mirrors the domain's
/// `journal/<year>/daily/<date>.md` rule via the shared relpath helper.
/// Callers pass the same `date` the domain call used, so the journalled
/// path can't drift across a midnight boundary between the write and the
/// path reconstruction. `#[doc(hidden)] pub` so the IPC integration
/// tests can locate the note a write was expected to touch.
#[doc(hidden)]
pub fn daily_path_for(date: NaiveDate) -> VaultPath {
    VaultPath::new(cdno_core::paths::daily_note_relpath(date))
        .expect("the daily relpath rule always yields a valid vault path")
}
