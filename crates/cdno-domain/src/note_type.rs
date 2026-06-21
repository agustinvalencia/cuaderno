use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The eleven canonical note types that make up a Cuaderno vault.
///
/// Serialises and parses as kebab-case (`Daily` ↔ `"daily"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoteType {
    Daily,
    Weekly,
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
    pub const ALL: [NoteType; 11] = [
        NoteType::Daily,
        NoteType::Weekly,
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

    /// The canonical frontmatter key order for this note type, with
    /// `type` first. This mirrors the field order each type's creation
    /// produces — the file templates in `crates/cdno-domain/templates/`
    /// and the in-code scaffolds for daily/weekly/inbox. Tests pin both
    /// to this list so they can't drift: a file-sync test for the
    /// single-file templates, and behavioural "fresh scaffold matches
    /// frontmatter_order" tests for the code-scaffolded types. The
    /// frontmatter normaliser (#233) reorders a note's known keys into
    /// this order; keys not listed here (hand-added, type-specific
    /// extras) keep their relative order after the known ones.
    pub fn frontmatter_order(self) -> &'static [&'static str] {
        match self {
            NoteType::Daily => &["type", "date"],
            NoteType::Weekly => &["type", "week", "date_start", "date_end"],
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
