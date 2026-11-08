//! Format-dispatching readers over a note's body (RFC 0008 T10).

use crate::markdown;
use crate::path::VaultPath;
use crate::paths::{NoteFormat, note_format};
use crate::typst;

/// Every heading in a note's `body` as `(level, text)`, in document order, by the note's
/// format. A path with no note extension is read as Markdown.
pub fn note_headings(path: &VaultPath, body: &str) -> Vec<(u8, String)> {
    match note_format(path) {
        Some(NoteFormat::Typst) => typst::headings(body),
        _ => markdown::headings(body),
    }
}

/// The text of each heading in a note's `body`, by the note's format.
pub fn note_heading_texts(path: &VaultPath, body: &str) -> Vec<String> {
    note_headings(path, body)
        .into_iter()
        .map(|(_, text)| text)
        .collect()
}
