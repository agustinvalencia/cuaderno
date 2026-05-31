//! cdno-domain: Domain logic for Cuaderno.
//!
//! Note types, business rules, queries, and state transitions.
//! Pure logic — no file I/O, no networking. Receives dependencies via constructor injection.

pub mod error;
pub mod frontmatter;
pub mod lint;
pub mod note_type;
pub mod recurrence;
pub mod vault;

pub use frontmatter::{Context, ProjectFrontmatter, ProjectStatus};
pub use lint::{LintIssue, LintReport};
pub use vault::{
    ActionListEntry, AttachedAction, CommitmentEntry, CommitmentSource, CompletedActionEntry,
    DailyLogLine, LapsedHabit, OrientationContext, PortfolioSummary, ProjectBacklinks,
    ProjectStateChange, ProjectSummary, QuestionSummary, StewardshipSummary, StewardshipVariant,
    TopAction, TrackingEntry, Vault,
};
