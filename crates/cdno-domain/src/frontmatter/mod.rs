//! Per-note-type frontmatter structs.
//!
//! Each note type with structured frontmatter beyond `type:` lives in
//! its own submodule (`project`, `commitment`, `action`, and later
//! `evidence`, `stewardship`, …). Every struct exposes a
//! `TryFrom<Frontmatter>` that validates required fields, and any
//! associated enums use kebab-case serde for YAML round-trips.
//!
//! Cross-cutting types — anything used by more than one note type's
//! frontmatter, like [`Context`] or [`EnergyLevel`] — live at the
//! module they were first introduced in and are re-imported by later
//! consumers.

pub mod action;
pub mod commitment;
pub mod context;
pub mod evidence;
pub mod portfolio;
pub mod project;
pub mod question;
pub mod stewardship;
pub mod tracking;

pub use action::{ActionFrontmatter, ActionStatus, ParseActionStatusError};
pub use commitment::{CommitmentFrontmatter, CommitmentStatus, ParseCommitmentStatusError};
pub use context::{Context, ParseContextError};
pub use evidence::EvidenceFrontmatter;
pub use portfolio::PortfolioFrontmatter;
pub use project::{EnergyLevel, ParseEnergyLevelError, ProjectFrontmatter, ProjectStatus};
pub use question::{
    ParseQuestionDomainError, ParseQuestionStatusError, QuestionDomain, QuestionFrontmatter,
    QuestionStatus,
};
pub use stewardship::StewardshipFrontmatter;
pub use tracking::TrackingFrontmatter;
