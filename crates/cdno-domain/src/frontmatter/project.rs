//! Project map frontmatter: typed view of `type: project` YAML headers.
//!
//! See `docs/design.md` §5.3 (project map shape) and §5.10 (canonical
//! context list and rationale for compile-time enums).

use std::str::FromStr;

use cdno_core::error::ValidationError;
use cdno_core::frontmatter::Frontmatter;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::context::Context;
use super::holder::{HolderReading, HolderSentinels, classify_holder_text};

/// Lifecycle state of a project. Park/activate transitions cap-check
/// against `max_active_projects`. `Completed` and `Dropped` are the two
/// closed outcomes; neither is terminal. A closed project keeps its slug,
/// so `activate` brings its map back rather than making it again, and a
/// closed project may be closed again with the other outcome, each a new
/// decision the log records (RFC 0004 §5.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectStatus {
    Active,
    Parked,
    Completed,
    Dropped,
}

impl ProjectStatus {
    /// Kebab-case YAML / CLI form. Parallel to [`Context::as_str`] so
    /// the template renderer doesn't have to round-trip through
    /// `serde_yaml`.
    pub fn as_str(self) -> &'static str {
        match self {
            ProjectStatus::Active => "active",
            ProjectStatus::Parked => "parked",
            ProjectStatus::Completed => "completed",
            ProjectStatus::Dropped => "dropped",
        }
    }
}

/// Effort/focus tag attached to a next action, displayed as a
/// parenthesised suffix (`- [ ] Run ablation (deep)`). Drives the
/// energy selector in the daily orientation view (§5.2 of the design
/// doc) — the user picks an energy level and the UI surfaces actions
/// matching that level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EnergyLevel {
    Deep,
    Medium,
    Light,
}

impl EnergyLevel {
    /// Every variant in declaration order — used by [`FromStr`] and
    /// any future "iterate every level" need (e.g. CLI help text).
    pub const ALL: [EnergyLevel; 3] = [EnergyLevel::Deep, EnergyLevel::Medium, EnergyLevel::Light];

    /// Kebab-case form used in the action suffix and the CLI flag.
    pub fn as_str(self) -> &'static str {
        match self {
            EnergyLevel::Deep => "deep",
            EnergyLevel::Medium => "medium",
            EnergyLevel::Light => "light",
        }
    }
}

/// Error returned when a string does not match any [`EnergyLevel`] variant.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown energy level: {0} (expected: deep, medium, or light)")]
pub struct ParseEnergyLevelError(pub String);

impl FromStr for EnergyLevel {
    type Err = ParseEnergyLevelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        EnergyLevel::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| ParseEnergyLevelError(s.to_owned()))
    }
}

/// Parsed and validated frontmatter for a project map. Once this
/// struct exists, every field is guaranteed present and well-typed —
/// downstream code does not re-validate.
///
/// `core_question` keeps the raw YAML string (a wikilink such as
/// `"[[questions/research/foo]]"`) rather than a resolved [`VaultPath`].
/// Wikilink resolution is the link-resolution layer's job, not the
/// frontmatter parser's.
///
/// [`VaultPath`]: cdno_core::path::VaultPath
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFrontmatter {
    pub context: Context,
    pub status: ProjectStatus,
    pub created: NaiveDate,
    pub core_question: Option<String>,
    /// The date the project was closed (completed or dropped). `None`
    /// while it is active or parked, and on a map written before the
    /// field existed.
    pub closed: Option<NaiveDate>,
    /// The raw `held_by:` text, the name of whoever else is moving the map
    /// (RFC 0006 §8.2). `None` when absent, null, or not a string: the key
    /// is read leniently, never through `optional_field::<String>`, so a
    /// hand-written `held_by: [[people/ana]]` (a nested YAML sequence) never
    /// fails the map. What the text means needs the vault's configured
    /// `[projects] holder_sentinels`: see [`holder`](Self::holder).
    pub held_by: Option<String>,
}

impl ProjectFrontmatter {
    /// What `held_by` says, classified against the configured list.
    ///
    /// The raw value is already reduced to a string here, so a value that
    /// was not one reads as [`HolderReading::Absent`]. Lint, which must
    /// report `NotAString`, reads the index's frontmatter through
    /// [`holder_reading_from_json`](super::holder::holder_reading_from_json)
    /// instead.
    pub fn holder(&self, sentinels: &HolderSentinels) -> HolderReading {
        match &self.held_by {
            None => HolderReading::Absent,
            Some(text) => classify_holder_text(text, sentinels),
        }
    }

    /// Whether this map takes one of the `max_active_projects` slots: it is
    /// active and does not name someone else as its holder (RFC 0006 §7.2).
    /// A holder that is not a plain name, names the person or names nobody
    /// leaves the map counted.
    pub fn counts_toward_cap(&self, sentinels: &HolderSentinels) -> bool {
        if self.status != ProjectStatus::Active {
            return false;
        }
        match self.holder(sentinels) {
            HolderReading::Named(_) => false,
            HolderReading::Absent | HolderReading::NotAString | HolderReading::Sentinel(_) => true,
        }
    }
}

impl TryFrom<Frontmatter> for ProjectFrontmatter {
    type Error = ValidationError;

    fn try_from(fm: Frontmatter) -> Result<Self, Self::Error> {
        Ok(Self {
            context: fm.require_field::<Context>("context")?,
            status: fm.require_field::<ProjectStatus>("status")?,
            created: fm.require_field::<NaiveDate>("created")?,
            core_question: fm.optional_field::<String>("core_question")?,
            closed: fm.optional_field::<NaiveDate>("closed")?,
            held_by: lenient_string(&fm, "held_by"),
        })
    }
}

/// Read an optional string field that must never fail the map: anything but
/// a YAML string (absent, null, a sequence, a number, …) reads as `None`.
fn lenient_string(fm: &Frontmatter, name: &str) -> Option<String> {
    match fm.optional_field::<serde_yaml::Value>(name) {
        Ok(Some(serde_yaml::Value::String(text))) => Some(text),
        _ => None,
    }
}
