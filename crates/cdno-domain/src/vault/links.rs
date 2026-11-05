//! `Vault::resolve_wikilink` — single-target wikilink resolution for
//! UI navigation.
//!
//! The batch resolver [`cdno_core::extractors::resolve_wikilinks`]
//! resolves the wikilinks of one note against the whole vault during
//! reconciliation. The desktop app needs the *inverse* shape: a user
//! clicked one `[[target]]` in a rendered note, and we resolve just
//! that target so the frontend can navigate to it. Rather than
//! re-implement the matching rules (and risk drift), this delegates to
//! the same batch function with a one-element input and the same alias
//! map the index resolves against, so the two paths always agree —
//! including on ambiguity and on aliases (RFC 0007 §5.5).

use std::collections::HashSet;

use cdno_core::extractors::{AliasMap, WikilinkRaw, resolve_wikilinks};
use cdno_core::path::VaultPath;

use crate::error::DomainError;

use super::Vault;

/// The resolved destination of a single wikilink target, for typed UI
/// navigation. `path` is the vault-relative note the target points at;
/// `note_type` is the index row's type (`"project"`, `"stewardship"`,
/// …) so the frontend can route to a typed view rather than always
/// falling back to the generic note reader. `note_type` is virtually
/// always `Some`: the target was resolved *from* `list_all_paths`, which
/// only returns indexed notes, so a matching index row is expected. It
/// is `None` only as a race artefact — the row was removed between the
/// path listing and the type lookup — not because ignored files leak
/// through (they aren't in `list_all_paths` to begin with, so they can
/// never resolve here).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ResolvedLink {
    /// Serialised as a plain string over the wire (VaultPath's Display
    /// form) — the same shape every other path field promises.
    pub path: VaultPath,
    pub note_type: Option<String>,
    /// The target reached the note through one of its `aliases:` entries
    /// (an old name) rather than its path or stem.
    pub via_alias: bool,
}

impl Vault {
    /// Resolve a single wikilink `target` (the text inside `[[…]]`,
    /// without the `|label`) against the vault index.
    ///
    /// Returns `Ok(None)` for an unresolved target — the UI renders it
    /// as a muted, un-clickable span. The resolution rules are exactly
    /// the batch resolver's (see
    /// [`cdno_core::extractors::resolve_wikilinks`]): an exact
    /// `<target>.md` path match wins first, then a folder-index match
    /// (`<target>/_index.md`, for a folder-backed note like a portfolio or
    /// expanded stewardship), then a unique file-stem (last path segment)
    /// match, and, only when no note has that stem, a slug-shaped alias
    /// exactly one note declares (`via_alias` is then set). **Ambiguity
    /// resolves to `None`**: when two or more notes share the stem the
    /// target names, or the alias, the link is left unresolved rather than
    /// guessing — resolution stays *sound* (a collision never navigates to
    /// the wrong note). This mirrors the batch resolver's behaviour
    /// byte-for-byte because it *is* the batch resolver, called with a
    /// one-element input and the index's alias map.
    ///
    /// A blank target short-circuits to `Ok(None)` without touching the
    /// index.
    pub fn resolve_wikilink(&self, target: &str) -> Result<Option<ResolvedLink>, DomainError> {
        let target = target.trim();
        if target.is_empty() {
            return Ok(None);
        }

        // The batch resolver matches against the full set of known vault
        // paths; `list_all_paths` is the cheapest existing query for it
        // (one indexed column scan, no file reads).
        let paths: HashSet<VaultPath> = self.index.list_all_paths()?.into_iter().collect();
        let aliases = AliasMap::from_rows(self.index.list_aliases()?);
        let raw = WikilinkRaw {
            target: target.to_owned(),
            label: None,
            // Resolving a clicked target, not scanning source — the embed
            // distinction is irrelevant to resolution and unused here.
            is_embed: false,
        };
        // One-element batch: exactly one LinkEntry comes back.
        let Some(entry) = resolve_wikilinks(vec![raw], &paths, &aliases)
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        let Some(path) = entry.resolved_path else {
            return Ok(None);
        };

        let note_type = self.index.find_by_path(&path)?.map(|entry| entry.note_type);
        Ok(Some(ResolvedLink {
            path,
            note_type,
            via_alias: entry.via_alias,
        }))
    }
}
