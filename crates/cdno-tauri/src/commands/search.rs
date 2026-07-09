//! Command palette search (M5, plan §1.0 cmdk palette): `search_vault`
//! wraps the domain full-text search. Pure read — no journal, no
//! events.

use cdno_domain::vault::{SearchFilters, SearchResultEntry};

use crate::error::CmdError;
use crate::state::AppState;
use crate::with_vault::with_vault;

/// Result cap for the palette. The palette shows a short, scannable
/// list — the domain ranks best-first, so the top slice is the useful
/// one; a deeper search is the dedicated (post-v1) search view's job.
const PALETTE_RESULT_LIMIT: usize = 30;

/// Full-text search over note title + body, ranked best-first. Feeds
/// the command palette's result list. `filters` are left at their
/// defaults (no type/date/portfolio narrowing) — the palette is a
/// broad recall surface; a blank or term-less query returns an empty
/// list rather than erroring.
#[tauri::command]
pub async fn search_vault(
    state: tauri::State<'_, AppState>,
    query: String,
) -> Result<Vec<SearchResultEntry>, CmdError> {
    Ok(with_vault(&state.vault(), move |vault| {
        vault.search(&query, &SearchFilters::default(), PALETTE_RESULT_LIMIT)
    })
    .await??)
}
