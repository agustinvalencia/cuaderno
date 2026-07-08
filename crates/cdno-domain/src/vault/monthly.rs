//! Monthly-review note reads and section writes.
//!
//! The monthly note (design §5.2.1) is one artefact per calendar month at
//! `journal/<year>/monthly/<YYYY-MM>.md`, composed during the
//! monthly-review ritual. It carries three review sections — Wins,
//! Themes, and Next Month's Focus — plus a scaffolded `## Weeks` block
//! that links (never copies) the month's weekly notes so the weeks stay
//! the source of truth and the month points at them. Two operations sit
//! here, mirroring the weekly-note seam (`weekly.rs`):
//!
//! - [`Vault::read_monthly_note`] — the read side, so a skill can check
//!   whether the month already has a note (and what's in it) before
//!   composing.
//! - [`Vault::upsert_monthly_section`] — create-or-write one section of
//!   the month's note, replacing it (compose the review) or appending to
//!   it (accrue across a session).
//!
//! Like the weekly note there is no append-only history section to
//! protect: every monthly section is review content, so [`MonthlySection`]
//! is the full set and both the replace and append paths may target any
//! of them. The note is keyed by calendar month, so any day in the month
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

/// A monthly note's content, returned by [`Vault::read_monthly_note`]. A
/// month with no note yet returns `exists: false` and empty `markdown`
/// rather than an error — absence is a normal answer the caller branches
/// on, not a failure. Mirrors [`super::DailyNoteView`] and
/// [`super::WeeklyNoteView`].
#[derive(Debug, Clone)]
pub struct MonthlyNoteView {
    pub path: VaultPath,
    pub exists: bool,
    pub markdown: String,
}

/// The writable sections of a monthly note (design §5.2.1). All three are
/// review content composed during the ritual. There is no append-only
/// history section, so — like the weekly note — nothing is held back
/// from the writer. Deliberately celebration-first and lean: no Metrics
/// section, since quantitative metrics live behind the desktop
/// "show metrics" toggle, not in a note section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonthlySection {
    Wins,
    Themes,
    NextMonthsFocus,
}

impl MonthlySection {
    /// The level-2 heading text this section maps to.
    pub fn heading(self) -> &'static str {
        match self {
            MonthlySection::Wins => "Wins",
            MonthlySection::Themes => "Themes",
            MonthlySection::NextMonthsFocus => "Next Month's Focus",
        }
    }
}

impl FromStr for MonthlySection {
    type Err = String;

    /// Case-insensitive parse, tolerant of hyphens/underscores and the
    /// apostrophe in "Next Month's Focus". The error string names the
    /// allowlist so the MCP layer can surface it verbatim.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalised = s
            .trim()
            .to_ascii_lowercase()
            .replace(['-', '_'], " ")
            .replace('\'', "");
        match normalised.as_str() {
            "wins" => Ok(MonthlySection::Wins),
            "themes" => Ok(MonthlySection::Themes),
            "next months focus" => Ok(MonthlySection::NextMonthsFocus),
            other => Err(format!(
                "unknown monthly section '{other}' (expected one of: \
                 wins, themes, next month's focus)"
            )),
        }
    }
}

impl Vault {
    /// Read the monthly note for the calendar month containing `month_of`.
    ///
    /// Returns `exists: false` with empty markdown when the month has no
    /// note yet, so callers can test for an existing review without
    /// catching an error.
    pub fn read_monthly_note(&self, month_of: NaiveDate) -> Result<MonthlyNoteView, DomainError> {
        let path = monthly_note_path(month_of)?;
        if self.store.exists(&path)? {
            let markdown = self.store.read_file(&path)?;
            Ok(MonthlyNoteView {
                path,
                exists: true,
                markdown,
            })
        } else {
            Ok(MonthlyNoteView {
                path,
                exists: false,
                markdown: String::new(),
            })
        }
    }

    /// Write one section of the monthly note for the calendar month
    /// containing `month_of`, returning the note's path.
    ///
    /// Creates the note (frontmatter + the three-section scaffold and the
    /// `## Weeks` links block) if it doesn't exist, then `ensure_section`
    /// followed by either `replace_section` (`append: false` —
    /// compose/overwrite the section, the default for a review pass) or
    /// `append_to_section` (`append: true` — accrue within a section
    /// across a session). Mirrors [`Vault::upsert_weekly_section`].
    pub fn upsert_monthly_section(
        &self,
        month_of: NaiveDate,
        section: MonthlySection,
        content: &str,
        append: bool,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let path = monthly_note_path(month_of)?;
        let heading = section.heading();
        let body = format_section_body(content);

        let base = if self.store.exists(&path)? {
            self.store.read_file(&path)?
        } else {
            self.scaffold_monthly_base(month_of)?
        };

        let mut doc = MarkdownDocument::parse(base)?;
        doc.ensure_section(heading)?;
        if append {
            doc.append_to_section(heading, &body)?;
        } else {
            doc.replace_section(heading, &body)?;
        }
        let new_content = doc.render().to_owned();

        let entry_meta = build_index_entry_for(&path, &new_content, "monthly")?;

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        tx.commit()?;

        Ok(path)
    }
}

/// The vault path of the monthly note for the calendar month containing
/// `date`.
pub(in crate::vault) fn monthly_note_path(date: NaiveDate) -> Result<VaultPath, DomainError> {
    Ok(VaultPath::new(cdno_core::paths::monthly_note_relpath(
        date,
    ))?)
}

/// The first day of `date`'s calendar month. Used to normalise any day in
/// the month to a stable key for the scaffold's period fields.
fn first_of_month(date: NaiveDate) -> NaiveDate {
    // `with_day(1)` never fails: day 1 is valid for every month.
    date.with_day(1).expect("day 1 is valid for every month")
}

/// The last day of `date`'s calendar month. Computed as the day before
/// the first of the next month, which sidesteps per-month length and
/// leap-year special-casing.
fn last_of_month(date: NaiveDate) -> NaiveDate {
    let (y, m) = (date.year(), date.month());
    let first_of_next = if m == 12 {
        NaiveDate::from_ymd_opt(y + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(y, m + 1, 1)
    }
    .expect("first of the next month is always a valid date");
    first_of_next - Duration::days(1)
}

/// Every Monday whose date falls within `date`'s calendar month, in
/// chronological order.
///
/// "Falls within the calendar month" is by the Monday's own date, not by
/// its ISO week — so a week that straddles a month boundary is listed
/// under whichever month its Monday lands in, and never in both. This is
/// what the monthly scaffold links: one weekly note per Monday in the
/// month.
pub(in crate::vault) fn mondays_in_month(date: NaiveDate) -> Vec<NaiveDate> {
    let first = first_of_month(date);
    let last = last_of_month(date);
    // Step forward from the first Monday on or after the 1st. `weekday()`
    // days-from-Monday is 0 on a Monday, so subtracting it from a Monday
    // is a no-op and from any other day rewinds to the prior Monday; add
    // 7 and re-take the offset to land on the first Monday >= `first`.
    let offset = i64::from(first.weekday().num_days_from_monday());
    let mut monday = if offset == 0 {
        first
    } else {
        first + Duration::days(7 - offset)
    };
    let mut mondays = Vec::new();
    while monday <= last {
        mondays.push(monday);
        monday += Duration::days(7);
    }
    mondays
}

impl Vault {
    /// Scaffold a fresh monthly note per design §5.2.1: calendar-month
    /// frontmatter (`month` = `YYYY-MM`, `date_start` = first of month,
    /// `date_end` = last of month), a `# <Month> YYYY` heading, the three
    /// empty review sections, and a `## Weeks` block linking every weekly
    /// note whose Monday falls in the month. Keyed off the calendar month,
    /// so any `date` within the month produces the same note. Rendered
    /// through the template engine (#212).
    pub(in crate::vault) fn scaffold_monthly_base(
        &self,
        date: NaiveDate,
    ) -> Result<String, DomainError> {
        let first = first_of_month(date);
        let last = last_of_month(date);
        let mut ctx = VariableContext::new();
        ctx.set_contextual("month", first.format("%Y-%m").to_string());
        // Full month name for the `# July 2026` heading; `%B` is the
        // locale-independent English month name chrono ships.
        ctx.set_contextual("month_name", first.format("%B").to_string());
        ctx.set_contextual("year", first.year().to_string());
        ctx.set_contextual("date_start", first.format("%Y-%m-%d").to_string());
        ctx.set_contextual("date_end", last.format("%Y-%m-%d").to_string());
        ctx.set_contextual("weeks", weeks_link_block(date));
        self.scaffold("monthly", None, &mut ctx)
    }
}

/// Build the `## Weeks` block body: one wikilink bullet per Monday in the
/// month, pointing at that Monday's weekly note. The link is emitted even
/// if the weekly note doesn't exist yet — the reader resolves it (or
/// renders it muted) once the week is started. The `.md` suffix is
/// dropped so the target matches the wikilink resolver's path form.
fn weeks_link_block(date: NaiveDate) -> String {
    let mondays = mondays_in_month(date);
    if mondays.is_empty() {
        // Defensive: every calendar month contains at least four Mondays,
        // so this is unreachable, but returning an empty string keeps the
        // scaffold well-formed rather than leaving a dangling placeholder.
        return String::new();
    }
    mondays
        .into_iter()
        .map(|monday| {
            let rel = cdno_core::paths::weekly_note_relpath(monday);
            let target = rel.strip_suffix(".md").unwrap_or(&rel);
            format!("- [[{target}]]")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
