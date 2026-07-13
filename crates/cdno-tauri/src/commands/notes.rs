//! Note-reader reads (M5, plan §1.0 NoteReader / §3.8 wikilinks):
//! `read_note` feeds the slide-in reader panel, `resolve_wikilink`
//! turns a clicked `[[target]]` into typed navigation. Both are pure
//! reads — no journal, no events.

use std::path::{Component, Path, PathBuf};

use base64::Engine;
use cdno_core::path::VaultPath;
use cdno_domain::vault::{NoteView, ResolvedLink};

use crate::commands::actions::record_and_emit;
use crate::error::CmdError;
use crate::events::classify;
use crate::state::AppState;
use crate::with_vault::with_vault;

/// Resolve a markdown image `src` (relative to the note that embeds it)
/// into a vault path. `../` segments pop within the vault and can never
/// escape it — a pop past the root is a no-op, so the result is always
/// vault-relative — and an absolute/rooted `src` is rejected (`None`). The
/// returned `PathBuf` is still passed through `VaultPath::new`, whose
/// newtype guard is the actual confinement boundary.
pub fn resolve_asset_path(note_path: &str, src: &str) -> Option<PathBuf> {
    let mut out = Path::new(note_path)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf();
    for component in Path::new(src).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(segment) => out.push(segment),
            // A rooted or prefixed `src` is an absolute reference, not an
            // in-vault asset — refuse rather than guess.
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// The `data:` MIME type for an asset, by extension. Unknown extensions
/// fall back to `application/octet-stream` — the browser then declines to
/// render it rather than mis-sniffing.
pub fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

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

/// Read a note's RAW markdown (frontmatter block and all) for the
/// in-app editor — the exact bytes, so an edit round-trip never reformats
/// what the author wrote. Pure read. Missing file → `NotFound`;
/// `..`/absolute → `Invalid`.
#[tauri::command]
pub async fn read_note_raw(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<String, CmdError> {
    let vault_path = parse_vault_path(path)?;
    Ok(with_vault(&state.vault(), move |vault| {
        vault.read_note_raw(&vault_path)
    })
    .await??)
}

/// Overwrite a note with `content` (free "posture B" editing) and reindex.
/// Journals the write (so the watcher suppresses its own echo) and emits a
/// change for the edited note's area, so backlinks and other surfaces
/// refetch. The domain reconciles after the write, so the index follows
/// the new bytes; `cdno lint` is the separate guardrail for a note the
/// edit made invalid.
#[tauri::command]
pub async fn write_note_raw<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    path: String,
    content: String,
) -> Result<(), CmdError> {
    let vault_path = parse_vault_path(path)?;
    let journalled = vault_path.clone();
    with_vault(&state.vault(), move |vault| {
        vault.write_note_raw(&vault_path, &content)
    })
    .await??;
    // Best-effort area from the path's first segment; the reader
    // invalidates its own read on save, so this is for sibling surfaces.
    let areas = classify(&journalled).into_iter().collect();
    record_and_emit(&app, &state, vec![journalled], areas);
    Ok(())
}

/// Read an image embedded in a note and return it as a `data:` URI the
/// webview can render inline. `src` is the raw markdown image target
/// (relative to `note_path`, e.g. `assets/fig.png`); it is resolved against
/// the note's folder and confined by `VaultPath` (so it cannot read outside
/// the vault). The bytes are base64-encoded into a `data:<mime>;base64,…`
/// string — the app CSP already allows `data:`, so no custom protocol or
/// asset scope is needed. Missing file → `NotFound`; an absolute/rooted
/// `src` or a `..`/absolute resolved path → `Invalid`.
#[tauri::command]
pub async fn read_note_asset(
    state: tauri::State<'_, AppState>,
    note_path: String,
    src: String,
) -> Result<String, CmdError> {
    let resolved = resolve_asset_path(&note_path, &src)
        .ok_or_else(|| CmdError::Invalid(format!("asset src is not vault-relative: {src}")))?;
    let mime = mime_for(&resolved);
    let vault_path = VaultPath::new(&resolved).map_err(|e| CmdError::Invalid(e.to_string()))?;
    let bytes = with_vault(&state.vault(), move |vault| {
        vault.read_asset_bytes(&vault_path)
    })
    .await??;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
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
