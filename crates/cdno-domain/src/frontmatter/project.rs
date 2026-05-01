//! Project map frontmatter: typed view of `type: project` YAML headers.
//!
//! See `docs/design.md` §5.3 (project map shape) and §5.10 (canonical
//! context list and rationale for compile-time enums).

use cdno_core::error::ValidationError;
use cdno_core::frontmatter::Frontmatter;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Life-domain classification for projects, stewardships, and
/// commitments. Canonical and closed — see §5.10 of the design doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Context {
    Work,
    SideProject,
    University,
    Family,
    Household,
    Legal,
    Personal,
}

impl Context {
    /// Kebab-case YAML / CLI form. Mirrors the `#[serde(rename_all =
    /// "kebab-case")]` projection used for serialisation, but exposed
    /// directly so write paths don't have to round-trip through
    /// `serde_yaml`.
    pub fn as_str(self) -> &'static str {
        match self {
            Context::Work => "work",
            Context::SideProject => "side-project",
            Context::University => "university",
            Context::Family => "family",
            Context::Household => "household",
            Context::Legal => "legal",
            Context::Personal => "personal",
        }
    }
}

/// Lifecycle state of a project. Park/activate transitions cap-check
/// against `max_active_projects`; completion is terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectStatus {
    Active,
    Parked,
    Completed,
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
    /// Kebab-case form used in the action suffix and the CLI flag.
    pub fn as_str(self) -> &'static str {
        match self {
            EnergyLevel::Deep => "deep",
            EnergyLevel::Medium => "medium",
            EnergyLevel::Light => "light",
        }
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
}

impl TryFrom<Frontmatter> for ProjectFrontmatter {
    type Error = ValidationError;

    fn try_from(fm: Frontmatter) -> Result<Self, Self::Error> {
        Ok(Self {
            context: fm.require_field::<Context>("context")?,
            status: fm.require_field::<ProjectStatus>("status")?,
            created: fm.require_field::<NaiveDate>("created")?,
            core_question: fm.optional_field::<String>("core_question")?,
        })
    }
}
