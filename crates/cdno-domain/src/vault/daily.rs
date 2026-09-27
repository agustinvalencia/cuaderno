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
//! The daily note is append-only for its **history**: `## Logs` and
//! `## Notes` only ever grow. `## Logs` grows via
//! [`Vault::log_to_daily_note`]; `## Notes` (RFC 0002 §5.4) is the
//! substance section — worked-out material that would otherwise bloat
//! `## Logs` — and grows through `upsert_daily_section` itself, which
//! forces `append = true` for it rather than routing it through a
//! separate writer. The other sections — Standup, Intention, Agenda
//! (mutable planning scratch, typically replaced) and Meeting (live
//! notes, typically appended) — are freely writable via
//! `upsert_daily_section`. [`DailySection`] is the type-level allowlist;
//! [`DailySection::is_history`] is what keeps the history sections from
//! being replaced outright.
//!
//! History sections only *grow* — but growth is still constrained:
//! [`Vault::validate_history_entry`] rejects a body appended to `##
//! Notes` unless every heading inside it is level-3-or-deeper and reuses
//! no daily-section name (Standup/Intention/Agenda/Meeting/Notes/Logs/the
//! template's anchor section), no heading already present in that day's
//! note, and no other heading in the same body — headings compared as
//! `MarkdownDocument` sees them (markup stripped, code fences skipped,
//! setext included). Without that check, a `### Standup` entry inside
//! `## Notes` would collide with the level-2 `## Standup` heading that a
//! later planning upsert looks up by flat text match — silently
//! replacing (or making ambiguous and unwritable) the wrong section, and
//! a `### Logs` entry would do the same to `log_to_daily_note`. The
//! overwrite path itself never touches `## Notes`'s heading; this check
//! is what keeps *entries inside it* from impersonating a section.

use std::str::FromStr;

use chrono::{Datelike, NaiveDate};

use cdno_core::markdown::{MarkdownDocument, headings};
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

/// The sections of a daily note that [`Vault::upsert_daily_section`]
/// may write. `Standup`/`Intention`/`Agenda` are mutable planning
/// scratch (typically replaced); `Meeting` accrues live meeting notes
/// (typically appended); `Notes` is the append-only substance section
/// (RFC 0002 §5.4) — see [`DailySection::is_history`]. `## Logs` is
/// deliberately not a variant here at all: it grows only via
/// [`Vault::log_to_daily_note`] and cannot be reached through this
/// allowlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DailySection {
    Standup,
    Intention,
    Agenda,
    Meeting,
    Notes,
}

impl DailySection {
    /// The level-2 heading text this section maps to.
    pub fn heading(self) -> &'static str {
        match self {
            DailySection::Standup => "Standup",
            DailySection::Intention => "Intention",
            DailySection::Agenda => "Agenda",
            DailySection::Meeting => "Meeting",
            DailySection::Notes => "Notes",
        }
    }

    /// Whether this section is a **history** section: append-only, like
    /// `## Logs`. Only [`DailySection::Notes`] is — it only ever grows,
    /// so [`Vault::upsert_daily_section`] forces `append = true` for it
    /// and refuses a replace outright, the same invariant `## Logs`
    /// gets by not being reachable through this enum at all.
    pub fn is_history(self) -> bool {
        matches!(self, DailySection::Notes)
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
            "notes" => Ok(DailySection::Notes),
            other => Err(format!(
                "unknown daily section '{other}' (expected one of: standup, intention, agenda, meeting, notes)"
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

    /// Write a section of the daily note for `date`, returning the
    /// note's path.
    ///
    /// A history section (`section.is_history()` — currently just
    /// [`DailySection::Notes`]) refuses `append: false` outright,
    /// before anything is read or written, with
    /// [`DomainError::HistorySectionNotReplaceable`]. Otherwise: creates
    /// the daily note (with an empty `## Logs`) if it doesn't exist,
    /// then `ensure_section` followed by either `replace_section`
    /// (`append: false` — the planning sections, idempotent overwrite)
    /// or `append_to_section` (`append: true` — live meeting notes, or
    /// `## Notes` entries, that accrue). The `## Logs` history content is
    /// never clobbered — the write targets `section`'s heading alone —
    /// and `move_section_to_end` then pins `## Logs` back to the bottom
    /// so a section created mid-day (planning, or a first `## Notes`
    /// entry) can't strand the history above it (#232). A brand-new `##
    /// Notes` section is placed immediately before the note's anchor
    /// section (`## Logs` by default, or whichever trailing section a
    /// custom daily template names, per [`Vault::daily_anchor_section`]);
    /// a planning section created later can land between the two.
    ///
    /// For a history section, the content is first checked with
    /// [`Vault::validate_history_entry`] — see that method for what an
    /// entry may and may not contain. That check runs **under** the
    /// transaction's write lock: it reads the note to test for duplicate
    /// headings, so running it before the lock would be a check-then-act
    /// race with a concurrent `## Notes` writer.
    pub fn upsert_daily_section(
        &self,
        date: NaiveDate,
        section: DailySection,
        content: &str,
        append: bool,
    ) -> Result<VaultPath, DomainError> {
        // Reads nothing, so it may refuse before the lock is taken.
        if section.is_history() && !append {
            return Err(DomainError::HistorySectionNotReplaceable {
                section: section.heading().to_string(),
            });
        }

        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        // Validate only once the lock is held: the duplicate-heading
        // check reads the note, and the read must see what this write
        // folds into.
        if section.is_history() {
            self.validate_history_entry(date, section, content)?;
        }
        let path = daily_note_path(date)?;
        let base = self.read_or_scaffold_daily(date)?;
        let new_content = self.fold_daily_section(base, section, content, append)?;

        let entry_meta = build_index_entry_for(&path, &new_content, "daily")?;

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        tx.commit()?;

        Ok(path)
    }

    /// The daily note for `date` as it stands in the store, or a fresh
    /// scaffold when no note exists yet — the base every daily-note
    /// write folds its change into.
    pub(in crate::vault) fn read_or_scaffold_daily(
        &self,
        date: NaiveDate,
    ) -> Result<String, DomainError> {
        let path = daily_note_path(date)?;
        if self.store.exists(&path)? {
            Ok(self.store.read_file(&path)?)
        } else {
            self.scaffold_daily_base(date)
        }
    }

    /// Fold a write of `section` into `base` — an already-materialised
    /// daily-note document — returning the rendered content, without
    /// touching the store. This is the whole of
    /// [`Vault::upsert_daily_section`]'s document edit: `ensure_section`,
    /// then `append_to_section` or `replace_section`, then re-pin the
    /// anchor section to the bottom.
    ///
    /// Split out (RFC 0002 T2) so `note_to_daily` can append a `## Notes`
    /// entry *and* fold its `## Logs` pointer line into the same
    /// in-flight content, writing the file once in one transaction —
    /// staging two separate writes to the same file would have the
    /// second read the pre-change content back from the store and drop
    /// the first. Performs no validation: the caller runs the
    /// history-section guard and [`Vault::validate_history_entry`]
    /// first — the latter with the transaction's write lock already
    /// held, so the note it checks is the note this folds into.
    pub(in crate::vault) fn fold_daily_section(
        &self,
        base: String,
        section: DailySection,
        content: &str,
        append: bool,
    ) -> Result<String, DomainError> {
        let heading = section.heading();
        let body = format_section_body(content);

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
        Ok(doc.render().to_owned())
    }

    /// Validate a body about to be appended to a **history** section
    /// (currently only [`DailySection::Notes`]) before it is folded into
    /// the note. Callers run it with the transaction's write lock held,
    /// since the duplicate check below reads the note.
    ///
    /// A history section only ever grows, one entry at a time, and an
    /// entry is conventionally a `### <title>` heading followed by its
    /// content. Heading lookup elsewhere in the vault (`ensure_section`,
    /// `replace_section`, `## Logs` appends) is flat and level-blind, so
    /// an entry heading that happens to match a real section name would
    /// collide with it — a later `Standup` upsert would silently replace
    /// the `### Standup` entry instead of the `## Standup` section (or,
    /// once a real `## Standup` section exists, every future `Standup`
    /// upsert would fail as ambiguous), and a `### Logs` entry would make
    /// `log_to_daily_note` unwritable the same way.
    ///
    /// Headings are found with [`cdno_core::markdown::headings`], the same
    /// `pulldown-cmark` scan `MarkdownDocument` uses for section lookup,
    /// on both the existing note and `content`. So the check and the
    /// lookup agree on what a heading is and what it is called: ATX and
    /// setext headings both count, a `#` line inside a fenced code block
    /// does not, and a heading is compared by its text **with inline
    /// markup stripped** (`*Notes*` is `Notes`, `` `Foo` `` is `Foo`).
    /// Names are compared case-insensitively with Unicode lower-casing.
    /// Every heading in `content` is rejected when it:
    ///
    /// - is level 1 or 2 (including a setext heading) — only
    ///   level-3-or-deeper headings are entries; a level-1/2 heading is
    ///   section-shaped and would itself be mistaken for one;
    /// - matches a daily-section heading
    ///   (Standup/Intention/Agenda/Meeting/Notes), `Logs`
    ///   ([`super::DAILY_LOGS_SECTION`]), or the effective template's
    ///   anchor section ([`Vault::daily_anchor_section`]);
    /// - matches a heading already present anywhere in that day's note —
    ///   checked only when the note already exists; a day with no note
    ///   yet has nothing to collide with; or
    /// - matches an earlier heading in `content` itself, since the entry
    ///   would otherwise make its own headings ambiguous.
    ///
    /// Every refusal is [`DomainError::HistoryEntryHeadingInvalid`],
    /// naming the offending heading by its stripped text.
    ///
    /// Exposed `pub(crate)` so T2's `note_to_daily` can run the same
    /// check on the content it appends.
    pub(crate) fn validate_history_entry(
        &self,
        date: NaiveDate,
        section: DailySection,
        content: &str,
    ) -> Result<(), DomainError> {
        let heading = section.heading();

        let mut reserved: Vec<String> = vec![
            DailySection::Standup.heading().to_lowercase(),
            DailySection::Intention.heading().to_lowercase(),
            DailySection::Agenda.heading().to_lowercase(),
            DailySection::Meeting.heading().to_lowercase(),
            DailySection::Notes.heading().to_lowercase(),
            super::DAILY_LOGS_SECTION.to_lowercase(),
        ];
        let anchor = self.daily_anchor_section()?;
        reserved.push(anchor.to_lowercase());

        let mut existing_headings: Vec<String> = Vec::new();
        let path = daily_note_path(date)?;
        if self.store.exists(&path)? {
            let note = self.store.read_file(&path)?;
            let body = match cdno_core::frontmatter::Frontmatter::parse(&note) {
                Ok((_, body)) => body,
                Err(_) => note.as_str(),
            };
            existing_headings = headings(body)
                .into_iter()
                .map(|(_, text)| text.to_lowercase())
                .collect();
        }

        let invalid = |text: String, reason: &str| DomainError::HistoryEntryHeadingInvalid {
            section: heading.to_string(),
            heading: text,
            reason: reason.to_string(),
        };
        let mut seen: Vec<String> = Vec::new();
        for (level, text) in headings(content) {
            if level <= 2 {
                return Err(invalid(
                    text,
                    "only level-3 or deeper headings are allowed inside a history section",
                ));
            }
            let lower = text.to_lowercase();
            if reserved.contains(&lower) {
                return Err(invalid(text, "it is the name of a daily section"));
            }
            if existing_headings.contains(&lower) || seen.contains(&lower) {
                return Err(invalid(
                    text,
                    "a heading with that text already exists in the note",
                ));
            }
            seen.push(lower);
        }

        Ok(())
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
