use cdno_core::error::{IndexError, StoreError, ValidationError};

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

    #[error("missing section '{0}' in note")]
    MissingSection(&'static str),

    #[error(transparent)]
    Validation(#[from] ValidationError),

    #[error(transparent)]
    Store(#[from] StoreError),

    #[error(transparent)]
    Index(#[from] IndexError),
}
