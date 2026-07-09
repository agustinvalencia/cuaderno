//! Note-reader reads (M5, plan §1.0 NoteReader / §3.8 wikilinks):
//! `read_note` feeds the slide-in reader panel, `resolve_wikilink`
//! turns a clicked `[[target]]` into typed navigation. Both are pure
//! reads — no journal, no events.

use cdno_core::path::VaultPath;
use cdno_domain::vault::{NoteView, ResolvedLink};

use crate::error::CmdError;
use crate::state::AppState;
use crate::with_vault::with_vault;

/// Parse a wire path string into a `VaultPath`, mapping the lexical
/// guard's rejection (absolute paths, `..` escapes) to a user-visible
/// `Invalid` rather than a generic failure.
fn parse_vault_path(path: String) -> Result<VaultPath, CmdError> {
    VaultPath::new(path).map_err(|e| CmdError::Invalid(e.to_string()))
}

/// Read any vault note for display: parsed frontmatter, markdown body,
/// note type, and title. Backs the NoteReader panel reused across the
/// Actions view, timelines, and Portfolio Browser. Missing file →
/// `NotFound`; a `..`/absolute path → `Invalid` (the VaultPath guard).
#[tauri::command]
pub async fn read_note(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<NoteView, CmdError> {
    let vault_path = parse_vault_path(path)?;
    // `.await??`: the outer `?` unwraps `with_vault`'s own error, the
    // inner converts the `DomainError` into `CmdError` via `From`.
    Ok(with_vault(&state.vault(), move |vault| vault.read_note(&vault_path)).await??)
}

/// Resolve a single clicked wikilink `target` to its note (path +
/// note_type) for typed navigation. `Ok(None)` when the target matches
/// no note or is ambiguous — the frontend renders those as a muted,
/// un-clickable span (plan §3.8).
#[tauri::command]
pub async fn resolve_wikilink(
    state: tauri::State<'_, AppState>,
    target: String,
) -> Result<Option<ResolvedLink>, CmdError> {
    Ok(with_vault(&state.vault(), move |vault| vault.resolve_wikilink(&target)).await??)
}
