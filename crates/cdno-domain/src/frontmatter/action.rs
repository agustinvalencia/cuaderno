//! Action note frontmatter: typed view of `type: action` YAML headers.
//!
//! See `docs/design.md` §5.11 for the layout and the two-state lifecycle
//! (attached / orphaned-after-completion). Action notes are the heavier
//! manifest form of an action — most actions stay as inline bullets in a
//! project's `## Next Actions` section. The note is opt-in via the
//! `--note` flag on `cdno action add`, or via `cdno action promote`
//! against an existing bullet.
//!
//! `milestone` and `due` are both optional and not mutually exclusive.
//! A frontmatter-level XOR rule was considered but rejected as too
//! strict — real cases want both (tied to a milestone *and* an internal
//! self-imposed checkpoint). Deduplication for commitments aggregation
//! lives in the query, not in the schema.

use std::str::FromStr;

use cdno_core::error::ValidationError;
use cdno_core::frontmatter::Frontmatter;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::project::EnergyLevel;

/// Lifecycle state of an action note. Created `Active`; flipped to
/// `Completed` by `Vault::complete_action` in the same transaction
/// that removes the matching bullet from the project map and moves
/// the file to `actions/_done/<year>/`. `Blocked` is set explicitly
/// when external work is gating progress; the `blocker` frontmatter
/// field carries the human description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
pub enum ActionStatus {
    Active,
    Completed,
    Blocked,
}

impl ActionStatus {
    /// Every variant in declaration order.
    pub const ALL: [ActionStatus; 3] = [
        ActionStatus::Active,
        ActionStatus::Completed,
        ActionStatus::Blocked,
    ];

    /// Kebab-case YAML / CLI form.
    pub fn as_str(self) -> &'static str {
        match self {
            ActionStatus::Active => "active",
            ActionStatus::Completed => "completed",
            ActionStatus::Blocked => "blocked",
        }
    }
}

/// Error returned when a string does not match any [`ActionStatus`]
/// variant.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown action status: {0} (expected: active, completed, or blocked)")]
pub struct ParseActionStatusError(pub String);

impl FromStr for ActionStatus {
    type Err = ParseActionStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ActionStatus::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| ParseActionStatusError(s.to_owned()))
    }
}

/// Parsed and validated frontmatter for an action note. Once this
/// struct exists, every required field is guaranteed present and
/// well-typed — downstream code does not re-validate.
///
/// `project` is the slug of the parent project; every action note
/// belongs to exactly one project (the bullet, attached or removed,
/// lives in that project's `## Next Actions`). `milestone` is a raw
/// wikilink string (e.g. `"[[projects/foo#paper-submission]]"`) when
/// the action is pinned to a project milestone — the milestone owns
/// the date and the action inherits it; wikilink resolution lives in
/// a later layer. `due` is a self-imposed deadline used only when the
/// action stands alone (not pinned to a milestone), and is what the
/// commitments aggregation reads as the action's deadline source.
///
/// `completed` is `Some(date)` for completed actions, `None` while
/// active or blocked. The two move together with `status`. `blocker`
/// is `Some(description)` while `status: blocked`, `None` otherwise.
/// `criteria` is free-form text describing what "done" looks like —
/// optional because trivial cases are encoded by the title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionFrontmatter {
    pub status: ActionStatus,
    pub project: String,
    pub energy: EnergyLevel,
    pub milestone: Option<String>,
    pub due: Option<NaiveDate>,
    pub created: NaiveDate,
    pub completed: Option<NaiveDate>,
    pub blocker: Option<String>,
    pub criteria: Option<String>,
    pub tags: Vec<String>,
}

impl TryFrom<Frontmatter> for ActionFrontmatter {
    type Error = ValidationError;

    fn try_from(fm: Frontmatter) -> Result<Self, Self::Error> {
        Ok(Self {
            status: fm.require_field::<ActionStatus>("status")?,
            project: fm.require_field::<String>("project")?,
            energy: fm.require_field::<EnergyLevel>("energy")?,
            milestone: fm.optional_field::<String>("milestone")?,
            due: fm.optional_field::<NaiveDate>("due")?,
            created: fm.require_field::<NaiveDate>("created")?,
            completed: fm.optional_field::<NaiveDate>("completed")?,
            blocker: fm.optional_field::<String>("blocker")?,
            criteria: fm.optional_field::<String>("criteria")?,
            tags: fm
                .optional_field::<Vec<String>>("tags")?
                .unwrap_or_default(),
        })
    }
}
