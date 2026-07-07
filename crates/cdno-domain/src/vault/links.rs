//! `Vault::resolve_wikilink` — single-target wikilink resolution for
//! UI navigation.
//!
//! The batch resolver [`cdno_core::extractors::resolve_wikilinks`]
//! resolves the wikilinks of one note against the whole vault during
//! reconciliation. The desktop app needs the *inverse* shape: a user
//! clicked one `[[target]]` in a rendered note, and we resolve just
//! that target so the frontend can navigate to it. Rather than
//! re-implement the matching rules (and risk drift), this delegates to
//! the same batch function with a one-element input, so the two paths
//! always agree — including on ambiguity.

use std::collections::HashSet;

use cdno_core::extractors::{WikilinkRaw, resolve_wikilinks};
use cdno_core::path::VaultPath;

use crate::error::DomainError;

use super::Vault;

/// The resolved destination of a single wikilink target, for typed UI
/// navigation. `path` is the vault-relative note the target points at;
/// `note_type` is the index row's type (`"project"`, `"stewardship"`,
/// …) so the frontend can route to a typed view rather than always
/// falling back to the generic note reader. `note_type` is `None` when
/// the resolved file has no index row (written since the last
/// reconcile, or an ignored file).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
pub struct ResolvedLink {
    /// Serialised as a plain string over the wire (VaultPath's Display
    /// form) — the same shape every other path field promises.
    #[cfg_attr(feature = "ts-bindings", ts(type = "string"))]
    pub path: VaultPath,
    pub note_type: Option<String>,
}

impl Vault {
    /// Resolve a single wikilink `target` (the text inside `[[…]]`,
    /// without the `|label`) against the vault index.
    ///
    /// Returns `Ok(None)` for an unresolved target — the UI renders it
    /// as a muted, un-clickable span. The resolution rules are exactly
    /// the batch resolver's (see
    /// [`cdno_core::extractors::resolve_wikilinks`]): an exact
    /// `<target>.md` path match wins first, then a unique file-stem
    /// (last path segment) match. **Ambiguity resolves to `None`**: when
    /// two or more notes share the stem the target names, the link is
    /// left unresolved rather than guessing — resolution stays *sound*
    /// (a stem collision never navigates to the wrong note). This
    /// mirrors the batch resolver's behaviour byte-for-byte because it
    /// *is* the batch resolver, called with a one-element input.
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
        let raw = WikilinkRaw {
            target: target.to_owned(),
            label: None,
        };
        // One-element batch: exactly one LinkEntry comes back.
        let resolved = resolve_wikilinks(vec![raw], &paths)
            .into_iter()
            .next()
            .and_then(|entry| entry.resolved_path);
        let Some(path) = resolved else {
            return Ok(None);
        };

        let note_type = self.index.find_by_path(&path)?.map(|entry| entry.note_type);
        Ok(Some(ResolvedLink { path, note_type }))
    }
}
