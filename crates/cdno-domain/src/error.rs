use cdno_core::error::{
    ConfigError, IndexError, ManipulationError, ParseError, PathError, StoreError, TemplateError,
    TransactionError, ValidationError,
};

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

    #[error("project is not active: {0}")]
    ProjectNotActive(String),

    #[error("project is not parked: {0}")]
    ProjectNotParked(String),

    #[error("commitment is not active: {0}")]
    CommitmentNotActive(String),

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

    #[error("no milestone matching '{query}' on project '{slug}'")]
    MilestoneNotFound { slug: String, query: String },

    #[error("ambiguous milestone match for '{query}' on project '{slug}': {candidates:?}")]
    AmbiguousMilestone {
        slug: String,
        query: String,
        candidates: Vec<String>,
    },

    #[error("no waiting-on item matching '{query}' on project '{slug}'")]
    WaitingOnNotFound { slug: String, query: String },

    #[error("ambiguous waiting-on match for '{query}' on project '{slug}': {candidates:?}")]
    AmbiguousWaitingOn {
        slug: String,
        query: String,
        candidates: Vec<String>,
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

    #[error("frontmatter has no field '{0}' to rewrite")]
    MissingFrontmatterField(String),

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
