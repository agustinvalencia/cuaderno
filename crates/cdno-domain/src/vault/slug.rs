//! Filename-friendly slugs derived from human text. Shared between
//! `capture` and `projects` (and any future op that needs to derive
//! a stable filename from a title).

use std::collections::{HashMap, HashSet};

use cdno_core::error::StoreError;
use cdno_core::frontmatter::yaml_scalar_is_string;
use cdno_core::path::VaultPath;

use crate::error::DomainError;

use super::Vault;
use super::note_ref::candidate_slug;

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

/// Why a slug the person typed is refused (RFC 0007 §5.7). Each variant
/// names one rule, in the order [`validate_explicit_slug`] checks them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SlugRejection {
    #[error("it is empty")]
    Empty,
    #[error("it has upper-case letters; use lower case")]
    UpperCase,
    #[error("'{0}' is not allowed; use only a-z, 0-9 and '-'")]
    BadChar(char),
    #[error("it starts or ends with '-'; put letters or digits at both ends")]
    LeadingOrTrailingDash,
    #[error("it has '--' in it; join words with a single '-'")]
    DoubleDash,
    #[error("it is longer than {max} characters")]
    TooLong { max: usize },
    #[error(
        "YAML would read it as a number, a date or true/false/null rather than text; add a word to it"
    )]
    YamlTyped,
}

/// What holds a name in [`Vault::taken_names`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakenBy {
    /// A note whose file stem is the name.
    Stem,
    /// A folder note (`<name>/_index.md`), which is addressed by its folder.
    Folder,
    /// An alias a note declares.
    Alias,
}

impl TakenBy {
    /// The wire name: `stem`, `folder` or `alias`.
    pub fn as_str(self) -> &'static str {
        match self {
            TakenBy::Stem => "stem",
            TakenBy::Folder => "folder",
            TakenBy::Alias => "alias",
        }
    }
}

/// Check a slug the person typed, as opposed to one derived from a title:
/// lowercase ASCII alphanumerics joined by single `-`, no leading or
/// trailing `-`, at most [`SLUG_MAX_CHARS`], and not a value the repo's YAML
/// parser reads back as something other than a string (the slug lands in frontmatter and wikilinks).
/// Pure: whether the name is already taken is [`Vault::taken_names`]'s job.
pub fn validate_explicit_slug(slug: &str) -> Result<(), SlugRejection> {
    if slug.is_empty() {
        return Err(SlugRejection::Empty);
    }
    if slug.chars().any(|c| c.is_uppercase()) {
        return Err(SlugRejection::UpperCase);
    }
    if let Some(c) = slug
        .chars()
        .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-'))
    {
        return Err(SlugRejection::BadChar(c));
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err(SlugRejection::LeadingOrTrailingDash);
    }
    if slug.contains("--") {
        return Err(SlugRejection::DoubleDash);
    }
    if slug.chars().count() > SLUG_MAX_CHARS {
        return Err(SlugRejection::TooLong {
            max: SLUG_MAX_CHARS,
        });
    }
    if is_yaml_typed(slug) {
        return Err(SlugRejection::YamlTyped);
    }
    Ok(())
}

/// Whether a charset-valid slug would not survive as text in frontmatter:
/// the repo's YAML parser reads it as a number, bool or null (asked of the
/// parser itself, [`yaml_scalar_is_string`]), or it is one of the YAML 1.1
/// bool words or an ISO date, which that parser reads as text but other
/// tools do not. The extras are deliberate over-rejection.
fn is_yaml_typed(slug: &str) -> bool {
    if !yaml_scalar_is_string(slug) {
        return true;
    }
    if matches!(slug, "yes" | "no" | "on" | "off") {
        return true;
    }
    // `2026-11-05`: a year, a month and a day, all digits.
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let parts: Vec<&str> = slug.split('-').collect();
    parts.len() == 3 && parts[0].len() == 4 && parts.iter().all(|p| digits(p))
}

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
///
/// A text that is already a valid explicit slug
/// ([`validate_explicit_slug`]) comes back unchanged, so a slug the person
/// typed is a fixed point: it is never cut to [`SLUG_MAX_WORDS`] words.
pub fn slugify(text: &str) -> String {
    if validate_explicit_slug(text).is_ok() {
        return text.to_owned();
    }
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
    /// Every name a new note's slug must not take, with its holder and why.
    ///
    /// Built from the index through [`candidate_slug`], so a folder note
    /// (`portfolios/surrogate-model/_index.md`) holds its **folder name**
    /// as well as `_index`: a derived `surrogate-model` would otherwise
    /// shadow it in the last-segment wikilink fallback (RFC 0007 §6.2).
    pub(in crate::vault) fn taken_names(
        &self,
    ) -> Result<HashMap<String, (VaultPath, TakenBy)>, DomainError> {
        let mut taken: HashMap<String, (VaultPath, TakenBy)> = HashMap::new();
        for path in self.index.list_all_paths()? {
            if let Some(stem) = path.as_path().file_stem().and_then(|s| s.to_str()) {
                taken
                    .entry(stem.to_owned())
                    .or_insert_with(|| (path.clone(), TakenBy::Stem));
            }
            if let Some(name) = candidate_slug(&path) {
                taken
                    .entry(name)
                    .or_insert_with(|| (path.clone(), TakenBy::Folder));
            }
        }
        Ok(taken)
    }

    /// Make `base` a globally-unique note stem: if any indexed note already
    /// uses that stem or folder name — anywhere in the vault, any note
    /// type — append `-2`, `-3`, … until free (#225). Keeps the
    /// last-segment wikilink fallback unambiguous, so a note that later
    /// relocates (an action archived to `_done/`, a project parked) keeps
    /// its `[[type/slug]]` backlinks instead of degrading to unresolved on
    /// a stem collision.
    ///
    /// Checks the index (the committed note set); a concurrent creator
    /// racing on the same stem is bounded by the write lock at commit and,
    /// worst case, healed by the next reconcile — the same tolerance the
    /// inbox per-day dedup accepts.
    pub(in crate::vault) fn unique_slug(&self, base: &str) -> Result<String, DomainError> {
        let taken: HashSet<String> = self.taken_names()?.into_keys().collect();
        disambiguate_slug(base, &taken)
            .ok_or_else(|| DomainError::Store(StoreError::AlreadyExists(base.to_owned())))
    }
}
