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
use cdno_core::template::VariableContext;
use cdno_core::transaction::VaultTransaction;

use crate::error::DomainError;

use super::Vault;
use super::index_entry::build_index_entry_for;

use super::DAILY_LOGS_SECTION;

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

        // One path for both fresh and existing notes: get the base
        // (scaffold a new one, or read the existing file), then insert
        // the line *into* the `## Logs` section rather than at the end of
        // the text. This keeps the line in Logs even when a custom daily
        // template has content after it, and surfaces a Logs-less
        // template as `SectionNotFound` rather than misplacing the entry.
        let base = if self.store.exists(&path)? {
            self.store.read_file(&path)?
        } else {
            self.scaffold_daily_base(at.date())?
        };
        let mut doc = MarkdownDocument::parse(base)?;
        doc.append_to_section(DAILY_LOGS_SECTION, &line)?;
        // Keep the trailing section pinned to the bottom — and self-heal a
        // note where it had drifted up (#232). The anchor is the daily
        // template's last section, so a custom template can pin something
        // other than `## Logs` last (#212).
        doc.move_section_to_end(&self.daily_anchor_section()?)?;
        let new_content = doc.render().to_owned();

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

impl Vault {
    /// Base scaffold for a brand-new daily note: valid frontmatter
    /// (`type: daily`), the date heading, and an empty `## Logs` section
    /// as a stable home for the first log line. Rendered through the
    /// template engine (#212), so a custom `.cuaderno/templates/daily.md`
    /// takes effect.
    ///
    /// `pub(in crate::vault)` so `upsert_daily_section` can seed a fresh
    /// note when a planning section is written before any log line exists.
    pub(in crate::vault) fn scaffold_daily_base(
        &self,
        date: NaiveDate,
    ) -> Result<String, DomainError> {
        let mut ctx = VariableContext::new();
        ctx.set_contextual("date", date.format("%Y-%m-%d").to_string());
        ctx.set_contextual("heading", date.format("%A, %-d %B %Y").to_string());
        self.scaffold("daily", None, &ctx)
    }

    /// The section kept pinned at the bottom of a daily note: the last
    /// level-2 heading of the effective daily template. For the built-in
    /// template that's `## Logs`; a custom `.cuaderno/templates/daily.md`
    /// can designate a different trailing section (#212). Falls back to
    /// `Logs` if the template has no `##` heading.
    pub(in crate::vault) fn daily_anchor_section(&self) -> Result<String, DomainError> {
        let template = self.resolve_template_content("daily", None)?;
        Ok(template
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("## "))
            .map(|s| s.trim().to_owned())
            .unwrap_or_else(|| DAILY_LOGS_SECTION.to_owned()))
    }
}
