//! Filename-friendly slugs derived from human text. Shared between
//! `capture` and `projects` (and any future op that needs to derive
//! a stable filename from a title).

use std::collections::HashSet;

use cdno_core::error::StoreError;

use crate::error::DomainError;

use super::Vault;

/// Maximum number of words kept from the source text. Six is enough to
/// be recognisable without producing absurdly long filenames.
pub(in crate::vault) const SLUG_MAX_WORDS: usize = 6;

/// Hard char cap on the slug. A single very long word still gets
/// truncated so a pathological input can't blow filesystem name limits.
pub(in crate::vault) const SLUG_MAX_CHARS: usize = 50;

/// Build a slug from the first words of `text`: lowercase
/// alphanumerics joined by `-`, capped to [`SLUG_MAX_WORDS`] /
/// [`SLUG_MAX_CHARS`] so the filename stays manageable. Returns
/// `"untitled"` if the text contains no alphanumerics.
///
/// Public so callers that must resolve the same template *variant* the
/// domain will (e.g. the CLI deriving the tracking activity variant for
/// [`Vault::template_prompts`](crate::Vault::template_prompts)) share one
/// slug rule rather than reimplementing it and drifting.
pub fn slugify(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().take(SLUG_MAX_WORDS).collect();
    if words.is_empty() {
        return "untitled".to_owned();
    }
    let mut slug = words.join("-");
    if slug.chars().count() > SLUG_MAX_CHARS {
        // Char-aware truncate, then trim any trailing partial-word
        // dashes so the slug never ends in a stray separator.
        let cut = slug
            .char_indices()
            .nth(SLUG_MAX_CHARS)
            .map(|(i, _)| i)
            .unwrap_or(slug.len());
        slug.truncate(cut);
        slug = slug.trim_end_matches('-').to_owned();
    }
    slug
}

/// Safety bound on the disambiguation counter (#225), mirroring capture's
/// inbox collision cap — a misbehaving store can't spin forever.
pub(in crate::vault) const SLUG_COLLISION_LIMIT: usize = 1000;

/// Return a stem unique against `taken`: `base` if it's free, else
/// `base-2`, `base-3`, … up to [`SLUG_COLLISION_LIMIT`] (#225). `None` if
/// the whole range is somehow exhausted. Pure — the vault-wide stem set is
/// gathered by the caller ([`Vault::unique_slug`]) so this stays testable
/// without a store.
pub(in crate::vault) fn disambiguate_slug(base: &str, taken: &HashSet<String>) -> Option<String> {
    if !taken.contains(base) {
        return Some(base.to_owned());
    }
    (2..SLUG_COLLISION_LIMIT)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !taken.contains(candidate))
}

impl Vault {
    /// Make `base` a globally-unique note stem: if any indexed note already
    /// uses that stem — anywhere in the vault, any note type — append
    /// `-2`, `-3`, … until free (#225). Keeps the last-segment wikilink
    /// fallback unambiguous, so a note that later relocates (an action
    /// archived to `_done/`, a project parked) keeps its `[[type/slug]]`
    /// backlinks instead of degrading to unresolved on a stem collision.
    ///
    /// Checks the index (the committed note set); a concurrent creator
    /// racing on the same stem is bounded by the write lock at commit and,
    /// worst case, healed by the next reconcile — the same tolerance the
    /// inbox per-day dedup accepts.
    pub(in crate::vault) fn unique_slug(&self, base: &str) -> Result<String, DomainError> {
        let taken: HashSet<String> = self
            .index
            .list_all_paths()?
            .iter()
            .filter_map(|p| {
                p.as_path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_owned)
            })
            .collect();
        disambiguate_slug(base, &taken)
            .ok_or_else(|| DomainError::Store(StoreError::AlreadyExists(base.to_owned())))
    }
}
