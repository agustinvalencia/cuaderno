//! `Vault::read_note` — read any vault note for display.
//!
//! The reader surface behind note-preview panes (MCP `read_note`-style
//! consumers, the desktop app's NoteReader): one call returns the
//! parsed frontmatter, the markdown body, and the note type so a
//! renderer never re-implements the frontmatter split.

use cdno_core::extractors::first_h1;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;

use crate::error::DomainError;

use super::Vault;

/// One vault note, split for display: typed metadata from the index,
/// frontmatter as JSON, and the markdown body ready for rendering.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
pub struct NoteView {
    /// Serialised as a plain string over the wire (VaultPath's Display
    /// form).
    #[cfg_attr(feature = "ts-bindings", ts(type = "string"))]
    pub path: VaultPath,
    /// The index row's note type (e.g. `"project"`, `"tracking"`), or
    /// `None` when the file isn't indexed — attachments, ignored
    /// files, or a note written since the last reconcile.
    pub note_type: Option<String>,
    /// Text of the body's first ATX H1, when present.
    pub title: Option<String>,
    /// Full frontmatter as JSON. `null` when the file has no parseable
    /// frontmatter block. Typed as `unknown` on the wire — the renderer
    /// narrows per note type.
    #[cfg_attr(feature = "ts-bindings", ts(type = "unknown"))]
    pub frontmatter: serde_json::Value,
    /// Markdown body after the closing `---` (or the whole file when
    /// there is no frontmatter block).
    pub body: String,
}

impl Vault {
    /// Read the note at `path` for display.
    ///
    /// The file is read fresh from the store — markdown is the source
    /// of truth — while `note_type` comes from the index row (startup
    /// reconciliation keeps that current). A file without a parseable
    /// frontmatter block is still returned, with `frontmatter: null`
    /// and the whole content as the body: the reader surface must be
    /// able to show hand-edited oddities rather than erroring on them.
    ///
    /// Errors with `Store(NotFound)` when no file exists at `path`.
    pub fn read_note(&self, path: &VaultPath) -> Result<NoteView, DomainError> {
        let raw = self.store.read_file(path)?;
        let note_type = self.index.find_by_path(path)?.map(|entry| entry.note_type);
        let (frontmatter, body) = match Frontmatter::parse(&raw) {
            Ok((fm, body)) => (fm.as_json(), body.to_owned()),
            Err(_) => (serde_json::Value::Null, raw),
        };
        let title = first_h1(&body);
        Ok(NoteView {
            path: path.clone(),
            note_type,
            title,
            frontmatter,
            body,
        })
    }
}
