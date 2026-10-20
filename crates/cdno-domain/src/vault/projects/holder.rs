//! `hand_off`: record whose move an open project is, or take it back
//! (RFC 0006 §7.2 Practice 8, §8.2 "Verb").
//!
//! One operation with an explicit [`HolderChange`], never "holder
//! omitted": handing off writes `held_by: "<name>"` on the map, taking
//! back writes `held_by: null`, and each change appends a
//! `holder on [[<slug>]]` entry with indented `was:` / `now:` lines to
//! today's daily log, in the mutable-field family `set_core_question`
//! belongs to. The entry is not a focus marker, so the lint prefix list
//! (`FOCUS_MARKER_PREFIXES`) does not name it.
//!
//! The three guards the verb carries:
//!
//! - **A holder is a name.** A value that names you or nobody (the
//!   configured `[projects] holder_sentinels`) is refused with
//!   [`DomainError::HolderNotAName`]: if you cannot name who has it, it
//!   is yours. The value is always written as a double-quoted YAML
//!   scalar, so the verb never writes a shape the lenient reader has to
//!   forgive, and a quoted wikilink holder becomes a link row in the
//!   index.
//! - **Not in the middle of its work.** A hand-off while an action on
//!   the map is in focus is [`DomainError::ProjectInFocus`]; the focus is
//!   read once under the write lock, as the start verbs read it.
//! - **The cap rule.** Taking back an active held map makes it count,
//!   so it passes [`Vault::ensure_slot`]. Taking back a parked map only
//!   clears the name: it needs no slot and does not activate.

use chrono::NaiveDateTime;
use serde_json::Value;

use crate::error::DomainError;
use crate::frontmatter::{
    HolderReading, HolderSentinels, ProjectFrontmatter, ProjectStatus, classify_holder_text,
};
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::WriteOutcome;
use super::super::frontmatter_edit::merge_fields_into_frontmatter;
use super::super::index_entry::build_index_entry_for;
use super::rewrite_field_in_frontmatter;

/// The frontmatter key the verb owns. Reserved from `set_frontmatter`.
const HELD_BY: &str = "held_by";

/// Log prefix for a holder change: `holder on [[<slug>]]`, followed by
/// indented `was:` / `now:` lines. A named constant so the writer and any
/// reader cannot drift. Not a focus marker.
pub const LOG_HOLDER_PREFIX: &str = "holder on ";

/// Rendered stand-in for "no holder" on either side of a holder change:
/// the map is the person's own. Also on the default
/// `[projects] holder_sentinels`, so a person copying it back from the log
/// into `held_by:` writes a value that reads as theirs.
pub const NO_HOLDER: &str = "(mine)";

/// What a [`Vault::hand_off`] call changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HolderChange {
    /// Name who else is moving the map, as the person gives it (trimmed).
    HandTo(String),
    /// Strike the name: the map is the person's again.
    TakeBack,
}

impl Vault {
    /// Hand an open (active or parked) project to a named holder, or take
    /// it back (RFC 0006 §7.2, §8.2).
    ///
    /// Validation runs before the write lock: a blank name is
    /// [`DomainError::EmptyField`] (`held_by`); a name on the configured
    /// `[projects] holder_sentinels` (one naming you or nobody) is
    /// [`DomainError::HolderNotAName`]; a date, which the reader cannot
    /// honour as a name, is [`DomainError::InvalidFieldValue`]; a line
    /// break or other control character is
    /// [`DomainError::UnrepresentableFrontmatterValue`].
    ///
    /// Under the lock the map is resolved with
    /// [`resolve_open_project`](Vault::resolve_open_project) (a closed or
    /// misfiled map is [`DomainError::ProjectNotActive`]). A change that
    /// would leave the holder as it reads now (the same name again, or a
    /// take-back on a map that is already yours) is
    /// [`WriteOutcome::noop`], with no log entry and no guard consulted.
    /// Otherwise a hand-off is refused with
    /// [`DomainError::ProjectInFocus`] while an action on this map is in
    /// focus, and a take-back of an active map that counts afterwards and
    /// did not before passes [`Vault::ensure_slot`]
    /// ([`DomainError::ProjectCapReached`] at the cap). A take-back on a
    /// parked map clears the name only.
    ///
    /// The write replaces `held_by` where it stands, or inserts it before
    /// the closing `---`; the map's other lines are untouched. The index
    /// entry is upserted in the same transaction as the log entry.
    ///
    /// `at` is a parameter so tests can pin the log timestamp and the
    /// daily-note date; production callers pass
    /// `chrono::Local::now().naive_local()`.
    pub fn hand_off(
        &self,
        at: NaiveDateTime,
        slug: &str,
        change: HolderChange,
    ) -> Result<WriteOutcome, DomainError> {
        let sentinels = HolderSentinels::from_config(&self.config);
        let new_holder = match &change {
            HolderChange::HandTo(name) => Some(validate_holder(name, &sentinels)?),
            HolderChange::TakeBack => None,
        };

        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let (path, doc) = self.resolve_open_project(slug)?;
        let frontmatter = ProjectFrontmatter::try_from(doc.frontmatter().clone())?;

        // What the map says now, as every counting surface reads it: only a
        // plain name of someone else is a holder; anything else is yours.
        let previous = match frontmatter.holder(&sentinels) {
            HolderReading::Named(name) => Some(name),
            HolderReading::Absent | HolderReading::NotAString | HolderReading::Sentinel(_) => None,
        };
        if previous == new_holder {
            return Ok(WriteOutcome::noop(path));
        }

        match &new_holder {
            Some(_) => {
                // One focus read, under the lock, as the start verbs make
                // it: you do not hand off a map while you are in the middle
                // of its work (RFC 0006 §7.2).
                if let Some(open) = self.current_focus(at.date())?
                    && open.project == slug
                {
                    return Err(DomainError::ProjectInFocus {
                        slug: slug.to_owned(),
                        action: open.action,
                    });
                }
            }
            None => {
                // The one cap rule: a map that did not count and will. Only
                // an active map can begin to count; a parked one stays
                // parked and needs no slot.
                if frontmatter.status == ProjectStatus::Active {
                    let counts_before = frontmatter.counts_toward_cap(&sentinels);
                    self.ensure_slot(counts_before, true)?;
                }
            }
        }

        let new_content = write_holder(doc.render(), new_holder.as_deref())?;
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;
        let log_entry = format_holder_log_entry(slug, previous.as_deref(), new_holder.as_deref());

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        let touched = tx.commit()?;

        Ok(WriteOutcome::written(path, touched))
    }
}

/// Check a holder as given and return it trimmed, or the refusal.
///
/// The verb refuses every value the reader would not honour as a name, so
/// a written holder always reads back as [`HolderReading::Named`] and the
/// map stops counting exactly when the person said so.
fn validate_holder(name: &str, sentinels: &HolderSentinels) -> Result<String, DomainError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(DomainError::EmptyField { field: HELD_BY });
    }
    // A line break would end the scalar; any other control character is not
    // printable YAML. Tab is legal inside a double-quoted scalar.
    if trimmed
        .chars()
        .any(|c| (c.is_control() && c != '\t') || c == '\u{2028}' || c == '\u{2029}')
    {
        return Err(DomainError::UnrepresentableFrontmatterValue {
            field: HELD_BY.to_owned(),
            reason: "a holder is one line of text, with no line break or control character"
                .to_owned(),
        });
    }
    match classify_holder_text(trimmed, sentinels) {
        HolderReading::Named(named) => Ok(named),
        HolderReading::Sentinel(_) => Err(DomainError::HolderNotAName {
            value: trimmed.to_owned(),
        }),
        HolderReading::NotAString => Err(DomainError::InvalidFieldValue {
            note_type: NoteType::Project.as_str().to_owned(),
            field: HELD_BY.to_owned(),
            reason: format!("'{trimmed}' is a date, which names no one: name who has the project"),
        }),
        // `classify_holder_text` reads text, which is never absent.
        HolderReading::Absent => Err(DomainError::EmptyField { field: HELD_BY }),
    }
}

/// Write `held_by` into `raw`'s frontmatter: the holder as a double-quoted
/// scalar, or `null` for none.
///
/// Inserted when absent, replaced where it stands when present (a value
/// spanning lines, such as a hand-written block sequence, is replaced
/// whole). The merge writes `held_by: null`, which the single-line rewrite
/// then quotes: the merge's emitter quotes only when YAML needs it, and
/// this verb always quotes.
fn write_holder(raw: &str, holder: Option<&str>) -> Result<String, DomainError> {
    let mut fields = serde_json::Map::new();
    fields.insert(HELD_BY.to_owned(), Value::Null);
    let cleared = merge_fields_into_frontmatter(raw, &fields)?;
    match holder {
        None => Ok(cleared),
        Some(name) => rewrite_field_in_frontmatter(&cleared, HELD_BY, &yaml_double_quoted(name)),
    }
}

/// `name` as a double-quoted YAML scalar, `\` and `"` escaped.
fn yaml_double_quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Build the daily-log entry recording a holder change, in the
/// header-plus-indented-`was:`/`now:` shape of the project map's other
/// mutable fields (`state on`, `core question on`), so "how did this
/// project change" stays one search over the daily log. The absent side
/// renders as [`NO_HOLDER`].
fn format_holder_log_entry(slug: &str, previous: Option<&str>, new: Option<&str>) -> String {
    format!(
        "{LOG_HOLDER_PREFIX}[[{slug}]]\n  was: {}\n  now: {}",
        previous.unwrap_or(NO_HOLDER),
        new.unwrap_or(NO_HOLDER),
    )
}
