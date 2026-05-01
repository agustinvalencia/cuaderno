//! Filename-friendly slugs derived from human text. Shared between
//! `capture` and `projects` (and any future op that needs to derive
//! a stable filename from a title).

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
pub(in crate::vault) fn slugify(text: &str) -> String {
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
