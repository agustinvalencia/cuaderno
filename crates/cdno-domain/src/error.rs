use cdno_core::error::{
    IndexError, ManipulationError, ParseError, PathError, StoreError, TransactionError,
    ValidationError,
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

    #[error("required field '{field}' cannot be empty")]
    EmptyField { field: &'static str },

    #[error(
        "malformed wikilink target '{value}' \u{2014} pass the bare path (e.g. 'projects/foo'), not [[\u{2026}]]"
    )]
    MalformedWikilink { value: String },

    #[error("missing section '{0}' in note")]
    MissingSection(&'static str),

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
}
