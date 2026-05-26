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
