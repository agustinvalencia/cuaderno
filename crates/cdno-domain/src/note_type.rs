use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The twelve canonical note types that make up a Cuaderno vault.
///
/// Serialises and parses as kebab-case (`Daily` ↔ `"daily"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoteType {
    Daily,
    Weekly,
    Monthly,
    Project,
    Action,
    Portfolio,
    Evidence,
    Stewardship,
    Tracking,
    Question,
    Commitment,
    Inbox,
}

impl NoteType {
    pub const ALL: [NoteType; 12] = [
        NoteType::Daily,
        NoteType::Weekly,
        NoteType::Monthly,
        NoteType::Project,
        NoteType::Action,
        NoteType::Portfolio,
        NoteType::Evidence,
        NoteType::Stewardship,
        NoteType::Tracking,
        NoteType::Question,
        NoteType::Commitment,
        NoteType::Inbox,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            NoteType::Daily => "daily",
            NoteType::Weekly => "weekly",
            NoteType::Monthly => "monthly",
            NoteType::Project => "project",
            NoteType::Action => "action",
            NoteType::Portfolio => "portfolio",
            NoteType::Evidence => "evidence",
            NoteType::Stewardship => "stewardship",
            NoteType::Tracking => "tracking",
            NoteType::Question => "question",
            NoteType::Commitment => "commitment",
            NoteType::Inbox => "inbox",
        }
    }

    /// The canonical *built-in* frontmatter key order for this note type,
    /// with `type` first. This mirrors the field order each type's
    /// creation produces — the file templates in
    /// `crates/cdno-domain/templates/` and the in-code scaffolds for
    /// daily/weekly/inbox. Tests pin both to this list so they can't
    /// drift: a file-sync test for the single-file templates, and
    /// behavioural "fresh scaffold matches frontmatter_order" tests for
    /// the code-scaffolded types.
    ///
    /// Since #212 the normaliser does *not* read this directly — it
    /// derives a note's canonical order from the note's *effective*
    /// template (a custom `.cuaderno/templates/` override, else the
    /// built-in). For the single-file types a sync test pins this list to
    /// the built-in template order, so on an un-customised vault the two
    /// agree. `Daily` and `Tracking` are excluded from that sync test:
    /// `Tracking`'s list is a superset that also carries the optional
    /// variant-shape fields (`duration_min`, `routine`) which the generic
    /// built-in template doesn't reference (only a vault variant would),
    /// so it stays a field-vocabulary reference rather than a
    /// built-in-order guard for tracking.
    pub fn frontmatter_order(self) -> &'static [&'static str] {
        match self {
            NoteType::Daily => &["type", "date"],
            NoteType::Weekly => &["type", "week", "date_start", "date_end"],
            NoteType::Monthly => &["type", "month", "date_start", "date_end"],
            NoteType::Project => &["type", "context", "status", "created", "core_question"],
            NoteType::Action => &[
                "type",
                "status",
                "project",
                "energy",
                "milestone",
                "due",
                "created",
                "completed",
                "blocker",
                "criteria",
                "tags",
            ],
            NoteType::Portfolio => &["type", "question", "created", "project"],
            NoteType::Evidence => &["type", "created", "source", "portfolio", "origin"],
            NoteType::Stewardship => &["type", "context"],
            NoteType::Tracking => &[
                "type",
                "stewardship",
                "activity",
                "date",
                "duration_min",
                "routine",
            ],
            NoteType::Question => &["type", "domain", "status", "created", "updated"],
            NoteType::Commitment => &[
                "type",
                "status",
                "due",
                "created",
                "completed",
                "context",
                "project",
                "stewardship",
            ],
            NoteType::Inbox => &["type", "created"],
        }
    }

    /// The complete set of `{{placeholders}}` this note type's create path
    /// supplies — the authoritative "what a custom template may reference"
    /// list, in a sensible display order.
    ///
    /// This overlaps [`Self::frontmatter_order`] but neither contains the
    /// other: this list drops non-placeholder frontmatter (`type`, and keys the
    /// template hardcodes or writes from the typed struct like `status`/
    /// `duration_min`) and adds body placeholders (`title`/`heading`/`content`/
    /// `activity_title`/…) plus keys the *default* template happens not to
    /// reference (e.g. `daily`'s `weekday`, `tracking`'s `routine`).
    ///
    /// It is the source of truth for `cdno templates vars` and mirrors the
    /// per-type `set_contextual` calls in each create path. A drift test asserts
    /// every built-in template only references names from this set (so a
    /// template can't reference a key that would render literally); the handful
    /// of keys no built-in template uses are covered by dedicated resolve tests.
    pub fn supplied_placeholders(self) -> &'static [&'static str] {
        match self {
            NoteType::Daily => &["date", "heading", "weekday"],
            NoteType::Weekly => &["week", "week_num", "year", "date_start", "date_end"],
            NoteType::Monthly => &[
                "month",
                "month_name",
                "year",
                "date_start",
                "date_end",
                "weeks",
            ],
            NoteType::Project => &["title", "context", "status", "created", "core_question"],
            NoteType::Action => &[
                "title",
                "slug",
                "project",
                "energy",
                "status",
                "created",
                "due",
                "completed",
                "milestone",
                "criteria",
                "blocker",
                "tags",
            ],
            NoteType::Portfolio => &["question", "project", "created"],
            NoteType::Evidence => &["source", "origin", "portfolio", "content", "created"],
            NoteType::Stewardship => &["name", "context"],
            NoteType::Tracking => &[
                "stewardship",
                "activity",
                "activity_title",
                "routine",
                "content",
                "date",
                "date_long",
            ],
            NoteType::Question => &["question", "domain", "created", "updated"],
            NoteType::Commitment => &[
                "title",
                "context",
                "status",
                "due",
                "project",
                "stewardship",
                "created",
                "completed",
            ],
            NoteType::Inbox => &["body", "created"],
        }
    }
}

impl fmt::Display for NoteType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when a string does not match any [`NoteType`] variant.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown note type: {0}")]
pub struct ParseNoteTypeError(pub String);

impl FromStr for NoteType {
    type Err = ParseNoteTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        for variant in NoteType::ALL {
            if variant.as_str() == s {
                return Ok(variant);
            }
        }
        Err(ParseNoteTypeError(s.to_owned()))
    }
}
