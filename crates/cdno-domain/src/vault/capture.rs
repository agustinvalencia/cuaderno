//! `Vault::capture_to_inbox` and the slug logic that produces inbox
//! filenames.

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::path::VaultPath;

use crate::error::DomainError;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;

/// Safety bound on the collision-counter loop. 100 same-day captures
/// with the same first six words is already a misuse — the user is
/// better off seeing an explicit error than waiting on an unbounded
/// retry loop.
const COLLISION_LIMIT: u32 = 100;

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
        let mut tx = self.transaction()?;
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
