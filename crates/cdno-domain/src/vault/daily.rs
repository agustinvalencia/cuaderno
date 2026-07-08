//! Daily-note reads and planning-section writes.
//!
//! Two operations sit here, both serving the skill layer (GH #158):
//!
//! - [`Vault::read_daily_note`] — the read side, so a skill can check
//!   for pre-planned content (a written intention, a pre-filled
//!   agenda) before deciding what to write.
//! - [`Vault::upsert_daily_section`] — create-or-replace a *planning*
//!   section of the daily note.
//!
//! # Which sections are writable
//!
//! The daily note is append-only for its **history**: `## Logs` (and
//! any `## Notes` captures) only ever grow, via
//! [`Vault::log_to_daily_note`]. The other sections — Standup,
//! Intention, Agenda (mutable planning scratch, typically replaced) and
//! Meeting (live notes, typically appended) — are writable via
//! `upsert_daily_section`. [`DailySection`] is the type-level allowlist
//! that keeps it away from the history sections, so neither the
//! overwrite nor the append path can ever clobber the log.

use std::str::FromStr;

use chrono::{Datelike, NaiveDate};

use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;

use crate::error::DomainError;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::log::daily_note_path;

/// A daily note's content, returned by [`Vault::read_daily_note`].
///
/// A day with no note yet returns `exists: false` and an empty
/// `markdown` rather than an error — absence is a normal answer the
/// caller branches on, not a failure.
#[derive(Debug, Clone)]
pub struct DailyNoteView {
    pub path: VaultPath,
    pub exists: bool,
    pub markdown: String,
}

/// The non-history sections of a daily note that
/// [`Vault::upsert_daily_section`] may write. `Standup`/`Intention`/
/// `Agenda` are mutable planning scratch (typically replaced); `Meeting`
/// accrues live meeting notes (typically appended). The append-only
/// history sections (`## Logs`, `## Notes`) are deliberately absent —
/// they grow only via [`Vault::log_to_daily_note`] and cannot be reached
/// here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DailySection {
    Standup,
    Intention,
    Agenda,
    Meeting,
}

impl DailySection {
    /// The level-2 heading text this section maps to.
    pub fn heading(self) -> &'static str {
        match self {
            DailySection::Standup => "Standup",
            DailySection::Intention => "Intention",
            DailySection::Agenda => "Agenda",
            DailySection::Meeting => "Meeting",
        }
    }
}

impl FromStr for DailySection {
    type Err = String;

    /// Case-insensitive parse. The error string names the allowlist so
    /// the MCP layer can surface it verbatim as an invalid-argument
    /// reason.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "standup" => Ok(DailySection::Standup),
            "intention" => Ok(DailySection::Intention),
            "agenda" => Ok(DailySection::Agenda),
            "meeting" => Ok(DailySection::Meeting),
            other => Err(format!(
                "unknown daily section '{other}' (expected one of: standup, intention, agenda, meeting)"
            )),
        }
    }
}

impl Vault {
    /// Read the daily note for `date`.
    ///
    /// Returns `exists: false` with empty markdown when no note has
    /// been created for that day yet, so callers can test for
    /// pre-planned content without catching an error.
    pub fn read_daily_note(&self, date: NaiveDate) -> Result<DailyNoteView, DomainError> {
        let path = daily_note_path(date)?;
        if self.store.exists(&path)? {
            let markdown = self.store.read_file(&path)?;
            Ok(DailyNoteView {
                path,
                exists: true,
                markdown,
            })
        } else {
            Ok(DailyNoteView {
                path,
                exists: false,
                markdown: String::new(),
            })
        }
    }

    /// The dates in `year`/`month` that already have a daily note, sorted
    /// chronologically.
    ///
    /// Scans only that year's daily directory (`journal/<year>/daily/`) —
    /// every daily note for the month lives there, since the note is filed
    /// under its calendar year — rather than walking the whole vault, then
    /// keeps only the filenames that parse as a `YYYY-MM-DD` date landing
    /// in the requested calendar month. Backs the desktop calendar grid's
    /// note-bearing-day marks.
    ///
    /// A year with no daily directory yet reads back as an empty listing
    /// from both stores, so a fresh (or note-less) month yields an empty
    /// vec rather than an error. The caller is responsible for passing a
    /// valid `month` (1..=12); an out-of-range value simply matches
    /// nothing, since no real date can fall in it.
    pub fn daily_dates_in_month(
        &self,
        year: i32,
        month: u32,
    ) -> Result<Vec<NaiveDate>, DomainError> {
        let dir = VaultPath::new(cdno_core::paths::journal_daily_dir(year))?;
        let mut dates: Vec<NaiveDate> = self
            .store
            .list_dir(&dir)?
            .into_iter()
            .filter_map(|p| {
                // Keep only children whose file stem parses as a real date
                // in the requested month; ignore any other file (or a
                // subdirectory) that happens to live alongside the notes.
                let stem = p.as_path().file_stem()?.to_str()?;
                let date = NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok()?;
                (date.year() == year && date.month() == month).then_some(date)
            })
            .collect();
        dates.sort_unstable();
        Ok(dates)
    }

    /// Write a non-history section of the daily note for `date`,
    /// returning the note's path.
    ///
    /// Creates the daily note (with an empty `## Logs`) if it doesn't
    /// exist, then `ensure_section` followed by either `replace_section`
    /// (`append: false` — the planning sections, idempotent overwrite)
    /// or `append_to_section` (`append: true` — live meeting notes that
    /// accrue). The `## Logs` history content is never clobbered — the
    /// write targets `section`'s heading alone — and `move_section_to_end`
    /// then pins `## Logs` back to the bottom so a planning section
    /// created mid-day can't strand the history above it (#232).
    pub fn upsert_daily_section(
        &self,
        date: NaiveDate,
        section: DailySection,
        content: &str,
        append: bool,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let path = daily_note_path(date)?;
        let heading = section.heading();
        let body = format_section_body(content);

        let base = if self.store.exists(&path)? {
            self.store.read_file(&path)?
        } else {
            self.scaffold_daily_base(date)?
        };

        let mut doc = MarkdownDocument::parse(base)?;
        doc.ensure_section(heading)?;
        if append {
            doc.append_to_section(heading, &body)?;
        } else {
            doc.replace_section(heading, &body)?;
        }
        // A newly created planning section is appended at the end of the
        // note, which would push the trailing section below it; pin that
        // section (the daily template's last one — `## Logs` by default)
        // back to the bottom (#232, #212).
        doc.move_section_to_end(&self.daily_anchor_section()?)?;
        let new_content = doc.render().to_owned();

        let entry_meta = build_index_entry_for(&path, &new_content, "daily")?;

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        tx.commit()?;

        Ok(path)
    }
}

/// Render a section body so it sits cleanly under its heading: the
/// content trimmed, on its own line, with a single trailing newline.
/// Empty content yields an empty section (just the heading), which is
/// how an intention is "cleared" by writing an empty string. Shared with
/// the weekly-note writer, which formats its sections the same way.
pub(in crate::vault) fn format_section_body(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}
