//! Weekly-review note reads and section writes.
//!
//! The weekly note (design §5.2) is one artefact per ISO week at
//! `journal/<iso-year>/weekly/<YYYY>-W<ww>.md`, composed during the
//! weekly-review ritual. It carries four sections — Wins, Challenges,
//! One Improvement, and This Week's Goal — the last of which is the
//! week's anchoring goal (set by planning at the top of the week, or
//! carried into the next week's note by the review). Two operations sit
//! here, mirroring the
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
use cdno_core::template::VariableContext;

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

/// The writable sections of a weekly note (design §5.2). Three are
/// review content composed during the ritual (Wins, Challenges, One
/// Improvement); `ThisWeeksGoal` is the week's anchor, set ahead of the
/// week by planning. None is an append-only history section, so — unlike
/// the daily note — there is nothing held back from the writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeeklySection {
    Wins,
    Challenges,
    OneImprovement,
    ThisWeeksGoal,
}

impl WeeklySection {
    /// The level-2 heading text this section maps to.
    pub fn heading(self) -> &'static str {
        match self {
            WeeklySection::Wins => "Wins",
            WeeklySection::Challenges => "Challenges",
            WeeklySection::OneImprovement => "One Improvement",
            WeeklySection::ThisWeeksGoal => "This Week's Goal",
        }
    }
}

impl FromStr for WeeklySection {
    type Err = String;

    /// Case-insensitive parse, tolerant of hyphens/underscores and the
    /// apostrophe in "This Week's Goal". The error string names the
    /// allowlist so the MCP layer can surface it verbatim. The former
    /// "Next Week's Focus" name is still accepted as a deprecated alias
    /// so pre-rename callers don't hard-fail during the transition.
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
            "this weeks goal" => Ok(WeeklySection::ThisWeeksGoal),
            // Deprecated alias from before the rename; maps to the same
            // section so existing automation keeps working.
            "next weeks focus" => Ok(WeeklySection::ThisWeeksGoal),
            other => Err(format!(
                "unknown weekly section '{other}' (expected one of: \
                 wins, challenges, one improvement, this week's goal)"
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
            self.scaffold_weekly_base(week_of)?
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

impl Vault {
    /// Scaffold a fresh weekly note per design §5.2: ISO-week frontmatter
    /// (`week`, `date_start` = Monday, `date_end` = Sunday), a `# Week N,
    /// YYYY` heading, and the four empty review sections. Keyed off the
    /// ISO week, so any `date` within the week produces the same note.
    /// Rendered through the template engine (#212).
    pub(in crate::vault) fn scaffold_weekly_base(
        &self,
        date: NaiveDate,
    ) -> Result<String, DomainError> {
        let iso = date.iso_week();
        let monday = date - Duration::days(i64::from(date.weekday().num_days_from_monday()));
        let sunday = monday + Duration::days(6);
        let mut ctx = VariableContext::new();
        // Two vars for the same number, intentionally: the frontmatter
        // `week:` wants the padded ISO form `YYYY-Www`, the `# Week N`
        // heading wants the bare number. Templates are pure substitution
        // (no logic), so the padding can't be expressed in-template.
        ctx.set_contextual("week", format!("{}-W{:02}", iso.year(), iso.week()));
        ctx.set_contextual("week_num", iso.week().to_string());
        ctx.set_contextual("year", iso.year().to_string());
        ctx.set_contextual("date_start", monday.format("%Y-%m-%d").to_string());
        ctx.set_contextual("date_end", sunday.format("%Y-%m-%d").to_string());
        self.scaffold("weekly", None, &ctx)
    }
}
