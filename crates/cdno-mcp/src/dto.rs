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
    ProjectSummary, QuestionSummary, SearchResultEntry, StewardshipSummary, StewardshipVariant,
    TopAction, TrackingEntry,
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
    /// Life-domain of the owning note, kebab-case (`work`,
    /// `side-project`, …). Mirrors the stewardship DTO's `context`.
    pub context: String,
}

impl From<CommitmentEntry> for CommitmentEntryDto {
    fn from(c: CommitmentEntry) -> Self {
        Self {
            date: c.date,
            title: c.title,
            source: c.source.into(),
            is_overdue: c.is_overdue,
            context: c.context.as_str().to_owned(),
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
    StandaloneCommitment(String),
    ActionNote(String),
}

impl From<CommitmentSource> for CommitmentSourceDto {
    fn from(s: CommitmentSource) -> Self {
        match s {
            CommitmentSource::ProjectMilestone(slug) => Self::ProjectMilestone(slug),
            CommitmentSource::Stewardship(slug) => Self::Stewardship(slug),
            CommitmentSource::StandaloneCommitment(slug) => Self::StandaloneCommitment(slug),
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
    /// Media kind when this evidence is a non-markdown attachment stub
    /// (`pdf`/`image`/`video`/…, #154); omitted for a plain prose note.
    /// Lets a retrieving agent tell media evidence apart and know to
    /// dereference the artefact linked in the stub body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
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
                    kind: ef.kind,
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

/// Max length (in `char`s) of the `new_state` snippet carried in a
/// [`ProjectStateChangeDto`].
///
/// State bodies were the dominant contributor to the oversized
/// `get_weekly_context` payload (GH #298): every change embedded the
/// *full* `## Current State` body, and a busy week stacks several
/// changes on a project whose state runs to hundreds of words —
/// multiplying multi-hundred-word bodies into tens of KB and blowing the
/// MCP client's token cap.
///
/// The weekly review only needs the *gist* of where a project landed;
/// the full body lives in the project map, one `get_project_context`
/// away. 200 chars is roughly the first sentence or two — enough to tell
/// what the state now is without shipping the whole thing.
const STATE_SNIPPET_MAX_CHARS: usize = 200;

/// Truncate `s` to at most `max` characters, appending an ellipsis
/// (`…`) when content was dropped. Counts and slices by `char` so a
/// multi-byte UTF-8 boundary is never split. The trailing marker is
/// deliberately observable so a consumer (and our tests) can tell the
/// text was truncated.
pub fn truncate_chars(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        return s;
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// Truncate a project state body to the [`STATE_SNIPPET_MAX_CHARS`] gist.
fn truncate_state_snippet(s: String) -> String {
    truncate_chars(s, STATE_SNIPPET_MAX_CHARS)
}

/// Safety-valve cap (in `char`s) on `get_project_context.body_markdown`
/// — the full project-map body (GH #388). Unlike the aggressive gist
/// caps, this is deliberately generous: a normal project map runs to a
/// few thousand chars, so this never bites in practice; it only bounds a
/// pathologically long map from dominating the payload and pressing the
/// MCP client's token cap. The truncation is observable (trailing `…`),
/// and the full body stays one `read_note` away.
pub const PROJECT_BODY_MAX_CHARS: usize = 20_000;

/// Cap on the number of backlinks carried in each group of
/// `get_project_context.backlinks` (GH #388). Backlink paths are short,
/// so the risk is low, but the set is unbounded — a heavily-referenced
/// project could stack hundreds. 100 per group is generous enough to
/// never bite normal navigation while bounding the pathological case.
pub const PROJECT_BACKLINKS_PER_GROUP_MAX: usize = 100;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectStateChangeDto {
    pub date: NaiveDate,
    pub project: String,
    /// A gist of the state the project moved *to* (see
    /// [`STATE_SNIPPET_MAX_CHARS`]). The previous state is deliberately
    /// omitted (GH #351): the two sides were ~90% identical, so shipping
    /// both discarded exactly the delta a review wants — and the old
    /// state is already auto-logged to the daily note before every
    /// overwrite (a core business rule), so a review can reconstruct it
    /// from `get_weekly_context.logs` / `read_daily_note` if needed.
    pub new_state: String,
}

impl From<ProjectStateChange> for ProjectStateChangeDto {
    fn from(c: ProjectStateChange) -> Self {
        Self {
            date: c.date,
            project: c.project,
            // Bound the gist — see STATE_SNIPPET_MAX_CHARS. `old_state`
            // is dropped (GH #351); this and the WEEKLY_LOGS_MAX cap are
            // what kill the 82k payload from GH #298.
            new_state: truncate_state_snippet(c.new_state),
        }
    }
}

/// Max number of daily-log lines carried in a [`WeeklyContextDto`].
///
/// Secondary contributor to the GH #298 blow-up: `logs` ships every
/// daily-log line of the week verbatim, and a heavy week runs to
/// ~140+ terse checkpoint lines. Once the state-change bodies are
/// bounded ([`STATE_SNIPPET_MAX_CHARS`]) this is what remains to trim
/// to get the whole payload *well* under the client's cap.
///
/// We keep the most-recent lines and drop the oldest, so the back
/// half of the week (the part a review most often reasons forward
/// from) survives intact; the full per-day logs stay one
/// `read_daily_note` away. 100 lines covers a normal-to-busy week
/// while keeping the `logs` slice to roughly 10 KB.
pub const WEEKLY_LOGS_MAX: usize = 100;

/// Cap on `get_project_context.recent_mentions` — the daily-log lines from
/// the past 30 days that mention a single project (GH #352). Single-entity,
/// so no multiplicative blow-up like #298, but a very active project over a
/// busy month can still accumulate a large slice. Bounded with the same
/// keep-most-recent [`cap_recent_logs`] the weekly logs use; 50 mentions
/// covers a heavily-referenced month while the full history stays one
/// `read_daily_note` away.
pub const PROJECT_MENTIONS_MAX: usize = 50;

/// Keep only the most-recent `max` entries of an oldest-first log
/// vec, dropping from the front. A no-op when the vec is already
/// within budget. The drop is observable (the vec shrinks) so a
/// consumer can tell the week was capped.
pub fn cap_recent_logs(mut logs: Vec<DailyLogLineDto>, max: usize) -> Vec<DailyLogLineDto> {
    if logs.len() > max {
        let drop = logs.len() - max;
        logs.drain(0..drop);
    }
    logs
}

/// Top-level output of `get_weekly_context`. The four slices are
/// what design §11 calls for: this week's daily logs, what got
/// done, project state changes during the week, and the lookahead
/// of upcoming commitments.
///
/// Two slices are bounded to keep the payload under the MCP client's
/// token cap (GH #298): each `state_changes` entry carries only a
/// [`STATE_SNIPPET_MAX_CHARS`]-char gist of its before/after bodies,
/// and `logs` is capped to the [`WEEKLY_LOGS_MAX`] most-recent lines.
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
// Monthly context — composes seven slices for the
// `get_monthly_context` MCP tool. Design §11 calls this the
// strategic scan: wins patterns, active questions, portfolio
// health, project stuck-detection, stewardship overview, a six-week
// commitments lookahead, and project slot allocation against the cap.
// ---------------------------------------------------------------------

/// Active-project slot usage against the configured cap (default 5
/// per design §3.1). Useful for the monthly-review "should I drop /
/// pick up a project" decision.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectSlotsDto {
    /// Current count of active projects (status: active under
    /// `projects/`, not `projects/_parked/`).
    pub active: usize,
    /// Configured cap (`max_active_projects` in vault config).
    pub cap: u8,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MonthlyContextDto {
    /// Start of the 30-day "past month" window — `today - 30 days`.
    /// Echoed back so clients render the window explicitly.
    pub since: NaiveDate,
    /// Completed action notes from the past 30 days, oldest-first.
    pub completed_actions: Vec<CompletedActionEntryDto>,
    /// Every question with `status: active`, sorted by (domain, slug).
    pub active_questions: Vec<QuestionSummaryDto>,
    /// Every portfolio with its evidence count and staleness.
    pub portfolios: Vec<PortfolioSummaryDto>,
    /// Active projects whose map hasn't been edited in 14 days
    /// (design §11's "unchanged > 2 weeks" stuck-detection rule).
    pub stuck_projects: Vec<ProjectSummaryDto>,
    /// Every stewardship dashboard.
    pub stewardships: Vec<StewardshipSummaryDto>,
    /// Commitments in the next six weeks (design §11 explicit
    /// figure).
    pub commitments: Vec<CommitmentEntryDto>,
    /// Active-project slots against the cap.
    pub slots: ProjectSlotsDto,
}

// ---------------------------------------------------------------------
// Project context — composes the full project map, recent daily-log
// mentions, backlinks grouped by source type, and (when set)
// the resolved core_question for the `get_project_context` MCP tool.
// ---------------------------------------------------------------------

/// Typed project frontmatter on the wire. Mirrors
/// [`cdno_domain::frontmatter::ProjectFrontmatter`] one-for-one.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectFrontmatterDto {
    pub context: String,
    pub status: String,
    pub created: NaiveDate,
    /// Raw wikilink string when set (e.g.
    /// `"[[questions/research/surrogate-cost]]"`); `None` when the
    /// project has no `core_question:` frontmatter field.
    pub core_question: Option<String>,
}

impl From<cdno_domain::frontmatter::ProjectFrontmatter> for ProjectFrontmatterDto {
    fn from(fm: cdno_domain::frontmatter::ProjectFrontmatter) -> Self {
        Self {
            context: fm.context.as_str().to_owned(),
            status: fm.status.as_str().to_owned(),
            created: fm.created,
            core_question: fm.core_question,
        }
    }
}

/// One row in the `list_projects` output: the project's slug paired
/// with its typed frontmatter. Mirrors the `slug + frontmatter` shape
/// of [`ProjectContextDto`] so a client can render either uniformly.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectListEntryDto {
    pub slug: String,
    pub frontmatter: ProjectFrontmatterDto,
}

/// The `list_projects` payload: active and parked projects plus the
/// slot budget, so a client can show "3 of 5 active" without a second
/// call.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectListDto {
    pub active: Vec<ProjectListEntryDto>,
    pub parked: Vec<ProjectListEntryDto>,
    pub slots: ProjectSlotsDto,
}

/// Wire-format mirror of a single [`cdno_domain::LintIssue`].
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LintIssueDto {
    pub path: String,
    /// `"error"` or `"warning"`.
    pub severity: String,
    pub message: String,
}

impl From<cdno_domain::LintIssue> for LintIssueDto {
    fn from(issue: cdno_domain::LintIssue) -> Self {
        Self {
            path: issue.path.to_string(),
            severity: issue.severity.as_str().to_owned(),
            message: issue.message,
        }
    }
}

/// Wire-format mirror of [`cdno_domain::LintReport`], with the
/// error/warning split precomputed so a client doesn't re-tally.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LintReportDto {
    pub clean: bool,
    pub error_count: usize,
    pub warning_count: usize,
    pub issues: Vec<LintIssueDto>,
}

impl From<cdno_domain::LintReport> for LintReportDto {
    fn from(report: cdno_domain::LintReport) -> Self {
        Self {
            clean: report.is_clean(),
            error_count: report.error_count(),
            warning_count: report.warning_count(),
            issues: report.issues.into_iter().map(LintIssueDto::from).collect(),
        }
    }
}

/// Wire-format mirror of [`cdno_domain::ProjectBacklinks`]. Carries
/// the raw paths grouped by source note type. Same body-wikilinks-only
/// scope limitation as the domain query (see its doc comment).
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ProjectBacklinksDto {
    pub portfolios: Vec<String>,
    pub questions: Vec<String>,
    pub evidence: Vec<String>,
    pub actions: Vec<String>,
    pub other: Vec<String>,
}

impl From<cdno_domain::ProjectBacklinks> for ProjectBacklinksDto {
    fn from(b: cdno_domain::ProjectBacklinks) -> Self {
        // Cap each group for token-cap safety (GH #388). Keeps the first
        // PROJECT_BACKLINKS_PER_GROUP_MAX paths. The drop is silent — a
        // trimmed group carries no in-band marker, so a consumer can't tell
        // from the payload alone that entries were dropped; acceptable
        // because backlinks are unordered navigation aids, the risk is low
        // (short paths, a generous per-group cap), and a project with more
        // than that in one group is better explored via `search_notes`.
        // This `From` is MCP-only — Tauri maps `project_backlinks` itself
        // and keeps the full list.
        let to_strings = |paths: Vec<cdno_core::path::VaultPath>| -> Vec<String> {
            paths
                .into_iter()
                .take(PROJECT_BACKLINKS_PER_GROUP_MAX)
                .map(|p| p.to_string())
                .collect()
        };
        Self {
            portfolios: to_strings(b.portfolios),
            questions: to_strings(b.questions),
            evidence: to_strings(b.evidence),
            actions: to_strings(b.actions),
            other: to_strings(b.other),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProjectContextDto {
    pub slug: String,
    pub frontmatter: ProjectFrontmatterDto,
    /// The project map body (everything after the closing `---` of the
    /// frontmatter), capped to the [`PROJECT_BODY_MAX_CHARS`] safety
    /// valve for token-cap safety (GH #388). A normal map is far shorter,
    /// so this never truncates in practice; when it does, the cut is
    /// observable (trailing `…`) and the full body is one `read_note`
    /// away.
    pub body_markdown: String,
    /// Log lines from daily notes (past 30 days) that wikilink the
    /// project either bare or qualified, capped to the
    /// [`PROJECT_MENTIONS_MAX`] most-recent lines for token-cap safety
    /// (GH #352). The drop is observable (the slice shrinks); the full
    /// history stays one `read_daily_note` away.
    pub recent_mentions: Vec<DailyLogLineDto>,
    /// Backlinks grouped by source note type, each group capped to
    /// [`PROJECT_BACKLINKS_PER_GROUP_MAX`] for token-cap safety (GH #388).
    /// Includes both body and frontmatter wikilinks (GH #395) — see the
    /// [`cdno_domain::Vault::project_backlinks`] doc comment.
    pub backlinks: ProjectBacklinksDto,
    /// The question this project answers, when `core_question:` is
    /// set on the project map AND that question exists in the vault.
    /// Resolved by parsing the wikilink target and looking it up;
    /// `None` if the field is absent, the wikilink doesn't parse, or
    /// the target question has been deleted.
    pub core_question: Option<QuestionSummaryDto>,
}

// ---------------------------------------------------------------------
// Stewardship tracking — wire-format mirrors for the
// `get_stewardship_tracking` MCP tool.
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TrackingEntryDto {
    pub path: String,
    pub stewardship: String,
    pub activity: String,
    pub date: NaiveDate,
    pub duration_min: Option<u32>,
    /// Raw wikilink string, present only on templates with a `routine:` field.
    pub routine: Option<String>,
    /// First non-blank body line after the H1, capped at 200 chars.
    pub body_excerpt: String,
}

impl From<TrackingEntry> for TrackingEntryDto {
    fn from(e: TrackingEntry) -> Self {
        Self {
            path: e.path.to_string(),
            stewardship: e.stewardship,
            activity: e.activity,
            date: e.date,
            duration_min: e.duration_min,
            routine: e.routine,
            body_excerpt: e.body_excerpt,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StewardshipTrackingDto {
    pub stewardship: String,
    /// Activity filter applied (`None` if all activities returned).
    pub activity: Option<String>,
    /// Inclusive start of the lookback window, echoed back so clients
    /// render it explicitly rather than re-parsing the `period` input.
    pub from: NaiveDate,
    /// Inclusive end of the window (today).
    pub to: NaiveDate,
    /// Tracking notes in the window, most-recent-first; ties broken
    /// by activity then path.
    pub entries: Vec<TrackingEntryDto>,
}

// ---------------------------------------------------------------------
// Daily note (GH #158)
// ---------------------------------------------------------------------

/// Output of `read_daily_note`. `exists: false` with empty `markdown`
/// is the normal answer for a day with no note yet — callers branch on
/// it rather than catching an error.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DailyNoteViewDto {
    pub path: String,
    pub exists: bool,
    pub markdown: String,
}

impl From<cdno_domain::DailyNoteView> for DailyNoteViewDto {
    fn from(v: cdno_domain::DailyNoteView) -> Self {
        Self {
            path: v.path.to_string(),
            exists: v.exists,
            markdown: v.markdown,
        }
    }
}

/// Output of `read_weekly_note` — mirrors [`DailyNoteViewDto`]. A week
/// with no note yet returns `exists: false` and empty `markdown`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WeeklyNoteViewDto {
    pub path: String,
    pub exists: bool,
    pub markdown: String,
}

impl From<cdno_domain::WeeklyNoteView> for WeeklyNoteViewDto {
    fn from(v: cdno_domain::WeeklyNoteView) -> Self {
        Self {
            path: v.path.to_string(),
            exists: v.exists,
            markdown: v.markdown,
        }
    }
}

/// Output of `read_monthly_note` — mirrors [`WeeklyNoteViewDto`]. A month
/// with no note yet returns `exists: false` and empty `markdown`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MonthlyNoteViewDto {
    pub path: String,
    pub exists: bool,
    pub markdown: String,
}

impl From<cdno_domain::MonthlyNoteView> for MonthlyNoteViewDto {
    fn from(v: cdno_domain::MonthlyNoteView) -> Self {
        Self {
            path: v.path.to_string(),
            exists: v.exists,
            markdown: v.markdown,
        }
    }
}

// ---------------------------------------------------------------------
// Write-op result
// ---------------------------------------------------------------------

/// Wire-format mirror of [`cdno_domain::InboxItem`] — one uncategorised
/// capture awaiting triage.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct InboxItemDto {
    /// Filename stem; the handle to pass to `discard_inbox_item`.
    pub slug: String,
    /// The captured text.
    pub text: String,
}

impl From<cdno_domain::InboxItem> for InboxItemDto {
    fn from(item: cdno_domain::InboxItem) -> Self {
        Self {
            slug: item.slug,
            text: item.text,
        }
    }
}

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

// ---------------------------------------------------------------------
// Search (#172)
// ---------------------------------------------------------------------

/// One ranked `search_notes` hit. `score` is the raw bm25 relevance —
/// lower is a better match (results arrive already sorted best-first).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchResultDto {
    pub path: String,
    pub note_type: String,
    pub title: Option<String>,
    /// Excerpt of the match with the query terms wrapped in `[`…`]`.
    pub snippet: String,
    pub score: f64,
}

impl From<SearchResultEntry> for SearchResultDto {
    fn from(r: SearchResultEntry) -> Self {
        Self {
            path: r.path.to_string(),
            note_type: r.note_type,
            title: r.title,
            snippet: r.snippet,
            score: r.score,
        }
    }
}
