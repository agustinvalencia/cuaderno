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

use std::collections::HashMap;

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
    /// Optional: a server-side filesystem path to a non-markdown artefact
    /// (PDF, image, video, …) to file as evidence (#154). When set, the
    /// file is copied into the portfolio and a linked evidence stub is
    /// scaffolded; `content` becomes the stub's **abstract** — write a
    /// real, descriptive one, since it's the only thing search and other
    /// agents will ever see of the artefact.
    pub attach: Option<String>,
    /// Values for the template's prompted variables (`[variables.prompt]`),
    /// as a name -> value map. Mirrors the CLI's repeatable `--var name=value`.
    /// Supply an entry for each prompted variable the note's effective template
    /// uses that has no static `[variables]` default; otherwise creation fails
    /// with an "unresolved prompts" error. Ignored when `attach` is set (the
    /// attachment stub is not templated). Omitted = none.
    pub vars: Option<HashMap<String, String>>,
}

/// Input for `create_custom_note` — a note of a config-defined custom type.
///
/// The valid `type_name` values and each type's `fields` are defined in the
/// vault's `[note_types.*]` config, so this schema can't enumerate them; a
/// client discovers them from the config (or a failed call names the problem).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateCustomNoteInput {
    /// The custom note type, declared under `[note_types.<type_name>]`. Built-in
    /// types (project, question, …) have their own dedicated create tools.
    pub type_name: String,
    /// The note's title; its slug becomes the filename.
    pub title: String,
    /// Frontmatter field values as a name -> value map. Each key must be a
    /// declared `required`/`optional` field of the type, and every `required`
    /// field must be present. Omitted = none.
    #[serde(default)]
    pub fields: HashMap<String, String>,
    /// Values for the type's template prompted variables (`[variables.prompt]`),
    /// as a name -> value map. Mirrors the CLI's repeatable `--var name=value`.
    /// Omitted = none.
    pub vars: Option<HashMap<String, String>>,
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
    /// Values for the action-note template's prompted variables
    /// (`[variables.prompt]`), as a name -> value map. Mirrors the CLI's
    /// repeatable `--var name=value`. Only used when `with_note: true`; the
    /// inline-bullet form is not templated. Supply an entry for each prompted
    /// variable the template uses that has no static `[variables]` default;
    /// otherwise creation fails with an "unresolved prompts" error.
    /// Omitted = none.
    pub vars: Option<HashMap<String, String>>,
}

/// Input for `complete_action`. `query` is matched against the bullet
/// text. (`promote_action` uses [`PromoteActionInput`], which adds `vars`;
/// completing a bullet is not templated, so it stays vars-free.)
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ActionQueryInput {
    pub project: String,
    /// Case-insensitive substring of the bullet to complete.
    pub query: String,
}

/// Input for `promote_action` — like [`ActionQueryInput`] but carries the
/// template `vars` the action-note template may prompt for.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PromoteActionInput {
    pub project: String,
    /// Case-insensitive substring of the bullet to promote.
    pub query: String,
    /// Values for the action-note template's prompted variables
    /// (`[variables.prompt]`), as a name -> value map. Mirrors the CLI's
    /// repeatable `--var name=value`. Supply an entry for each prompted
    /// variable the template uses that has no static `[variables]` default;
    /// otherwise promotion fails with an "unresolved prompts" error.
    /// Omitted = none.
    pub vars: Option<HashMap<String, String>>,
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
    /// Optional origin-link slug of a related project (bare slug, e.g.
    /// `surrogate-model`). Lets the project list its related dated
    /// commitments. Loose pointer — not validated against existing
    /// projects.
    pub project: Option<String>,
    /// Optional origin-link slug of a related stewardship (bare slug,
    /// e.g. `health`). Lets the stewardship list its related dated
    /// commitments. Loose pointer — not validated against existing
    /// stewardships.
    pub stewardship: Option<String>,
    /// Values for the template's prompted variables (`[variables.prompt]`),
    /// as a name -> value map. Mirrors the CLI's repeatable `--var name=value`.
    /// Supply an entry for each prompted variable the note's effective template
    /// uses that has no static `[variables]` default; otherwise creation fails
    /// with an "unresolved prompts" error. Omitted = none.
    pub vars: Option<HashMap<String, String>>,
}

/// Input for `complete_commitment` — commitment slug.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteCommitmentInput {
    pub commitment: String,
}

/// Input for `create_tracking_entry`. `routine` is the bare slug of
/// a routine doc; domain wraps the wikilink and it only takes effect on
/// a template with a `routine:` field (not the generic default).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateTrackingEntryInput {
    pub stewardship: String,
    pub activity: String,
    pub routine: Option<String>,
    #[serde(default)]
    pub content: String,
    /// Values for the template's prompted variables (`[variables.prompt]`),
    /// as a name -> value map. Mirrors the CLI's repeatable `--var name=value`.
    /// The template variant is derived from `activity` (e.g. `weight training`
    /// -> `tracking-weight-training`), so a variant with prompted variables
    /// needs them supplied here. Supply an entry for each prompted variable the
    /// effective template uses that has no static `[variables]` default;
    /// otherwise creation fails with an "unresolved prompts" error.
    /// Omitted = none.
    pub vars: Option<HashMap<String, String>>,
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

/// Input for `read_weekly_note`. `date` is any day in the target ISO
/// week; omitted = the current week.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadWeeklyNoteInput {
    /// ISO `YYYY-MM-DD`, any day in the week. Omitted = this week.
    pub date: Option<chrono::NaiveDate>,
}

/// Input for `upsert_weekly_section`. `section` is one of the weekly-note
/// sections; `date` is any day in the target ISO week (omitted = this
/// week); `append` defaults to replace.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpsertWeeklySectionInput {
    /// One of `Wins`, `Challenges`, `One Improvement`, `This Week's Goal`
    /// (case-insensitive).
    pub section: String,
    #[serde(default)]
    pub content: String,
    /// ISO `YYYY-MM-DD`, any day in the week. Omitted = this week.
    pub date: Option<chrono::NaiveDate>,
    /// Append to the section instead of replacing it (default: replace).
    #[serde(default)]
    pub append: bool,
}

/// Input for `read_monthly_note`. `date` is any day in the target
/// calendar month; omitted = the current month.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadMonthlyNoteInput {
    /// ISO `YYYY-MM-DD`, any day in the month. Omitted = this month.
    pub date: Option<chrono::NaiveDate>,
}

/// Input for `upsert_monthly_section`. `section` is one of the
/// monthly-note sections; `date` is any day in the target calendar month
/// (omitted = this month); `append` defaults to replace.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpsertMonthlySectionInput {
    /// One of `Wins`, `Themes`, `Next Month's Focus` (case-insensitive).
    pub section: String,
    #[serde(default)]
    pub content: String,
    /// ISO `YYYY-MM-DD`, any day in the month. Omitted = this month.
    pub date: Option<chrono::NaiveDate>,
    /// Append to the section instead of replacing it (default: replace).
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
    /// Values for the template's prompted variables (`[variables.prompt]`),
    /// as a name -> value map. Mirrors the CLI's repeatable `--var name=value`.
    /// Supply an entry for each prompted variable the note's effective template
    /// uses that has no static `[variables]` default; otherwise creation fails
    /// with an "unresolved prompts" error. Omitted = none.
    pub vars: Option<HashMap<String, String>>,
}

/// Input for `create_portfolio` (GH #162). `project` optionally links
/// the portfolio to a project slug.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreatePortfolioInput {
    /// The question or topic the portfolio gathers evidence for.
    pub question: String,
    pub project: Option<String>,
    /// Values for the template's prompted variables (`[variables.prompt]`),
    /// as a name -> value map. Mirrors the CLI's repeatable `--var name=value`.
    /// Supply an entry for each prompted variable the note's effective template
    /// uses that has no static `[variables]` default; otherwise creation fails
    /// with an "unresolved prompts" error. Omitted = none.
    pub vars: Option<HashMap<String, String>>,
}

/// Input for `get_commitments` (GH #204). `lookahead_weeks` mirrors
/// the CLI `cdno commitments --weeks N`; omitted defaults to 2.
/// Overdue commitments are always included regardless of the window.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCommitmentsInput {
    /// Forward window in weeks; omitted defaults to 2. Overdue
    /// commitments are returned regardless of this window.
    pub lookahead_weeks: Option<u32>,
}

/// Input for `add_milestone` (GH #213).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddMilestoneInput {
    /// Active project slug.
    pub project: String,
    pub title: String,
    /// ISO `YYYY-MM-DD`.
    pub target_date: chrono::NaiveDate,
    /// `true` records a *hard* deadline, which the commitments
    /// aggregation surfaces; omitted/`false` is a soft target.
    #[serde(default)]
    pub hard: bool,
}

/// Input for `complete_milestone` (GH #213). `query` mirrors the
/// substring-match field of [`ActionQueryInput`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteMilestoneInput {
    /// Active project slug.
    pub project: String,
    /// Case-insensitive substring of the open milestone's title.
    pub query: String,
}

/// Input for `add_waiting_on` (GH #213).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddWaitingOnInput {
    /// Active project slug.
    pub project: String,
    /// The blocker description (informational; no checkbox).
    pub description: String,
}

/// Input for `resolve_waiting_on` (GH #213). `query` mirrors the
/// substring-match field of [`ActionQueryInput`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResolveWaitingOnInput {
    /// Active project slug.
    pub project: String,
    /// Case-insensitive substring of the waiting-on item to remove.
    pub query: String,
}

/// Input for `discard_inbox_item` (GH #208) — clear a triaged capture.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiscardInboxItemInput {
    /// Inbox item slug (the `<YYYY-MM-DD>-<slug>` filename stem from
    /// `triage_inbox`).
    pub slug: String,
}

/// Input for `capture` (GH #204) — drop a raw line into the inbox.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CaptureInput {
    /// The thought/idea/todo to capture verbatim.
    pub text: String,
}

/// Input for `link_portfolio_to_question` (GH #200) — retrofit an
/// existing portfolio onto an existing question's `## Related
/// Portfolios` backlinks. Both are slugs, not free text.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LinkPortfolioToQuestionInput {
    /// Slug of the existing portfolio (the `portfolios/<slug>/` folder
    /// name).
    pub portfolio: String,
    /// Slug of the existing question note, resolved across both the
    /// `research` and `life` domains.
    pub question: String,
}

/// Input for `link_portfolio_to_project` — retrofit an existing
/// portfolio onto an existing project: sets the portfolio's `project:`
/// frontmatter and appends it to the project map's `## Links`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LinkPortfolioToProjectInput {
    /// Slug of the existing portfolio (the `portfolios/<slug>/` folder
    /// name).
    pub portfolio: String,
    /// Bare wikilink target to the existing project note, e.g.
    /// `projects/surrogate-model` (no `[[ ]]`).
    pub project: String,
}

/// Input for `create_question` (GH #162).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateQuestionInput {
    /// `research` or `life`.
    pub domain: String,
    pub text: String,
    /// Values for the template's prompted variables (`[variables.prompt]`),
    /// as a name -> value map. Mirrors the CLI's repeatable `--var name=value`.
    /// Supply an entry for each prompted variable the note's effective template
    /// uses that has no static `[variables]` default; otherwise creation fails
    /// with an "unresolved prompts" error. Omitted = none.
    pub vars: Option<HashMap<String, String>>,
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
    /// Values for the template's prompted variables (`[variables.prompt]`),
    /// as a name -> value map. Mirrors the CLI's repeatable `--var name=value`.
    /// Supply an entry for each prompted variable the note's effective template
    /// uses that has no static `[variables]` default; otherwise creation fails
    /// with an "unresolved prompts" error. Omitted = none.
    pub vars: Option<HashMap<String, String>>,
}

// `park_project` / `activate_project` (GH #166) reuse [`ProjectSlugInput`].

/// Input for `set_question_status` (GH #166).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetQuestionStatusInput {
    pub question: String,
    /// One of `active`, `parked`, `answered`, `retired`.
    pub status: String,
}

/// Input for `set_frontmatter` (#301).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetFrontmatterInput {
    /// The note to edit: `today`, a `YYYY-MM-DD` date (both resolve to the
    /// daily note), or a vault-relative note path (e.g. `projects/foo.md`).
    pub note: String,
    /// The declared, settable frontmatter field to set.
    pub key: String,
    /// The new value as a string; coerced to the field's declared type.
    pub value: String,
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
