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
    CompletedActionEntry, ConfigDocument, ConfigSaveError, ConfigValidationError, CurrentFocus,
    DailyLogLine, DailyNoteView, DailySection, InboxItem, LapsedHabit, Miss, MonthlyNoteView,
    MonthlySection, NormaliseReport, NoteRef, OrientationContext, PeriodRef, PlaceholderSource,
    PortfolioSummary, ProjectBacklinks, ProjectStateChange, ProjectSummary, QuestionBacklinks,
    QuestionSummary, RefResolution, RelativeDay, SearchFilters, SearchResultEntry,
    StewardshipSummary, StewardshipVariant, TemplateContent, TemplatePlaceholder,
    TemplateSourceKind, TemplateSummary, TopAction, TrackingEntry, TrackingEntryDraft, Vault,
    WeeklyNoteView, WeeklySection, WriteOutcome, validate_config_str,
};
