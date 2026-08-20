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

/// Floor on what backing off to a word boundary may leave (#524).
///
/// The retreat exists to avoid a mangled fragment, but a long word
/// starting early puts the preceding boundary near the front of the
/// slug: `ab cd ef <45-char word>` would retreat from 50 chars to 8,
/// discarding most of what makes the filename recognisable — and slugs
/// that collapse to a shared prefix collide, which
/// [`Vault::create_portfolio`](crate::Vault::create_portfolio) reports
/// as `AlreadyExists` rather than disambiguating away. Below this floor
/// the word being cut is long enough to be the pathological case the
/// char cap exists for, so it is cut where the cap falls — exactly as a
/// *first* word of the same length already is.
pub(in crate::vault) const SLUG_MIN_AFTER_RETREAT: usize = SLUG_MAX_CHARS / 2;

/// Build a slug from the first words of `text`: lowercase
/// alphanumerics joined by `-`, capped to [`SLUG_MAX_WORDS`] /
/// [`SLUG_MAX_CHARS`] so the filename stays manageable. Returns
/// `"untitled"` if the text contains no alphanumerics.
///
/// A char cap landing inside a word backs off to the preceding word
/// boundary, so the slug ends on a whole word (#524). Two cases are
/// exempt, and both are a word too long to keep whole: the *first* word
/// overrunning the cap, which leaves no boundary to retreat to however
/// many words follow it, and a retreat that would leave less than
/// [`SLUG_MIN_AFTER_RETREAT`] chars. Both are cut where the cap falls.
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
        // Char-aware truncate, then back off to a word boundary so the
        // slug ends on a whole word rather than a fragment (#524).
        let cut = slug
            .char_indices()
            .nth(SLUG_MAX_CHARS)
            .map(|(i, _)| i)
            .unwrap_or(slug.len());
        // Whether the char being dropped is the separator itself. If it
        // is, the cut already fell between two words and the last kept
        // word is whole — backing off further would discard a word the
        // cap left room for.
        let cut_on_separator = slug.as_bytes().get(cut) == Some(&b'-');
        slug.truncate(cut);
        if !cut_on_separator {
            // The cut landed inside a word, leaving a fragment. Drop back
            // to the last separator: the slug is the filename and the
            // wikilink target, so a mangled word is permanent and visible
            // everywhere the note is referenced.
            //
            // Two ways the retreat is declined, both meaning the word
            // being cut is too long to keep whole. `rfind` finds no
            // separator when the first word alone overruns the cap, and
            // `SLUG_MIN_AFTER_RETREAT` refuses a retreat that would give
            // up most of the slug to save one fragment. Either way the
            // hard truncation stands, which is what the cap is for.
            if let Some(sep) = slug.rfind('-')
                && slug[..sep].chars().count() >= SLUG_MIN_AFTER_RETREAT
            {
                slug.truncate(sep);
            }
        }
        // No trailing-dash trim: every path above now ends on a word
        // char. A cut on the separator keeps the text before it, the
        // retreat truncates *at* a separator, and a declined retreat can
        // only happen when the last separator sits below the floor —
        // which a slug ending in one never does.
    }
    slug
}

/// Safety bound on the disambiguation counter (#225), mirroring capture's
/// inbox collision cap — a misbehaving store can't spin forever.
pub(in crate::vault) const SLUG_COLLISION_LIMIT: usize = 1000;

/// Return a stem unique against `taken`: `base` if it's free, else the first
/// free of `base-2`, `base-3`, … (bounded by [`SLUG_COLLISION_LIMIT`] so a
/// misbehaving store can't spin forever, #225). `None` only if that whole
/// range is somehow exhausted. Pure — the vault-wide stem set is gathered by
/// the caller ([`Vault::unique_slug`]) so this stays testable without a store.
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
