//! `Vault::log_to_daily_note` and the helpers it needs.
//!
//! The daily-log write is exposed in two shapes. `log_to_daily_note`
//! is the one-shot public surface — it owns its transaction. For
//! higher-level ops that need the daily-log write to commit
//! atomically with other vault changes (e.g. `update_project_state`,
//! which must rewrite the project file *and* log the previous state
//! in the same commit), [`Vault::stage_daily_log`] adds the writes
//! to a caller-owned transaction and returns the daily-note path.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Timelike};

use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;
use cdno_core::transaction::VaultTransaction;

use crate::error::DomainError;

use super::Vault;
use super::index_entry::build_index_entry_for;

/// The heading used for the log subsection in a daily note.
const DAILY_LOGS_SECTION: &str = "Logs";

impl Vault {
    /// Append a log entry to the daily note for the given moment.
    ///
    /// Creates the note with a minimal scaffold if it doesn't exist.
    /// For existing notes, inserts the line at the end of the `## Logs`
    /// section — so later manual additions under other headings stay
    /// where the author put them.
    ///
    /// Returns the vault-relative path of the daily note touched.
    pub fn log_to_daily_note(
        &self,
        at: NaiveDateTime,
        entry: &str,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?;
        let path = self.stage_daily_log(at, entry, &mut tx)?;
        tx.commit()?;
        Ok(path)
    }

    /// Stage the writes that append `entry` to the daily-log section
    /// for `at` onto `tx`, without committing.
    ///
    /// Use this when the daily-log write must commit together with
    /// other vault changes — e.g. `update_project_state` rewrites the
    /// project file *and* logs the previous state in the same commit
    /// so a partial failure can't leave the project changed without
    /// the matching log entry (or vice versa).
    pub(in crate::vault) fn stage_daily_log(
        &self,
        at: NaiveDateTime,
        entry: &str,
        tx: &mut VaultTransaction,
    ) -> Result<VaultPath, DomainError> {
        let path = daily_note_path(at.date())?;
        let line = format_log_line(at.time(), entry);

        let new_content = if self.store.exists(&path)? {
            // File exists: parse, append into the Logs section, re-render.
            // Going through `MarkdownDocument` means a missing Logs
            // section surfaces as `ManipulationError::SectionNotFound`
            // rather than silently appending in the wrong place.
            let current = self.store.read_file(&path)?;
            let mut doc = MarkdownDocument::parse(current)?;
            doc.append_to_section(DAILY_LOGS_SECTION, &line)?;
            doc.render().to_owned()
        } else {
            // Fresh daily note: compose the scaffold with the first
            // log line already inside `## Logs`.
            scaffold_daily_note(at.date(), &line)
        };

        // Rebuild the index row from the new content so the committed
        // transaction leaves file + index in sync.
        let entry_meta = build_index_entry_for(&path, &new_content, "daily")?;

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);

        Ok(path)
    }
}

/// Vault-relative path for a daily note of the given date —
/// `journal/<year>/daily/YYYY-MM-DD.md`.
///
/// `pub(in crate::vault)` so the daily-note read/section-write module
/// (`vault/daily.rs`) resolves the same path without duplicating the
/// relpath rule.
pub(in crate::vault) fn daily_note_path(date: NaiveDate) -> Result<VaultPath, DomainError> {
    Ok(VaultPath::new(cdno_core::paths::daily_note_relpath(date))?)
}

/// Render one log line in the canonical `- **HH:MM**: text` form.
/// Trailing newline means subsequent `append_to_section` calls stack
/// cleanly without introducing blank lines between entries.
fn format_log_line(time: NaiveTime, entry: &str) -> String {
    format!("- **{:02}:{:02}**: {}\n", time.hour(), time.minute(), entry,)
}

/// Base scaffold for a brand-new daily note: valid frontmatter
/// (`type: daily`, satisfying the reconciliation contract), the date
/// heading, and an empty `## Logs` section as a stable home for the
/// first log line.
///
/// `pub(in crate::vault)` so `upsert_daily_section` can seed a fresh
/// note when a planning section is written before any log line exists.
pub(in crate::vault) fn scaffold_daily_note_base(date: NaiveDate) -> String {
    format!(
        "---\ndate: {date}\ntype: daily\n---\n\n# {heading}\n\n## {section}\n",
        date = date.format("%Y-%m-%d"),
        heading = date.format("%A, %-d %B %Y"),
        section = DAILY_LOGS_SECTION,
    )
}

/// Minimal scaffold for a brand-new daily note, pre-populated with the
/// first log line inside `## Logs`.
fn scaffold_daily_note(date: NaiveDate, first_log_line: &str) -> String {
    let mut note = scaffold_daily_note_base(date);
    note.push_str(first_log_line);
    note
}
