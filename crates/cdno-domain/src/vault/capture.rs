//! `Vault::capture_to_inbox` and the slug logic that produces inbox
//! filenames.

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;
use cdno_core::template::VariableContext;

use crate::error::DomainError;
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;

/// One uncategorised capture under `inbox/` awaiting triage.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InboxItem {
    /// Filename stem (`<YYYY-MM-DD>-<slug>`) — the handle passed to
    /// [`Vault::discard_inbox_item`].
    pub slug: String,
    /// The captured text (the note body, trimmed).
    pub text: String,
}

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
        let content = self.scaffold_inbox(at, text)?;

        let entry_meta = build_index_entry_for(&path, &content, "inbox")?;
        let mut tx = self.transaction()?;
        tx.write_file(path.clone(), content);
        tx.upsert_note(entry_meta);
        tx.commit()?;
        Ok(path)
    }

    /// Every uncategorised capture under `inbox/`, oldest first (the
    /// filename's `YYYY-MM-DD` prefix sorts chronologically). Feeds
    /// `cdno triage` and the `triage_inbox` MCP tool — the read half of
    /// draining the inbox.
    pub fn list_inbox(&self) -> Result<Vec<InboxItem>, DomainError> {
        let mut entries = self.index.list_by_type(NoteType::Inbox.as_str())?;
        entries.sort_by(|a, b| a.path.as_path().cmp(b.path.as_path()));

        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            let raw = self.store.read_file(&entry.path)?;
            let (_fm, body) = Frontmatter::parse(&raw)?;
            out.push(InboxItem {
                slug: inbox_slug_from_path(&entry.path),
                text: body.trim().to_owned(),
            });
        }
        Ok(out)
    }

    /// Discard a triaged inbox capture: delete the note (file + index
    /// row) in one commit and log the discard to today's daily. `slug`
    /// is the filename stem from [`list_inbox`](Self::list_inbox).
    ///
    /// "Route to a task/note" needs no special method — create the
    /// destination with the usual verb (`add_action`, …) and then
    /// discard the capture.
    ///
    /// This is a deliberate **hard delete** (no `inbox/_done/` archive):
    /// inbox items are fleeting captures, not completed work like
    /// actions or commitments that earn a `_done` record. The captured
    /// text is still preserved in the daily-log line, so a discard is
    /// recoverable from the append-only daily even though the note
    /// itself is gone — no unrecoverable data loss.
    ///
    /// Errors with [`StoreError::NotFound`] when no inbox note matches.
    pub fn discard_inbox_item(
        &self,
        at: NaiveDateTime,
        slug: &str,
    ) -> Result<VaultPath, DomainError> {
        let path = VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::INBOX))?;
        if !self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::NotFound(path.to_string())));
        }

        // Read the captured text first so the discard is recoverable
        // from the daily log after the note is deleted. Collapse
        // whitespace so a multi-line capture stays a single log line.
        let raw = self.store.read_file(&path)?;
        let (_fm, body) = Frontmatter::parse(&raw)?;
        let text = body.split_whitespace().collect::<Vec<_>>().join(" ");

        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        tx.delete_file(path.clone());
        tx.remove_note(path.clone());
        // Plain text, not a wikilink: the note is being deleted, so a
        // `[[inbox/<slug>]]` would immediately dangle.
        let log_entry = if text.is_empty() {
            format!("triaged inbox item `{slug}` -- discarded")
        } else {
            format!("triaged inbox item `{slug}` -- discarded: {text}")
        };
        self.stage_daily_log(at, &log_entry, &mut tx)?;
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

/// The filename stem of an `inbox/<stem>.md` capture. Empty string for
/// a malformed path; callers have already filtered to the inbox type.
fn inbox_slug_from_path(path: &VaultPath) -> String {
    path.as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_owned()
}

impl Vault {
    /// Render the canonical inbox note for `at` carrying `text`, through
    /// the template engine (#212).
    fn scaffold_inbox(&self, at: NaiveDateTime, text: &str) -> Result<String, DomainError> {
        let mut ctx = VariableContext::new();
        ctx.set_contextual("created", at.format("%Y-%m-%dT%H:%M:%S").to_string());
        ctx.set_contextual("body", text.trim());
        self.scaffold("inbox", None, &mut ctx)
    }
}
