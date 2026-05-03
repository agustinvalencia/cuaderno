//! Per-note-type frontmatter structs.
//!
//! Each note type with structured frontmatter beyond `type:` lives in
//! its own submodule (`project`, `commitment`, and later `evidence`,
//! `stewardship`, …). Every struct exposes a `TryFrom<Frontmatter>`
//! that validates required fields, and any associated enums use
//! kebab-case serde for YAML round-trips.
//!
//! Cross-cutting types — anything used by more than one note type's
//! frontmatter, like [`Context`] — live at this level rather than
//! under any single note type.

pub mod commitment;
pub mod context;
pub mod project;

pub use commitment::{CommitmentFrontmatter, CommitmentStatus, ParseCommitmentStatusError};
pub use context::{Context, ParseContextError};
pub use project::{EnergyLevel, ParseEnergyLevelError, ProjectFrontmatter, ProjectStatus};
