//! Shared helper that turns freshly-written markdown into a
//! [`NoteEntry`] ready for the index. Used by every operation that
//! writes a note: `log_to_daily_note`, `capture_to_inbox`, and (in
//! Phase 2/3) the project, portfolio, and stewardship operations.

use std::time::{SystemTime, UNIX_EPOCH};

use cdno_core::frontmatter::Frontmatter;
use cdno_core::hash::content_hash;
use cdno_core::index::NoteEntry;
use cdno_core::path::VaultPath;

use crate::error::DomainError;

/// Build the [`NoteEntry`] row for a freshly-written note at `path`
/// with the given `content`. The caller supplies `note_type` rather
/// than re-deriving it from the frontmatter — operations always know
/// what type of note they just wrote, and trusting that avoids one
/// more spot where an inconsistent frontmatter could surprise us.
///
/// Timestamps use `SystemTime::now()` — close enough to the post-write
/// filesystem mtime for reconciliation to treat the row as up-to-date
/// on the next pass.
pub(in crate::vault) fn build_index_entry_for(
    path: &VaultPath,
    content: &str,
    note_type: &str,
) -> Result<NoteEntry, DomainError> {
    let (fm, _body) = Frontmatter::parse(content)?;
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    Ok(NoteEntry {
        path: path.clone(),
        note_type: note_type.to_owned(),
        title: None,
        content_hash: content_hash(content),
        mtime_ns: now_ns,
        size: content.len() as u64,
        frontmatter: fm.as_json(),
        indexed_at_ns: now_ns,
    })
}
