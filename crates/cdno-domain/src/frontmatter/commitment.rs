//! Commitment note frontmatter: typed view of `type: commitment`
//! YAML headers.
//!
//! See `docs/design.md` §5.9 for the layout. Commitments live in
//! `commitments/` while active and move to `commitments/_done/<year>/`
//! on completion. Both `status` and `completed` are recorded in the
//! frontmatter so queries (the commitments aggregation in #32, weekly
//! / monthly reviews) can run as index lookups rather than filesystem
//! walks.

use std::str::FromStr;

use cdno_core::error::ValidationError;
use cdno_core::frontmatter::Frontmatter;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::context::Context;

/// Lifecycle state of a commitment. Created `Active`; flipped to
/// `Completed` by `Vault::complete_commitment` in the same
/// transaction that moves the file to `_done/<year>/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommitmentStatus {
    Active,
    Completed,
}

impl CommitmentStatus {
    /// Every variant in declaration order.
    pub const ALL: [CommitmentStatus; 2] = [CommitmentStatus::Active, CommitmentStatus::Completed];

    /// Kebab-case YAML / CLI form.
    pub fn as_str(self) -> &'static str {
        match self {
            CommitmentStatus::Active => "active",
            CommitmentStatus::Completed => "completed",
        }
    }
}

/// Error returned when a string does not match any
/// [`CommitmentStatus`] variant.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown commitment status: {0} (expected: active or completed)")]
pub struct ParseCommitmentStatusError(pub String);

impl FromStr for CommitmentStatus {
    type Err = ParseCommitmentStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CommitmentStatus::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| ParseCommitmentStatusError(s.to_owned()))
    }
}

/// Parsed and validated frontmatter for a commitment note. Once this
/// struct exists, every required field is guaranteed present and
/// well-typed — downstream code does not re-validate.
///
/// `project` and `stewardship` are wikilink targets when the
/// commitment originates from one of those sources; `None` for
/// standalone commitments. The design doc notes that originating
/// commitments are typically tracked inline (as project milestone
/// dates or stewardship periodic commitments) rather than as separate
/// files, so standalone is the dominant case.
///
/// `completed` is `Some(date)` for completed commitments, `None`
/// while active. The two move together with `status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitmentFrontmatter {
    pub status: CommitmentStatus,
    pub due: NaiveDate,
    pub created: NaiveDate,
    pub completed: Option<NaiveDate>,
    pub context: Context,
    pub project: Option<String>,
    pub stewardship: Option<String>,
}

impl TryFrom<Frontmatter> for CommitmentFrontmatter {
    type Error = ValidationError;

    fn try_from(fm: Frontmatter) -> Result<Self, Self::Error> {
        Ok(Self {
            status: fm.require_field::<CommitmentStatus>("status")?,
            due: fm.require_field::<NaiveDate>("due")?,
            created: fm.require_field::<NaiveDate>("created")?,
            completed: fm.optional_field::<NaiveDate>("completed")?,
            context: fm.require_field::<Context>("context")?,
            project: fm.optional_field::<String>("project")?,
            stewardship: fm.optional_field::<String>("stewardship")?,
        })
    }
}
