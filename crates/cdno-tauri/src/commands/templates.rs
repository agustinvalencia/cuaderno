//! Templates view (#357): browse every note type's template, read its
//! effective content, edit-and-save a custom override, and scaffold a
//! starter for a config-defined custom type that has none.
//!
//! Reads (`list_templates`, `read_template`, `list_template_placeholders`)
//! are pure — no journal, no events. The two writes (`save_template`,
//! `create_template`) follow the module's write pattern (see `actions.rs`):
//! run the domain call on the blocking pool, then `record_and_emit` the
//! touched template path as a `VaultArea::Config` self-write so the app's
//! own edit is journalled (the watcher suppresses its echo) while the
//! frontend's invalidation map still refreshes the view.
//!
//! Templates are config files, not append-only notes, so the domain writes
//! them with a plain confined `store.write_file` — no `VaultTransaction`.

use cdno_domain::vault::{TemplateContent, TemplatePlaceholder, TemplateSummary};

use crate::commands::actions::record_and_emit;
use crate::error::CmdError;
use crate::events::VaultArea;
use crate::state::AppState;
use crate::with_vault::with_vault;

/// Every note type and the status of its template — the Templates list.
/// A pure read: no journal, no emit.
#[tauri::command]
pub async fn list_templates(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<TemplateSummary>, CmdError> {
    let summaries = with_vault(&state.vault, |vault| vault.list_templates()).await??;
    Ok(summaries)
}

/// The effective content of `note_type`'s template (custom override if
/// present, else the built-in default; a synthesised starter for a custom
/// type with no file) plus its source rung. A pure read. `variant` is
/// optional — the list view always reads the base template (`None`).
#[tauri::command]
pub async fn read_template(
    state: tauri::State<'_, AppState>,
    note_type: String,
    variant: Option<String>,
) -> Result<TemplateContent, CmdError> {
    let content = with_vault(&state.vault, move |vault| {
        vault.read_template(&note_type, variant.as_deref())
    })
    .await??;
    Ok(content)
}

/// The full placeholder set `note_type` supports, for the editor's
/// reference panel and its unknown-token check — built-in supplied keys,
/// a config custom type's declared schema fields, and config
/// `[variables]` / `[variables.prompt]`. A pure read.
#[tauri::command]
pub async fn list_template_placeholders(
    state: tauri::State<'_, AppState>,
    note_type: String,
) -> Result<Vec<TemplatePlaceholder>, CmdError> {
    let placeholders = with_vault(&state.vault, move |vault| {
        vault.template_placeholders(&note_type)
    })
    .await??;
    Ok(placeholders)
}

/// Save `content` verbatim as the custom template for `note_type` (+
/// optional `variant`). Transparently creates the override for a
/// built-in-backed type on first save (the edit-and-save model). Journals
/// the touched template path as a `Config` self-write.
#[tauri::command]
pub async fn save_template<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    note_type: String,
    variant: Option<String>,
    content: String,
) -> Result<(), CmdError> {
    let path = with_vault(&state.vault, move |vault| {
        vault.save_template(&note_type, variant.as_deref(), &content)
    })
    .await??;
    record_and_emit(&app, &state, vec![path], vec![VaultArea::Config]);
    Ok(())
}

/// Scaffold a starter template for a config-defined custom type that has
/// none yet. A built-in type is rejected (edit-and-save its override via
/// `save_template` instead). Journals the written path as a `Config`
/// self-write.
#[tauri::command]
pub async fn create_template<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    note_type: String,
) -> Result<(), CmdError> {
    let path = with_vault(&state.vault, move |vault| vault.create_template(&note_type)).await??;
    record_and_emit(&app, &state, vec![path], vec![VaultArea::Config]);
    Ok(())
}
