use cdno_core::error::{
    ConfigError, IndexError, ManipulationError, ParseError, PathError, StoreError, TemplateError,
    TransactionError, ValidationError,
};
use cdno_core::path::VaultPath;

/// Errors from domain-level business logic.
///
/// Wraps core errors via `From` conversions and adds
/// domain-specific variants for business rule violations.
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("project cap reached ({current}/{max}), active: {active_projects:?}")]
    ProjectCapReached {
        current: usize,
        max: usize,
        active_projects: Vec<String>,
    },

    #[error(
        "Current State for '{slug}' is {chars} characters (limit {max}) \u{2014} summarise it; the detail belongs in the daily log (the previous state is auto-logged there on every change)"
    )]
    StateTooLong {
        slug: String,
        chars: usize,
        max: usize,
    },

    /// The project exists but not as an active map at `projects/<slug>.md`.
    /// `status` is its frontmatter's, so a caller can tell parked from
    /// completed from dropped (and, when it reads `active`, a map filed in
    /// the wrong folder); `closed` is its closure date when it has one.
    #[error(
        "project is not active: {slug} ({})",
        crate::error::describe_project_status(*.status, *.closed)
    )]
    ProjectNotActive {
        slug: String,
        status: crate::frontmatter::ProjectStatus,
        closed: Option<chrono::NaiveDate>,
    },

    #[error("project is not parked or closed: {0}")]
    ProjectNotParked(String),

    #[error("commitment is not active: {0}")]
    CommitmentNotActive(String),

    #[error(
        "commitment '{slug}' is already due on {due} \u{2014} a reschedule must move the date, and \
         writing an unchanged one would log a slip that never happened"
    )]
    CommitmentAlreadyDue {
        slug: String,
        due: chrono::NaiveDate,
    },

    /// A verb that acts on the focus found nothing to act on: `pause_action`
    /// with nothing open, `resume_action` with no carried focus and no
    /// resumable pause. The message is neutral because both verbs share it.
    #[error("nothing is in focus or paused to act on")]
    NoFocus,

    /// A start, or a resume of a pause, was attempted while an action is
    /// already in focus (RFC 0005 §5.1, §5.3, D5): focus is one slot, and
    /// neither verb displaces it.
    /// `focus` is the open one; `same_action` is true when the refused start
    /// targeted that very action; `carried` is true when the focus was
    /// started on an earlier day. The message differs by case because the
    /// remedy does: switching to the focused action is itself refused.
    #[error("{}", focus_open_message(.focus, *.same_action, *.carried))]
    FocusOpen {
        focus: crate::CurrentFocus,
        same_action: bool,
        carried: bool,
    },

    #[error("no action matching '{query}' on project '{slug}'")]
    ActionNotFound { slug: String, query: String },

    #[error("ambiguous action match for '{query}' on project '{slug}': {candidates:?}")]
    AmbiguousAction {
        slug: String,
        query: String,
        candidates: Vec<String>,
    },

    #[error("action on project '{slug}' is already promoted to an action note: {line}")]
    ActionAlreadyPromoted { slug: String, line: String },

    #[error(
        "bullet on project '{slug}' has no energy tag (expected `(deep|medium|light)`): {line}"
    )]
    BulletMissingEnergy { slug: String, line: String },

    #[error(
        "milestone '{title}' on project '{slug}' is hard but has no target date \u{2014} a hard deadline with no date is not a thing; supply a date, or record it as a soft target to leave it undated"
    )]
    HardMilestoneRequiresDate { slug: String, title: String },

    #[error("no milestone matching '{query}' on project '{slug}'")]
    MilestoneNotFound { slug: String, query: String },

    #[error("ambiguous milestone match for '{query}' on project '{slug}': {candidates:?}")]
    AmbiguousMilestone {
        slug: String,
        query: String,
        candidates: Vec<String>,
    },

    #[error("no periodic commitment matching '{query}' on stewardship '{slug}'")]
    PeriodicNotFound { slug: String, query: String },

    #[error(
        "ambiguous periodic commitment match for '{query}' on stewardship '{slug}': {candidates:?}"
    )]
    AmbiguousPeriodic {
        slug: String,
        query: String,
        candidates: Vec<String>,
    },

    #[error(
        "periodic commitment '{title}' on stewardship '{slug}' has an unreadable recurrence \u{2014} \
         fix the line to one of: daily, weekly, monthly, every N months, yearly"
    )]
    PeriodicRecurrenceUnreadable { slug: String, title: String },

    #[error(
        "periodic commitment line on stewardship '{slug}' has no rewritable `next:` date: `{line}` \
         \u{2014} the schedule was left untouched rather than logging a move that did not happen"
    )]
    PeriodicDateUnwritable { slug: String, line: String },

    #[error("no waiting-on item matching '{query}' on project '{slug}'")]
    WaitingOnNotFound { slug: String, query: String },

    #[error("ambiguous waiting-on match for '{query}' on project '{slug}': {candidates:?}")]
    AmbiguousWaitingOn {
        slug: String,
        query: String,
        candidates: Vec<String>,
    },

    #[error(
        "project '{slug}' exists at more than one location: {} \u{2014} keep one",
        .candidates.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
    )]
    AmbiguousProject {
        slug: String,
        candidates: Vec<VaultPath>,
    },

    /// Closing a project with open actions or milestones (RFC 0004 §5.3).
    /// Data, not prose: the CLI renders the report as a list and MCP
    /// returns it with its hash, which a cascade must echo back.
    #[error(
        "project '{slug}' has {} open item(s) \u{2014} tick the ones that are done, or let them go explicitly",
        .report.open_count()
    )]
    ProjectHasOpenItems {
        slug: String,
        report: crate::vault::OpenItemsReport,
    },

    #[error("ambiguous slug '{0}' \u{2014} matches more than one note across domains")]
    AmbiguousSlug(String),

    #[error(
        "stewardship '{0}' is flat \u{2014} only expanded stewardships have a tracking/ subdir; convert it by moving to stewardships/{0}/_index.md first"
    )]
    TrackingOnFlatStewardship(String),

    #[error("required field '{field}' cannot be empty")]
    EmptyField { field: &'static str },

    #[error(
        "malformed wikilink target '{value}' \u{2014} pass the bare path (e.g. 'projects/foo'), not [[\u{2026}]]"
    )]
    MalformedWikilink { value: String },

    #[error("missing section '{0}' in note")]
    MissingSection(&'static str),

    #[error("section '## {section}' is append-only and cannot be replaced")]
    HistorySectionNotReplaceable { section: String },

    #[error("heading `{heading}` is not allowed in `## {section}`: {reason}")]
    HistoryEntryHeadingInvalid {
        section: String,
        heading: String,
        reason: String,
    },

    #[error("frontmatter has no field '{0}' to rewrite")]
    MissingFrontmatterField(String),

    #[error("frontmatter field '{field}' cannot be written as YAML: {reason}")]
    UnrepresentableFrontmatterValue { field: String, reason: String },

    #[error(
        "date {date} is outside the plausible range {earliest}..={latest} \u{2014} an entry that far \
         from today is almost always a typo, and backdating silently reshapes a trend"
    )]
    ImplausibleDate {
        date: chrono::NaiveDate,
        earliest: chrono::NaiveDate,
        latest: chrono::NaiveDate,
    },

    #[error(
        "template '{note_type}' references prompted variable(s) {names:?} with no value \u{2014} \
         provide a value for each (the CLI `--var name=value` flag, the MCP `vars` parameter, or a \
         static default under [variables] in .cuaderno/config.toml)"
    )]
    UnresolvedPrompts {
        note_type: String,
        names: Vec<String>,
    },

    #[error("unknown note type '{note_type}'")]
    UnknownNoteType { note_type: String },

    #[error(
        "custom note type '{name}' shadows a built-in type — pick a different name in [note_types]"
    )]
    ReservedTypeName { name: String },

    #[error(
        "schema field '[schemas.{note_type}.fields.{field}]' redeclares the engine-owned key \
         '{field}' — the engine writes it; remove the declaration"
    )]
    ReservedSchemaField { note_type: String, field: String },

    #[error(
        "'{note_type}' is a built-in note type — create it with its own command \
         (e.g. `cdno {note_type} create`); `note`/`create_note` is for config-defined custom types"
    )]
    BuiltinTypeNotCustom { note_type: String },

    #[error(
        "note type '{note_type}' has no declared field '{field}' — declare it under \
         [schemas.{note_type}.fields.{field}] to make it settable"
    )]
    UndeclaredSchemaField { note_type: String, field: String },

    #[error(
        "field '{field}' on note type '{note_type}' is not settable — add `settable = true` under \
         [schemas.{note_type}.fields.{field}] to allow it"
    )]
    FieldNotSettable { note_type: String, field: String },

    #[error("value for field '{field}' on note type '{note_type}' {reason}")]
    InvalidFieldValue {
        note_type: String,
        field: String,
        reason: String,
    },

    #[error("note type '{note_type}' requires field '{field}'")]
    MissingRequiredField { note_type: String, field: String },

    #[error(
        "note type '{note_type}' has no field '{field}' — declare it under [note_types.{note_type}]"
    )]
    UnknownField { note_type: String, field: String },

    #[error("note '{path}' cannot be revised: {reason}")]
    NoteNotRevisable { path: String, reason: String },

    /// The revision itself is malformed: a section heading that cannot
    /// serve as a wikilink anchor, or section content that would add a
    /// heading at the target section's level or higher. A different
    /// argument succeeds, so this is the caller's to fix.
    #[error("invalid revision: {reason}")]
    RevisionInvalid { reason: String },

    #[error(
        "note '{path}' changed since it was read (expected hash {expected}, found {actual}) \u{2014} \
         read it again and reapply the revision"
    )]
    StaleRevision {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("no built-in template for variant '{variant}' of '{note_type}'")]
    UnknownTemplateVariant { note_type: String, variant: String },

    #[error("a custom template already exists at {path} — pass force to overwrite it")]
    TemplateAlreadyExists { path: String },

    #[error(transparent)]
    Validation(#[from] ValidationError),

    #[error(transparent)]
    Store(#[from] StoreError),

    #[error(transparent)]
    Index(#[from] IndexError),

    #[error(transparent)]
    Parse(#[from] ParseError),

    #[error(transparent)]
    Manipulation(#[from] ManipulationError),

    #[error(transparent)]
    Transaction(#[from] TransactionError),

    #[error(transparent)]
    Path(#[from] PathError),

    #[error(transparent)]
    Template(#[from] TemplateError),

    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// How [`DomainError::ProjectNotActive`] names a project's state: `parked`,
/// `completed on 2026-09-29`, `dropped on …`, or, for a map whose status
/// says `active` but which is not at `projects/<slug>.md`, the mismatch.
pub(crate) fn describe_project_status(
    status: crate::frontmatter::ProjectStatus,
    closed: Option<chrono::NaiveDate>,
) -> String {
    use crate::frontmatter::ProjectStatus;
    match (status, closed) {
        (ProjectStatus::Active, _) => "marked active but not at projects/<slug>.md".to_owned(),
        (ProjectStatus::Parked, _) => "parked".to_owned(),
        (closed_status, Some(date)) => format!("{} on {date}", closed_status.as_str()),
        (closed_status, None) => closed_status.as_str().to_owned(),
    }
}

fn focus_open_message(focus: &crate::CurrentFocus, same_action: bool, carried: bool) -> String {
    let named = format!("{}: {}", focus.project, focus.action);
    match (same_action, carried) {
        (true, true) => format!(
            "that action is already in focus, carried over from an earlier day \u{2014} {named}. \
             Resume it instead of starting it"
        ),
        (true, false) => format!("that action is already in focus \u{2014} {named}"),
        (false, _) => format!(
            "an action is already in focus \u{2014} {named}. Switch to the new action, or pause \
             or complete this one first"
        ),
    }
}
