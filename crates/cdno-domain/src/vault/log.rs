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
        let new_content = self.fold_daily_log_line(at.time(), base, entry)?;

        // Rebuild the index row from the new content so the committed
        // transaction leaves file + index in sync.
        let entry_meta = build_index_entry_for(&path, &new_content, "daily")?;

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);

        Ok(path)
    }

    /// Fold one log line for `time` into `base` — an already-materialised
    /// daily-note document — returning the rendered content. The line is
    /// inserted into the `## Logs` section and the trailing anchor section
    /// re-pinned to the bottom (#232/#212), matching `stage_daily_log`.
    ///
    /// Split out so a caller that has *already staged a write to today's
    /// daily note* (e.g. `set_frontmatter` toggling a `log_on_change`
    /// field on the daily note itself) can fold the log line into that
    /// same in-flight content and write the file once. Staging a separate
    /// daily-log write would read the pre-change content back from the
    /// store and clobber the frontmatter edit — this seam keeps the file
    /// and its index row consistent.
    pub(in crate::vault) fn fold_daily_log_line(
        &self,
        time: NaiveTime,
        base: String,
        entry: &str,
    ) -> Result<String, DomainError> {
        let line = format_log_line(time, entry);
        let mut doc = MarkdownDocument::parse(base)?;
        doc.append_to_section(DAILY_LOGS_SECTION, &line)?;
        doc.move_section_to_end(&self.daily_anchor_section()?)?;
        Ok(doc.render().to_owned())
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
    /// Variables supplied to the daily template:
    /// - `{{date}}` — ISO date, e.g. `2026-06-28`
    /// - `{{heading}}` — long form, e.g. `Sunday, 28 June 2026`
    /// - `{{weekday}}` — weekday name, e.g. `Sunday`
    /// - `{{day_name}}` — alias of `weekday`, the same value (people reach
    ///   for either name); both resolve to `%A`
    /// - `{{week}}` — ISO week label `YYYY-Www`, e.g. `2026-W27`, so a
    ///   custom daily template can render its own `week:` frontmatter that
    ///   matches the weekly note it points at (#300)
    ///
    /// Unsupplied placeholders are left verbatim by the engine, so a
    /// custom template referencing a variable not in this set renders it
    /// literally (e.g. `# {{weekday}}` stayed unrendered before `weekday`
    /// was provided here).
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
        // `weekday` and `day_name` are deliberate aliases for the same `%A`
        // value: `weekday` is the original name, `day_name` is what people
        // naturally reach for when customising the template (#300).
        let weekday = date.format("%A").to_string();
        ctx.set_contextual("weekday", weekday.clone());
        ctx.set_contextual("day_name", weekday);
        // ISO-week label reused from the weekly scaffold via the shared
        // helper, so a daily note's `week:` frontmatter matches the weekly
        // note for the same date.
        ctx.set_contextual("week", super::iso_week_label(date));
        self.scaffold("daily", None, &mut ctx)
    }

    /// The section kept pinned at the bottom of a daily note: the last
    /// level-2 heading of the effective daily template. For the built-in
    /// template that's `## Logs`; a custom `.cuaderno/templates/daily.md`
    /// can designate a different trailing section (#212). Falls back to
    /// `Logs` if the template has no `##` heading.
    ///
    /// Uses a simple line scan, so a `## ` inside a fenced code block in
    /// a custom template would be mistaken for a heading — fine for the
    /// plain section-list templates this is meant for.
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
