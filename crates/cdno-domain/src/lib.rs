//! cdno-domain: Domain logic for Cuaderno.
//!
//! Note types, business rules, queries, and state transitions.
//! Pure logic — no file I/O, no networking. Receives dependencies via constructor injection.

pub mod error;
pub mod frontmatter;
pub mod lint;
pub mod note_type;
pub mod recurrence;
pub mod type_registry;
pub mod vault;

pub use cdno_core::template::TemplateSource;
pub use frontmatter::{Context, ProjectFrontmatter, ProjectStatus};
pub use lint::{LintIssue, LintReport, LintSeverity};
pub use type_registry::{NoteTypeDescriptor, TypeRegistry};
pub use vault::slug::slugify;
pub use vault::{
    ActionListEntry, AttachedAction, CommitmentEntry, CommitmentSource, CompletedActionEntry,
    DailyLogLine, DailyNoteView, DailySection, InboxItem, LapsedHabit, NormaliseReport,
    OrientationContext, PlaceholderSource, PortfolioSummary, ProjectBacklinks, ProjectStateChange,
    ProjectSummary, QuestionSummary, SearchFilters, SearchResultEntry, StewardshipSummary,
    StewardshipVariant, TemplatePlaceholder, TopAction, TrackingEntry, Vault, WeeklyNoteView,
    WeeklySection,
};
