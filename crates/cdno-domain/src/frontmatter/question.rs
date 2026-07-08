//! Question note frontmatter: typed view of `type: question` YAML
//! headers (design §5.8).
//!
//! The question text itself is the H1 of the body, not a frontmatter
//! field — questions are long, often multi-line, and stay readable as
//! markdown headings rather than buried in YAML. The frontmatter just
//! carries the lifecycle bits (domain, status, dates) so the
//! `active_questions()` query can run against the index without
//! reading each file.

use std::str::FromStr;

use cdno_core::error::ValidationError;
use cdno_core::frontmatter::Frontmatter;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Which kind of question this is. Drives the on-disk folder
/// (`questions/research/` vs `questions/life/`). Closed enum: design
/// §5.8 names exactly these two; adding a third should be a conscious
/// decision rather than a typo silently creating a new folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
pub enum QuestionDomain {
    Research,
    Life,
}

impl QuestionDomain {
    /// Every variant in declaration order. Drives iteration in tests
    /// and (eventually) renderers that group by domain.
    pub const ALL: [QuestionDomain; 2] = [QuestionDomain::Research, QuestionDomain::Life];

    /// Kebab-case YAML / CLI form. Also the on-disk folder name under
    /// `questions/`.
    pub fn as_str(self) -> &'static str {
        match self {
            QuestionDomain::Research => "research",
            QuestionDomain::Life => "life",
        }
    }
}

/// Error returned when a string does not match any [`QuestionDomain`]
/// variant.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown question domain: {0} (expected: research or life)")]
pub struct ParseQuestionDomainError(pub String);

impl FromStr for QuestionDomain {
    type Err = ParseQuestionDomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        QuestionDomain::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| ParseQuestionDomainError(s.to_owned()))
    }
}

/// Lifecycle state of a question. The four variants are the design
/// vocabulary (§5.8): `active` lives on the orientation surface,
/// `parked` is interesting-but-not-now, `answered` is resolved-and-
/// kept-for-reference, `retired` is no-longer-relevant. The file
/// stays in place across every transition — only the frontmatter
/// changes — so backlinks from projects and portfolios survive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
pub enum QuestionStatus {
    Active,
    Parked,
    Answered,
    Retired,
}

impl QuestionStatus {
    /// Every variant in declaration order.
    pub const ALL: [QuestionStatus; 4] = [
        QuestionStatus::Active,
        QuestionStatus::Parked,
        QuestionStatus::Answered,
        QuestionStatus::Retired,
    ];

    /// Kebab-case YAML / CLI form.
    pub fn as_str(self) -> &'static str {
        match self {
            QuestionStatus::Active => "active",
            QuestionStatus::Parked => "parked",
            QuestionStatus::Answered => "answered",
            QuestionStatus::Retired => "retired",
        }
    }
}

/// Error returned when a string does not match any [`QuestionStatus`]
/// variant.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown question status: {0} (expected: active, parked, answered, or retired)")]
pub struct ParseQuestionStatusError(pub String);

impl FromStr for QuestionStatus {
    type Err = ParseQuestionStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        QuestionStatus::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| ParseQuestionStatusError(s.to_owned()))
    }
}

/// Parsed and validated frontmatter for a question note. Once this
/// struct exists, every required field is guaranteed present and
/// well-typed — downstream code does not re-validate.
///
/// `updated` mirrors mtime semantically but is stored explicitly so
/// it's stable across filesystem moves and lives in git-tracked
/// content. `set_question_status` bumps it on every transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionFrontmatter {
    pub domain: QuestionDomain,
    pub status: QuestionStatus,
    pub created: NaiveDate,
    pub updated: NaiveDate,
}

impl TryFrom<Frontmatter> for QuestionFrontmatter {
    type Error = ValidationError;

    fn try_from(fm: Frontmatter) -> Result<Self, Self::Error> {
        Ok(Self {
            domain: fm.require_field::<QuestionDomain>("domain")?,
            status: fm.require_field::<QuestionStatus>("status")?,
            created: fm.require_field::<NaiveDate>("created")?,
            updated: fm.require_field::<NaiveDate>("updated")?,
        })
    }
}
