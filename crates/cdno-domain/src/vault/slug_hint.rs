//! Shared builder for the "— available …: a, b, c" suffix appended to a
//! slug not-found error.
//!
//! When a slug doesn't resolve — most often because an agent driving the
//! MCP server guessed it — the error names the valid set so the caller can
//! self-correct instead of guessing again (the failure that started this:
//! a client invented `fitness` when the real stewardship was `gym`). The
//! per-entity resolvers each pass their own note type and a closure that
//! renders a path into a slug; this keeps the list/sort/format logic in one
//! place.

use cdno_core::index::VaultIndex;
use cdno_core::path::VaultPath;

/// Build " — available {label}: …" from the indexed notes of `note_type`.
///
/// `render` turns a note's path into a `(sort_key, display)` pair, or
/// `None` to skip a malformed path. Results are sorted by `sort_key`
/// (typically the bare slug, so a parenthetical flag like `(parked)` can't
/// perturb the order) and the `display`s are comma-joined.
///
/// Best-effort and index-derived: an index read error or an empty set both
/// yield `""` (no suffix), so the base not-found message is never masked or
/// left with a dangling separator.
pub(in crate::vault) fn available_slugs_hint(
    index: &dyn VaultIndex,
    note_type: &str,
    label: &str,
    render: impl Fn(&VaultPath) -> Option<(String, String)>,
) -> String {
    let Ok(entries) = index.list_by_type(note_type) else {
        return String::new();
    };
    let mut items: Vec<(String, String)> = entries.iter().filter_map(|e| render(&e.path)).collect();
    if items.is_empty() {
        return String::new();
    }
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let rendered: Vec<String> = items.into_iter().map(|(_, display)| display).collect();
    format!(" — available {label}: {}", rendered.join(", "))
}
