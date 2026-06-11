//! Tool input structs.
//!
//! Each tool's input is a `derive(Deserialize, JsonSchema)` struct.
//! Tools with no parameters take `Parameters<EmptyInput>` — rmcp
//! generates an `object`-typed empty schema rather than `null`.
//!
//! Enum-typed inputs (energy, context, question domain) are typed as
//! `String` here rather than the domain enum because the domain types
//! do not derive `JsonSchema` (they live in `cdno-domain`, which has
//! no schemars dependency). Handlers parse them via `FromStr` and
//! reject unknown values as `INVALID_PARAMS`.
//!
//! `JsonSchema` comes from the top-level `schemars` crate, pinned to
//! the same major as rmcp's transitive version — the derive macro's
//! hygiene resolves `::schemars::...` paths, so the re-export at
//! `rmcp::schemars` alone isn't enough.

use schemars::JsonSchema;
use serde::Deserialize;

/// Empty input for tools that take no parameters.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct EmptyInput {}

/// Input for `get_orientation`. `energy` biases the suggested starting
/// point per design §11.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetOrientationInput {
    /// Optional bias toward `"deep"`, `"medium"`, or `"light"`.
    pub energy: Option<String>,
}

/// Input for tools that take a single project slug.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectSlugInput {
    pub project: String,
}

/// Input for tools that take a single portfolio slug.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioSlugInput {
    pub portfolio: String,
}

/// Input for `get_stewardship_tracking` (design §11).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetStewardshipTrackingInput {
    pub stewardship: String,
    pub activity: String,
    /// `period` is a free-form lookback window (e.g. `"30d"`, `"6m"`).
    pub period: Option<String>,
}

/// Input for `get_active_questions` (design §11). `domain` is
/// optional — when omitted, all domains are returned.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetActiveQuestionsInput {
    /// Optional filter: `"research"` or `"life"`. Omitted = all.
    pub domain: Option<String>,
}

/// Input for `append_to_log`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AppendToLogInput {
    pub text: String,
}

/// Input for `file_to_portfolio`. `origin` is the bare wikilink
/// target (e.g. `"projects/foo"`); the domain wraps it per design §5.5.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileToPortfolioInput {
    pub portfolio: String,
    pub source: String,
    pub origin: String,
    #[serde(default)]
    pub content: String,
}

/// Input for `update_project_state`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateProjectStateInput {
    pub project: String,
    pub new_state: String,
}

/// Input for `add_action`. `with_note` flips between the inline
/// bullet (default) and the heavier action-note form (design §5.11).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddActionInput {
    pub project: String,
    pub title: String,
    /// One of `"deep"`, `"medium"`, `"light"`.
    pub energy: String,
    #[serde(default)]
    pub with_note: bool,
}

/// Input for the substring-match action verbs (`promote_action`,
/// `complete_action`). `query` is matched against the bullet text.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ActionQueryInput {
    pub project: String,
    pub query: String,
}

/// Input for `create_commitment`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateCommitmentInput {
    pub title: String,
    /// ISO `YYYY-MM-DD`.
    pub due: chrono::NaiveDate,
    /// One of the [`cdno_domain::frontmatter::Context`] variants
    /// (kebab-case: `work`, `household`, `personal`, …).
    pub context: String,
    pub project: Option<String>,
    pub stewardship: Option<String>,
}

/// Input for `complete_commitment` — commitment slug.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteCommitmentInput {
    pub commitment: String,
}

/// Input for `create_tracking_entry`. `routine` is the bare slug of
/// a routine doc (gym/swim templates only); domain wraps the
/// wikilink.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateTrackingEntryInput {
    pub stewardship: String,
    pub activity: String,
    pub routine: Option<String>,
    #[serde(default)]
    pub content: String,
}

/// Input for `read_daily_note` (GH #158). `date` defaults to today
/// when omitted.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadDailyNoteInput {
    /// ISO `YYYY-MM-DD`. Omitted = today.
    pub date: Option<chrono::NaiveDate>,
}

/// Input for `upsert_daily_section` (GH #158, #170). `section` is one of
/// the writable daily sections; `content` defaults to empty; `date`
/// defaults to today; `append` defaults to replace.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpsertDailySectionInput {
    /// One of `Standup`, `Intention`, `Agenda`, `Meeting` (case-insensitive).
    pub section: String,
    #[serde(default)]
    pub content: String,
    /// ISO `YYYY-MM-DD`. Omitted = today.
    pub date: Option<chrono::NaiveDate>,
    /// Append to the section instead of replacing it (for live meeting
    /// notes that accrue). Defaults to false (replace).
    #[serde(default)]
    pub append: bool,
}

/// Input for `create_project` (GH #162). `core_question` is an optional
/// bare wikilink target (e.g. `questions/research/foo`).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateProjectInput {
    pub title: String,
    /// One of the [`cdno_domain::frontmatter::Context`] variants
    /// (kebab-case: `work`, `household`, `personal`, …).
    pub context: String,
    pub core_question: Option<String>,
}

/// Input for `create_portfolio` (GH #162). `project` optionally links
/// the portfolio to a project slug.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreatePortfolioInput {
    /// The question or topic the portfolio gathers evidence for.
    pub question: String,
    pub project: Option<String>,
}

/// Input for `create_question` (GH #162).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateQuestionInput {
    /// `research` or `life`.
    pub domain: String,
    pub text: String,
}

/// Input for `create_stewardship` (GH #162). `expanded` makes a folder
/// stewardship (with a lazy `tracking/`) instead of a flat file.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateStewardshipInput {
    pub name: String,
    /// One of the [`cdno_domain::frontmatter::Context`] variants
    /// (kebab-case).
    pub context: String,
    #[serde(default)]
    pub expanded: bool,
}

// `park_project` / `activate_project` (GH #166) reuse [`ProjectSlugInput`].

/// Input for `set_question_status` (GH #166).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetQuestionStatusInput {
    pub question: String,
    /// One of `active`, `parked`, `answered`, `retired`.
    pub status: String,
}

/// Input for `add_periodic_commitment` (GH #166).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddPeriodicCommitmentInput {
    pub stewardship: String,
    pub title: String,
    /// `daily`, `weekly`, `monthly`, `yearly`, or `every N months`.
    pub recurrence: String,
    /// ISO `YYYY-MM-DD` — the next occurrence date.
    pub next_date: chrono::NaiveDate,
}

/// Input for `search_notes` (#172). `query` is required free text; every
/// filter is optional.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchNotesInput {
    /// Free-text query. Terms are matched case-insensitively and ANDed;
    /// quotes/operators in the text are treated as literal words.
    pub query: String,
    /// Restrict to one note type (e.g. `project`, `evidence`, `daily`).
    /// Omitted = any type.
    pub note_type: Option<String>,
    /// Inclusive earliest note date, ISO `YYYY-MM-DD`. Omitted = no lower bound.
    pub from: Option<chrono::NaiveDate>,
    /// Inclusive latest note date, ISO `YYYY-MM-DD`. Omitted = no upper bound.
    pub to: Option<chrono::NaiveDate>,
    /// Restrict to notes in this portfolio (their `portfolio` frontmatter).
    pub portfolio: Option<String>,
    /// Maximum results to return. Defaults to 20.
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_search_limit() -> usize {
    20
}
