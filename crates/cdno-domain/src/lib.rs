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
pub use error::{FOCUS_OPEN_MESSAGE, FocusRemedy, NO_FOCUS_MESSAGE};
pub use frontmatter::{Context, ProjectFrontmatter, ProjectStatus};
pub use lint::{LintIssue, LintReport, LintSeverity};
pub use type_registry::{FieldInfo, NoteTypeDescriptor, NoteTypeInfo, NoteTypeKind, TypeRegistry};
pub use vault::slug::slugify;
pub use vault::{
    ActionListEntry, AttachedAction, BacklinkRef, ClosedProjectEntry, CommitmentEntry,
    CommitmentSource, CompletedActionEntry, CompletedActionSource, ConfigDocument, ConfigSaveError,
    ConfigValidationError, CurrentFocus, DAILY_LOGS_SECTION, DailyLogLine, DailyNoteView,
    DailySection, InboxItem, LapsedHabit, LastPause, LinkedCommitment, Miss, MonthlyNoteView,
    MonthlySection, NormaliseReport, NoteRef, NoteToDailyOutcome, OpenAction, OpenItems,
    OpenItemsHash, OpenItemsReport, OpenMilestone, OpenPauses, OrientationContext, PauseOutcome,
    PeriodRef, PlaceholderSource, PortfolioSummary, ProjectBacklinks, ProjectClosureOutcome,
    ProjectStateChange, ProjectSummary, QuestionBacklinks, QuestionSummary, RefResolution,
    RelativeDay, ResumeOutcome, ResumedFrom, ResumedKind, SearchFilters, SearchResultEntry,
    StewardshipSummary, StewardshipVariant, SwitchOutcome, TemplateContent, TemplatePlaceholder,
    TemplateSourceKind, TemplateSummary, TemplateSyncReport, TemplateSyncStatus, TopAction,
    TrackingEntry, TrackingEntryDraft, Vault, WeeklyNoteView, WeeklySection, WriteOutcome,
    validate_config_str,
};
