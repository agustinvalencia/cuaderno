//! cdno-domain: Domain logic for Cuaderno.
//!
//! Note types, business rules, queries, and state transitions.
//! Pure logic — no file I/O, no networking — with exactly one named
//! exception: [`bootstrap`], the composition root that wires the
//! concrete store/index for long-lived consumers. Everything else
//! receives dependencies via constructor injection and stays pure.

pub mod bootstrap;
pub mod error;
pub mod frontmatter;
pub mod lint;
pub mod note_type;
pub mod recurrence;
pub mod type_registry;
pub mod vault;

pub use bootstrap::{BootstrapError, OpenedVault, open_vault};
pub use cdno_core::template::TemplateSource;
pub use frontmatter::{Context, ProjectFrontmatter, ProjectStatus};
pub use lint::{LintIssue, LintReport, LintSeverity};
pub use type_registry::{FieldInfo, NoteTypeDescriptor, NoteTypeInfo, NoteTypeKind, TypeRegistry};
pub use vault::slug::slugify;
pub use vault::{
    ActionListEntry, AttachedAction, BacklinkRef, CommitmentEntry, CommitmentSource,
    CompletedActionEntry, ConfigDocument, ConfigSaveError, ConfigValidationError, DailyLogLine,
    DailyNoteView, DailySection, InboxItem, LapsedHabit, MonthlyNoteView, MonthlySection,
    NormaliseReport, OrientationContext, PlaceholderSource, PortfolioSummary, ProjectBacklinks,
    ProjectStateChange, ProjectSummary, QuestionBacklinks, QuestionSummary, SearchFilters,
    SearchResultEntry, StewardshipSummary, StewardshipVariant, TemplateContent,
    TemplatePlaceholder, TemplateSourceKind, TemplateSummary, TopAction, TrackingEntry, Vault,
    WeeklyNoteView, WeeklySection, WriteOutcome, validate_config_str,
};
