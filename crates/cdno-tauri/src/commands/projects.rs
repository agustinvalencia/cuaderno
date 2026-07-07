//! Project write commands — M2 ships `update_project_state` (the
//! Home card's inline Current State editor).

use chrono::Local;

use crate::error::CmdError;
use crate::events::VaultArea;
use crate::state::AppState;
use crate::with_vault::with_vault;

use super::actions::{daily_path_for, record_and_emit};

/// Rewrite a project's `## Current State`. The domain auto-logs the
/// previous state to today's daily entry in the same transaction —
/// free history — and no-ops silently when the text is unchanged.
#[tauri::command]
pub async fn update_project_state<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    project: String,
    new_state: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    // Journal the daily for the same instant the domain call received,
    // so a state update straddling midnight records the day it wrote to
    // rather than the day the path is reconstructed (midnight TOCTOU).
    let date = now.date();
    let project_path = with_vault(&state.vault, move |vault| {
        vault.update_project_state(now, &project, &new_state)
    })
    .await??;
    let daily = daily_path_for(date);
    record_and_emit(
        &app,
        &state,
        vec![project_path, daily],
        vec![VaultArea::Projects, VaultArea::Daily],
    );
    Ok(())
}
