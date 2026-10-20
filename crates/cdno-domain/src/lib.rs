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
pub mod method_version;
pub mod note_type;
pub mod recurrence;
pub mod type_registry;
pub mod vault;

pub use bootstrap::{BootstrapError, OpenedVault, open_vault};
pub use cdno_core::template::TemplateSource;
pub use error::{
    FOCUS_OPEN_MESSAGE, FocusRemedy, NO_FOCUS_TO_PAUSE_MESSAGE, NO_FOCUS_TO_RESUME_MESSAGE,
    describe_vault_method_version,
};
pub use frontmatter::{Context, ProjectFrontmatter, ProjectStatus};
pub use lint::{LintIssue, LintReport, LintSeverity};
pub use method_version::{
    METHOD_VERSION, MethodSkew, describe_method_version, newer_minor_message,
};
pub use type_registry::{FieldInfo, NoteTypeDescriptor, NoteTypeInfo, NoteTypeKind, TypeRegistry};
pub use vault::slug::slugify;
pub use vault::{
    ActionListEntry, AttachedAction, BacklinkRef, ClosedProjectEntry, CommitmentEntry,
    CommitmentSource, CompletedActionEntry, CompletedActionSource, ConfigDocument, ConfigSaveError,
    ConfigValidationError, CurrentFocus, DAILY_LOGS_SECTION, DailyLogLine, DailyNoteView,
    DailySection, FILE_MIGRATIONS, FileMigration, GroupParent, HolderChange, InboxItem,
    LOG_HOLDER_PREFIX, LapsedHabit, LastPause, LinkedCommitment, MethodStatus, MigrationPlan,
    MigrationReport, MigrationStep, Miss, MonthlyContext, MonthlyNoteView, MonthlySection,
    NO_HOLDER, NormaliseReport, NoteRef, NoteToDailyOutcome, OpenAction, OpenItems, OpenItemsHash,
    OpenItemsReport, OpenMilestone, OpenPauses, OrientationContext, PauseOutcome, PeriodRef,
    PlaceholderSource, PortfolioSummary, ProjectBacklinks, ProjectClosureOutcome, ProjectGroup,
    ProjectStateChange, ProjectSummary, QuestionBacklinks, QuestionSummary, READ_THROUGH_BELOW,
    RefResolution, RelativeDay, ResumeOutcome, ResumedFrom, ResumedKind, SearchFilters,
    SearchResultEntry, SlotUsage, StewardshipSummary, StewardshipVariant, SwitchOutcome,
    TemplateContent, TemplatePlaceholder, TemplateSourceKind, TemplateSummary, TemplateSyncReport,
    TemplateSyncStatus, TopAction, TrackingEntry, TrackingEntryDraft, VAULT_MIGRATED_PREFIX, Vault,
    WeeklyContext, WeeklyNoteView, WeeklySection, WriteOutcome, validate_config_str,
};
