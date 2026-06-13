//! Weekly-review note reads and section writes.
//!
//! The weekly note (design §5.2) is one artefact per ISO week at
//! `journal/<iso-year>/weekly/<YYYY>-W<ww>.md`, composed during the
//! weekly-review ritual. It carries four sections — Wins, Challenges,
//! One Improvement, and Next Week's Focus — the last of which is where
//! the forward *plan* lives. Two operations sit here, mirroring the
//! daily-note seam (`daily.rs`):
//!
//! - [`Vault::read_weekly_note`] — the read side, so a skill can check
//!   whether the week already has a note (and what's in it) before
//!   composing.
//! - [`Vault::upsert_weekly_section`] — create-or-write one section of
//!   the week's note, replacing it (compose the review) or appending to
//!   it (accrue across a session).
//!
//! Unlike the daily note there is no append-only history section to
//! protect: every weekly section is review content, so [`WeeklySection`]
//! is the full set and both the replace and append paths may target any
//! of them. The note is keyed by ISO week, so any day in the week
//! resolves to the same note.

use std::str::FromStr;

use chrono::{Datelike, Duration, NaiveDate};

use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;

use crate::error::DomainError;

use super::Vault;
use super::daily::format_section_body;
use super::index_entry::build_index_entry_for;

/// A weekly note's content, returned by [`Vault::read_weekly_note`]. A
/// week with no note yet returns `exists: false` and empty `markdown`
/// rather than an error — absence is a normal answer the caller branches
/// on, not a failure. Mirrors [`super::DailyNoteView`].
#[derive(Debug, Clone)]
pub struct WeeklyNoteView {
    pub path: VaultPath,
    pub exists: bool,
    pub markdown: String,
}

/// The writable sections of a weekly-review note (design §5.2). All four
/// are review content composed during the ritual, so — unlike the daily
/// note — there is no history section held back from the writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeeklySection {
    Wins,
    Challenges,
    OneImprovement,
    NextWeeksFocus,
}

impl WeeklySection {
    /// The level-2 heading text this section maps to.
    pub fn heading(self) -> &'static str {
        match self {
            WeeklySection::Wins => "Wins",
            WeeklySection::Challenges => "Challenges",
            WeeklySection::OneImprovement => "One Improvement",
            WeeklySection::NextWeeksFocus => "Next Week's Focus",
        }
    }
}

impl FromStr for WeeklySection {
    type Err = String;

    /// Case-insensitive parse, tolerant of hyphens/underscores and the
    /// apostrophe in "Next Week's Focus". The error string names the
    /// allowlist so the MCP layer can surface it verbatim.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalised = s
            .trim()
            .to_ascii_lowercase()
            .replace(['-', '_'], " ")
            .replace('\'', "");
        match normalised.as_str() {
            "wins" => Ok(WeeklySection::Wins),
            "challenges" => Ok(WeeklySection::Challenges),
            "one improvement" => Ok(WeeklySection::OneImprovement),
            "next weeks focus" => Ok(WeeklySection::NextWeeksFocus),
            other => Err(format!(
                "unknown weekly section '{other}' (expected one of: \
                 wins, challenges, one improvement, next week's focus)"
            )),
        }
    }
}

impl Vault {
    /// Read the weekly note for the ISO week containing `week_of`.
    ///
    /// Returns `exists: false` with empty markdown when the week has no
    /// note yet, so callers can test for an existing review without
    /// catching an error.
    pub fn read_weekly_note(&self, week_of: NaiveDate) -> Result<WeeklyNoteView, DomainError> {
        let path = weekly_note_path(week_of)?;
        if self.store.exists(&path)? {
            let markdown = self.store.read_file(&path)?;
            Ok(WeeklyNoteView {
                path,
                exists: true,
                markdown,
            })
        } else {
            Ok(WeeklyNoteView {
                path,
                exists: false,
                markdown: String::new(),
            })
        }
    }

    /// Write one section of the weekly note for the ISO week containing
    /// `week_of`, returning the note's path.
    ///
    /// Creates the note (frontmatter + the four-section scaffold) if it
    /// doesn't exist, then `ensure_section` followed by either
    /// `replace_section` (`append: false` — compose/overwrite the
    /// section, the default for a review pass) or `append_to_section`
    /// (`append: true` — accrue within a section across a session).
    pub fn upsert_weekly_section(
        &self,
        week_of: NaiveDate,
        section: WeeklySection,
        content: &str,
        append: bool,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let path = weekly_note_path(week_of)?;
        let heading = section.heading();
        let body = format_section_body(content);

        let base = if self.store.exists(&path)? {
            self.store.read_file(&path)?
        } else {
            scaffold_weekly_note_base(week_of)
        };

        let mut doc = MarkdownDocument::parse(base)?;
        doc.ensure_section(heading)?;
        if append {
            doc.append_to_section(heading, &body)?;
        } else {
            doc.replace_section(heading, &body)?;
        }
        let new_content = doc.render().to_owned();

        let entry_meta = build_index_entry_for(&path, &new_content, "weekly")?;

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        tx.commit()?;

        Ok(path)
    }
}

/// The vault path of the weekly note for the ISO week containing `date`.
pub(in crate::vault) fn weekly_note_path(date: NaiveDate) -> Result<VaultPath, DomainError> {
    Ok(VaultPath::new(cdno_core::paths::weekly_note_relpath(date))?)
}

/// Scaffold a fresh weekly note per design §5.2: ISO-week frontmatter
/// (`week`, `date_start` = Monday, `date_end` = Sunday), a `# Week N,
/// YYYY` heading, and the four empty review sections. Keyed off the ISO
/// week, so any `date` within the week produces the same scaffold.
pub(in crate::vault) fn scaffold_weekly_note_base(date: NaiveDate) -> String {
    let iso = date.iso_week();
    let monday = date - Duration::days(i64::from(date.weekday().num_days_from_monday()));
    let sunday = monday + Duration::days(6);
    format!(
        "---\n\
         type: weekly\n\
         week: {year}-W{week:02}\n\
         date_start: {start}\n\
         date_end: {end}\n\
         ---\n\
         \n\
         # Week {week}, {year}\n\
         \n\
         ## Wins\n\
         \n\
         ## Challenges\n\
         \n\
         ## One Improvement\n\
         \n\
         ## Next Week's Focus\n",
        year = iso.year(),
        week = iso.week(),
        start = monday.format("%Y-%m-%d"),
        end = sunday.format("%Y-%m-%d"),
    )
}
