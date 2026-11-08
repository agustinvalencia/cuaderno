//! Format-dispatching readers over a note's body (RFC 0008 T10).

use crate::extractors::first_h1;
use crate::frontmatter::Frontmatter;
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

/// The title a note's full-text row carries: a Typst note's block `title`, never a guess from its
/// markup (§5.3); a Markdown note's H1, which is where it carries its title. `frontmatter` is
/// `None` when the note did not parse.
pub fn fts_title(
    path: &VaultPath,
    frontmatter: Option<&Frontmatter>,
    body: &str,
) -> Option<String> {
    match note_format(path) {
        Some(NoteFormat::Typst) => frontmatter?.optional_field("title").ok().flatten(),
        _ => first_h1(body),
    }
}
