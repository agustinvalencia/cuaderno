//! The command-layer error surface: every `#[tauri::command]` returns
//! `Result<T, CmdError>`, and the frontend's `commands.ts` rethrows
//! the serialised form as a typed `CuadernoError`.

use cdno_core::error::ConfigEditError;
use cdno_domain::error::DomainError;

/// Serialisable command error, tagged for the frontend to match on.
///
/// The mapping from `DomainError` is deliberately lossy — the UI
/// needs a handful of *reactions* (show the cap modal, render a
/// disambiguation picker, toast a fixable message, toast a generic
/// failure), not the domain's full taxonomy. What must never be
/// lossy is the direction: a user-fixable error must not degrade to
/// `Internal` (which hides the message behind a generic toast) —
/// hence the exhaustive match below.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, serde::Serialize, thiserror::Error)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum CmdError {
    /// Drives the project-slot allocator's gentle modal.
    #[error("project cap reached ({current}/{max})")]
    ProjectCapReached {
        current: usize,
        max: usize,
        active: Vec<String>,
    },
    #[error("{0}")]
    NotFound(String),
    /// The UI renders the candidates as a disambiguation picker —
    /// this is UX, not failure. `candidates` is empty for
    /// `AmbiguousSlug`, which doesn't carry any.
    #[error("ambiguous match for '{query}'")]
    Ambiguous {
        query: String,
        candidates: Vec<String>,
    },
    /// User-fixable: the message is shown verbatim in a toast.
    #[error("{0}")]
    Invalid(String),
    /// Not user-fixable: full detail is logged server-side via
    /// tracing; the client sees only a generic message.
    #[error("internal error")]
    Internal(String),
}

impl From<ConfigEditError> for CmdError {
    /// Both failure modes of a surgical config edit are user-fixable and
    /// safe to show verbatim: a `Parse` error means the current draft is
    /// not valid TOML (fix it in the raw editor), and `NotATable` names a
    /// key whose shape blocks the edit. Neither leaks an internal — the
    /// message is the whole point.
    fn from(e: ConfigEditError) -> Self {
        CmdError::Invalid(e.to_string())
    }
}

impl From<DomainError> for CmdError {
    fn from(e: DomainError) -> Self {
        use cdno_core::error::StoreError;
        // Exhaustive on purpose (review condition on the M1 design):
        // a future DomainError variant must force a decision here at
        // compile time rather than silently degrading to Internal.
        match e {
            DomainError::ProjectCapReached {
                current,
                max,
                active_projects,
            } => CmdError::ProjectCapReached {
                current,
                max,
                active: active_projects,
            },

            DomainError::AmbiguousAction {
                query, candidates, ..
            }
            | DomainError::AmbiguousMilestone {
                query, candidates, ..
            }
            | DomainError::AmbiguousWaitingOn {
                query, candidates, ..
            } => CmdError::Ambiguous { query, candidates },
            DomainError::AmbiguousSlug(query) => CmdError::Ambiguous {
                query,
                candidates: Vec::new(),
            },

            DomainError::ActionNotFound { .. }
            | DomainError::MilestoneNotFound { .. }
            | DomainError::WaitingOnNotFound { .. } => CmdError::NotFound(e.to_string()),
            DomainError::Store(StoreError::NotFound(_)) => CmdError::NotFound(e.to_string()),

            DomainError::ProjectNotActive(_)
            | DomainError::ProjectNotParked(_)
            | DomainError::CommitmentNotActive(_)
            | DomainError::ActionAlreadyPromoted { .. }
            | DomainError::BulletMissingEnergy { .. }
            | DomainError::TrackingOnFlatStewardship(_)
            | DomainError::EmptyField { .. }
            | DomainError::MalformedWikilink { .. }
            | DomainError::MissingSection(_)
            | DomainError::MissingFrontmatterField(_)
            | DomainError::UnresolvedPrompts { .. }
            | DomainError::UnknownNoteType { .. }
            | DomainError::ReservedTypeName { .. }
            | DomainError::ReservedSchemaField { .. }
            | DomainError::UndeclaredSchemaField { .. }
            | DomainError::FieldNotSettable { .. }
            | DomainError::InvalidFieldValue { .. }
            | DomainError::BuiltinTypeNotCustom { .. }
            | DomainError::MissingRequiredField { .. }
            | DomainError::UnknownField { .. }
            | DomainError::UnknownTemplateVariant { .. }
            | DomainError::TemplateAlreadyExists { .. } => CmdError::Invalid(e.to_string()),

            // File-shaped problems the user can fix in an editor.
            DomainError::Validation(_) | DomainError::Parse(_) | DomainError::Manipulation(_) => {
                CmdError::Invalid(e.to_string())
            }
            // Disk failures and the symlink-escape backstop are not
            // user-fixable — a bug or an environment fault, never a
            // typo. Log, genericise.
            DomainError::Store(StoreError::Io { .. } | StoreError::OutsideVault(_)) => {
                tracing::error!(error = %e, "store failure behind a command");
                CmdError::Internal("internal error while executing the command".to_owned())
            }
            // The remaining store errors (AlreadyExists, LockTimeout,
            // PermissionDenied) read as user-actionable messages.
            DomainError::Store(_) | DomainError::Path(_) | DomainError::Config(_) => {
                CmdError::Invalid(e.to_string())
            }

            // Full detail stays in the server-side log; the client
            // gets a generic message only (PR #307 lesson: never leak
            // internals across the bridge).
            DomainError::Index(_) | DomainError::Transaction(_) | DomainError::Template(_) => {
                tracing::error!(error = %e, "domain call failed");
                CmdError::Internal("internal error while executing the command".to_owned())
            }
        }
    }
}
