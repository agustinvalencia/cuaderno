//! Per-note-type frontmatter structs.
//!
//! Each note type with structured frontmatter beyond `type:` lives in
//! its own submodule (`project`, and later `evidence`, `stewardship`,
//! …). Every struct exposes a `TryFrom<Frontmatter>` that validates
//! required fields, and any associated enums use kebab-case serde for
//! YAML round-trips.

pub mod project;

pub use project::{Context, ProjectFrontmatter, ProjectStatus};
