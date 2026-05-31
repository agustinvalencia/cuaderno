//! Wire-format mirrors of domain summary types.
//!
//! Why DTOs at the MCP boundary rather than reusing domain types
//! directly:
//!
//! - The domain types live in `cdno-domain`, which deliberately has
//!   no dependency on `schemars`. Keeping the JSON Schema dialect
//!   out of the domain layer means the same types can serve the CLI,
//!   Tauri, and any future consumer without dragging the schema
//!   machinery along.
//! - DTOs let us flatten or rename fields for the MCP wire format
//!   without churning the domain API. [`CommitmentSourceDto`] for
//!   example is a flat tagged enum suitable for `serde_json`, while
//!   [`cdno_domain::CommitmentSource`] is the strongly-typed Rust
//!   enum the domain prefers.
//!
//! `JsonSchema` is taken from `rmcp::schemars` so the derive macro
//! matches the schemars version rmcp itself bundles — pinning our
//! own would risk two versions in the tree, which schemars detects
//! and rejects at compile time.
//!
//! Each DTO implements [`From`] from its domain counterpart so
//! handlers (#46, #47) convert in one explicit line.

use chrono::{NaiveDate, NaiveTime};
use schemars::JsonSchema;
use serde::Serialize;

use cdno_domain::frontmatter::{
    ActionStatus, EnergyLevel, EvidenceFrontmatter, PortfolioFrontmatter, ProjectStatus,
    QuestionDomain, QuestionStatus, StewardshipFrontmatter,
};
use cdno_domain::{
    ActionListEntry, AttachedAction, CommitmentEntry, CommitmentSource, CompletedActionEntry,
    DailyLogLine, LapsedHabit, OrientationContext, PortfolioSummary, ProjectStateChange,
    ProjectSummary, QuestionSummary, StewardshipSummary, StewardshipVariant, TopAction,
};

// ---------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectSummaryDto {
    pub slug: String,
    pub status: String,
    pub state_snippet: String,
    pub top_action: Option<TopActionDto>,
}

impl From<ProjectSummary> for ProjectSummaryDto {
    fn from(p: ProjectSummary) -> Self {
        Self {
            slug: p.slug,
            status: project_status_str(p.status).to_owned(),
            state_snippet: p.state_snippet,
            top_action: p.top_action.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TopActionDto {
    pub text: String,
    /// `None` when the bullet has no `(deep|medium|light)` tag.
    pub energy: Option<String>,
}

impl From<TopAction> for TopActionDto {
    fn from(a: TopAction) -> Self {
        Self {
            text: a.text,
            energy: a.energy.map(|e| energy_level_str(e).to_owned()),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ActionListEntryDto {
    pub text: String,
    pub energy: Option<String>,
    pub attached: Option<AttachedActionDto>,
}

impl From<ActionListEntry> for ActionListEntryDto {
    fn from(a: ActionListEntry) -> Self {
        Self {
            text: a.text,
            energy: a.energy.map(|e| energy_level_str(e).to_owned()),
            attached: a.attached.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AttachedActionDto {
    pub slug: String,
    pub status: String,
}

impl From<AttachedAction> for AttachedActionDto {
    fn from(a: AttachedAction) -> Self {
        Self {
            slug: a.slug,
            status: action_status_str(a.status).to_owned(),
        }
    }
}

// ---------------------------------------------------------------------
// Commitments aggregation (design §5.9, §5.11)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CommitmentEntryDto {
    pub date: NaiveDate,
    pub title: String,
    pub source: CommitmentSourceDto,
    pub is_overdue: bool,
}

impl From<CommitmentEntry> for CommitmentEntryDto {
    fn from(c: CommitmentEntry) -> Self {
        Self {
            date: c.date,
            title: c.title,
            source: c.source.into(),
            is_overdue: c.is_overdue,
        }
    }
}

/// Wire-format mirror of [`CommitmentSource`]. Externally tagged so
/// `serde_json` round-trips cleanly and the JSON Schema stays a
/// straightforward `oneOf`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "kind", content = "slug", rename_all = "snake_case")]
pub enum CommitmentSourceDto {
    ProjectMilestone(String),
    Stewardship(String),
    StandaloneCommitment,
    ActionNote(String),
}

impl From<CommitmentSource> for CommitmentSourceDto {
    fn from(s: CommitmentSource) -> Self {
        match s {
            CommitmentSource::ProjectMilestone(slug) => Self::ProjectMilestone(slug),
            CommitmentSource::Stewardship(slug) => Self::Stewardship(slug),
            CommitmentSource::StandaloneCommitment => Self::StandaloneCommitment,
            CommitmentSource::ActionNote(slug) => Self::ActionNote(slug),
        }
    }
}

// ---------------------------------------------------------------------
// Orientation (design §5.2 / §11)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OrientationContextDto {
    pub commitments: Vec<CommitmentEntryDto>,
    pub projects: Vec<ProjectSummaryDto>,
    pub lapsed_habits: Vec<LapsedHabitDto>,
}

impl From<OrientationContext> for OrientationContextDto {
    fn from(o: OrientationContext) -> Self {
        Self {
            commitments: o.commitments.into_iter().map(Into::into).collect(),
            projects: o.projects.into_iter().map(Into::into).collect(),
            lapsed_habits: o.lapsed_habits.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LapsedHabitDto {
    pub stewardship: String,
    /// Free-form description of the lapse.
    pub detail: String,
}

impl From<LapsedHabit> for LapsedHabitDto {
    fn from(h: LapsedHabit) -> Self {
        Self {
            stewardship: h.stewardship,
            detail: h.detail,
        }
    }
}

// ---------------------------------------------------------------------
// Portfolios + evidence (design §5.4 / §5.5)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PortfolioSummaryDto {
    pub slug: String,
    pub question: String,
    pub evidence_count: usize,
    pub last_updated: Option<NaiveDate>,
    pub staleness_days: Option<i64>,
}

impl From<PortfolioSummary> for PortfolioSummaryDto {
    fn from(p: PortfolioSummary) -> Self {
        Self {
            slug: p.slug,
            question: p.question,
            evidence_count: p.evidence_count,
            last_updated: p.last_updated,
            staleness_days: p.staleness_days,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PortfolioDetailDto {
    pub slug: String,
    pub question: String,
    pub created: NaiveDate,
    pub project: Option<String>,
    pub evidence: Vec<EvidenceEntryDto>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EvidenceEntryDto {
    pub path: String,
    pub created: NaiveDate,
    pub source: String,
    pub origin: String,
}

impl PortfolioDetailDto {
    /// Convenience constructor for handlers (#46): builds the detail
    /// view from the typed frontmatter plus the per-evidence pairs
    /// the domain query returns.
    pub fn new(
        slug: String,
        fm: PortfolioFrontmatter,
        evidence: Vec<(cdno_core::path::VaultPath, EvidenceFrontmatter)>,
    ) -> Self {
        Self {
            slug,
            question: fm.question,
            created: fm.created,
            project: fm.project,
            evidence: evidence
                .into_iter()
                .map(|(path, ef)| EvidenceEntryDto {
                    path: path.to_string(),
                    created: ef.created,
                    source: ef.source,
                    origin: ef.origin,
                })
                .collect(),
        }
    }
}

// ---------------------------------------------------------------------
// Questions (design §5.8)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct QuestionSummaryDto {
    pub slug: String,
    pub domain: String,
    pub status: String,
    pub question_text: String,
    pub updated: NaiveDate,
}

impl From<QuestionSummary> for QuestionSummaryDto {
    fn from(q: QuestionSummary) -> Self {
        Self {
            slug: q.slug,
            domain: question_domain_str(q.domain).to_owned(),
            status: question_status_str(q.status).to_owned(),
            question_text: q.question_text,
            updated: q.updated,
        }
    }
}

// ---------------------------------------------------------------------
// Stewardships (design §5.6)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StewardshipSummaryDto {
    pub slug: String,
    pub name: String,
    pub context: String,
    pub variant: String,
    pub tracking_count: usize,
    pub last_tracking_date: Option<NaiveDate>,
    pub staleness_days: Option<i64>,
}

impl From<StewardshipSummary> for StewardshipSummaryDto {
    fn from(s: StewardshipSummary) -> Self {
        Self {
            slug: s.slug,
            name: s.name,
            context: s.context.as_str().to_owned(),
            variant: stewardship_variant_str(s.variant).to_owned(),
            tracking_count: s.tracking_count,
            last_tracking_date: s.last_tracking_date,
            staleness_days: s.staleness_days,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StewardshipDetailDto {
    pub slug: String,
    pub name: String,
    pub context: String,
    pub variant: String,
    /// Markdown body of the `_index.md` (or flat `<slug>.md`) — the
    /// full dashboard so the client can render Current Status,
    /// Periodic Commitments, Active Habits, and Notes sections.
    pub body_markdown: String,
}

impl StewardshipDetailDto {
    pub fn new(
        slug: String,
        fm: StewardshipFrontmatter,
        body: String,
        variant: StewardshipVariant,
        name: String,
    ) -> Self {
        Self {
            slug,
            name,
            context: fm.context.as_str().to_owned(),
            variant: stewardship_variant_str(variant).to_owned(),
            body_markdown: body,
        }
    }
}

// ---------------------------------------------------------------------
// Weekly context — composes four query slices for the
// `get_weekly_context` MCP tool. Mirrors the design §11 surface.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DailyLogLineDto {
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub text: String,
}

impl From<DailyLogLine> for DailyLogLineDto {
    fn from(l: DailyLogLine) -> Self {
        Self {
            date: l.date,
            time: l.time,
            text: l.text,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CompletedActionEntryDto {
    pub slug: String,
    pub project: String,
    pub title: String,
    pub completed: NaiveDate,
    pub path: String,
}

impl From<CompletedActionEntry> for CompletedActionEntryDto {
    fn from(a: CompletedActionEntry) -> Self {
        Self {
            slug: a.slug,
            project: a.project,
            title: a.title,
            completed: a.completed,
            path: a.path.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectStateChangeDto {
    pub date: NaiveDate,
    pub project: String,
    pub old_state: String,
    pub new_state: String,
}

impl From<ProjectStateChange> for ProjectStateChangeDto {
    fn from(c: ProjectStateChange) -> Self {
        Self {
            date: c.date,
            project: c.project,
            old_state: c.old_state,
            new_state: c.new_state,
        }
    }
}

/// Top-level output of `get_weekly_context`. The four slices are
/// what design §11 calls for: this week's daily logs, what got
/// done, project state changes during the week, and the lookahead
/// of upcoming commitments.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WeeklyContextDto {
    /// The Monday of the ISO week the rest of the slices cover.
    /// Echoed back so clients render the week explicitly rather than
    /// relying on a local "today" interpretation.
    pub week_of: NaiveDate,
    pub logs: Vec<DailyLogLineDto>,
    pub completed_actions: Vec<CompletedActionEntryDto>,
    pub state_changes: Vec<ProjectStateChangeDto>,
    /// Commitments in the next two weeks (design §11 explicit
    /// figure). Overdue rows from the standing 30-day look-back
    /// still surface — `is_overdue` flags them.
    pub commitments: Vec<CommitmentEntryDto>,
}

// ---------------------------------------------------------------------
// Write-op result
// ---------------------------------------------------------------------

/// Uniform output shape for every operation tool — carries the
/// vault-relative path of the file the op touched (the new evidence
/// note, the updated project map, the appended-to daily, …) plus a
/// short human-readable summary line that mirrors the CLI's success
/// message. JSON-object shape (not a bare string) keeps the schema
/// extensible: future fields like `affected_ids` or `warnings` slot
/// in without breaking clients.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WriteResultDto {
    /// Vault-relative path of the file the op touched.
    pub path: String,
    /// Short summary line — what the CLI would print on success.
    pub message: String,
}

impl WriteResultDto {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

// ---------------------------------------------------------------------
// Enum-to-wire-string mappings
// ---------------------------------------------------------------------

fn project_status_str(s: ProjectStatus) -> &'static str {
    s.as_str()
}

fn energy_level_str(e: EnergyLevel) -> &'static str {
    e.as_str()
}

fn action_status_str(s: ActionStatus) -> &'static str {
    s.as_str()
}

fn question_domain_str(d: QuestionDomain) -> &'static str {
    d.as_str()
}

fn question_status_str(s: QuestionStatus) -> &'static str {
    s.as_str()
}

fn stewardship_variant_str(v: StewardshipVariant) -> &'static str {
    match v {
        StewardshipVariant::Flat => "flat",
        StewardshipVariant::Expanded => "expanded",
    }
}
