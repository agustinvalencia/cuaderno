//! `Vault::capture_to_inbox` and the slug logic that produces inbox
//! filenames.

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::path::VaultPath;

use crate::error::DomainError;

use super::Vault;
use super::index_entry::build_index_entry_for;

/// Maximum number of words kept from the captured text when building
/// the slug. Six is enough to be recognisable without producing
/// absurdly long filenames.
const SLUG_MAX_WORDS: usize = 6;

/// Hard char cap on the slug. A single very long word still gets
/// truncated so a pathological input can't blow filesystem name
/// limits.
const SLUG_MAX_CHARS: usize = 50;

/// Safety bound on the collision-counter loop, so a misbehaving store
/// can't hang `capture_to_inbox` forever.
const COLLISION_LIMIT: u32 = 1000;

impl Vault {
    /// Capture a quick note into `inbox/`. Returns the vault-relative
    /// path of the new file.
    ///
    /// Filename layout: `inbox/<YYYY-MM-DD>-<slug>.md`, where the slug
    /// is derived from the first ~6 words of the text. If the slug
    /// would be empty (the text is whitespace or punctuation only), it
    /// falls back to `untitled`. Filename collisions on the same day
    /// — same date plus same leading words — get a `-N` counter
    /// suffix, so `2026-04-26-buy-groceries.md`,
    /// `2026-04-26-buy-groceries-2.md`, and so on.
    ///
    /// The body of the file is the captured text trimmed of leading
    /// and trailing whitespace, with minimal frontmatter
    /// (`type: inbox`, `created: <ISO>`).
    pub fn capture_to_inbox(
        &self,
        at: NaiveDateTime,
        text: &str,
    ) -> Result<VaultPath, DomainError> {
        let path = self.next_inbox_path(at.date(), text)?;
        let content = scaffold_inbox_note(at, text);

        let entry_meta = build_index_entry_for(&path, &content, "inbox")?;
        let mut tx = self.transaction();
        tx.write_file(path.clone(), content);
        tx.upsert_note(entry_meta);
        tx.commit()?;
        Ok(path)
    }

    /// Resolve an unused inbox filename for `(date, text)`. Walks
    /// `-2`, `-3`, ... suffixes if needed, capped at a safety limit
    /// to avoid an infinite loop on a misbehaving store.
    fn next_inbox_path(&self, date: NaiveDate, text: &str) -> Result<VaultPath, DomainError> {
        let slug = slugify(text);
        let base = format!(
            "{}/{}-{}",
            cdno_core::paths::INBOX,
            date.format("%Y-%m-%d"),
            slug
        );
        let first = VaultPath::new(format!("{base}.md"))?;
        if !self.store.exists(&first)? {
            return Ok(first);
        }
        for n in 2..COLLISION_LIMIT {
            let candidate = VaultPath::new(format!("{base}-{n}.md"))?;
            if !self.store.exists(&candidate)? {
                return Ok(candidate);
            }
        }
        Err(DomainError::Store(
            cdno_core::error::StoreError::AlreadyExists(base),
        ))
    }
}

/// Render the canonical inbox note for `at` carrying `text`.
fn scaffold_inbox_note(at: NaiveDateTime, text: &str) -> String {
    format!(
        "---\ntype: inbox\ncreated: {created}\n---\n\n{body}\n",
        created = at.format("%Y-%m-%dT%H:%M:%S"),
        body = text.trim(),
    )
}

/// Build a slug from the first words of `text`: lowercase,
/// alphanumerics joined by `-`, capped to a sensible length so the
/// filename stays manageable. Returns `"untitled"` if the text
/// contains no alphanumerics.
fn slugify(text: &str) -> String {
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
