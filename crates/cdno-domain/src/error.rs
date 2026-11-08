use cdno_core::error::{
    ConfigError, IndexError, ManipulationError, ParseError, PathError, StoreError, TemplateError,
    TransactionError, ValidationError,
};
use cdno_core::path::VaultPath;

/// Why a link's interior broke, by the leaf that broke it (RFC 0008 §5.5).
fn link_fault_reason(leaf: &str) -> String {
    match leaf {
        "end of run" => "it is never closed with `]]` before the text around it ends".to_string(),
        "Space" | "Parbreak" | "Escape" => "it wraps across a line".to_string(),
        other => format!("`{other}` inside it is markup, not part of the address"),
    }
}

/// What a caller does about a [`DomainError::FocusOpen`]: the one rule,
/// shared by every surface that names a remedy (the CLI's refusal text and
/// JSON, the MCP rejection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusRemedy {
    /// The same action, started today: nothing to do.
    AlreadyFocused,
    /// The same action, carried over from an earlier day: resume it.
    Resume,
    /// A different action: switch to it, on the person's word.
    Switch,
}

impl FocusRemedy {
    /// The remedy for a `FocusOpen` with these two facts.
    pub fn of(same_action: bool, carried: bool) -> Self {
        match (same_action, carried) {
            (true, false) => FocusRemedy::AlreadyFocused,
            (true, true) => FocusRemedy::Resume,
            (false, _) => FocusRemedy::Switch,
        }
    }

    /// The wire name: `already_focused`, `resume_action` or `switch_action`.
    pub fn as_str(self) -> &'static str {
        match self {
            FocusRemedy::AlreadyFocused => "already_focused",
            FocusRemedy::Resume => "resume_action",
            FocusRemedy::Switch => "switch_action",
        }
    }
}

/// Message for a `FocusOpen` rejection: the action the agent must handle
/// before starting a different one (RFC 0005 §5.1).
pub const FOCUS_OPEN_MESSAGE: &str =
    "An action is already in focus. Ask the person before switching; do not retry.";

/// Message for a `NoFocus` rejection of a pause: nothing is in focus. A
/// resumable pause may still exist, so it says nothing about pauses.
pub const NO_FOCUS_TO_PAUSE_MESSAGE: &str = "Nothing is in focus to pause.";

/// Message for a `NoFocus` rejection of a resume: no carried focus and no
/// pause (on the asked-for project, when one is named).
pub const NO_FOCUS_TO_RESUME_MESSAGE: &str =
    "Nothing to resume: no focus carried over and no pause.";

impl DomainError {
    /// For a `FocusOpen`, its [`FocusRemedy`]; `None` for any other error.
    pub fn focus_remedy(&self) -> Option<FocusRemedy> {
        match self {
            DomainError::FocusOpen {
                same_action,
                carried,
                ..
            } => Some(FocusRemedy::of(*same_action, *carried)),
            _ => None,
        }
    }
}

impl DomainError {
    /// What the caller does about a Typst creation refusal (RFC 0008 §5.7,
    /// §6.4); `None` for any other error. The MCP and CLI render it beside the
    /// message, as they do the focus remedy.
    pub fn remedy(&self) -> Option<&'static str> {
        match self {
            DomainError::BodyCarriesMetadataBlock => Some(
                "Remove the `#metadata(...) <cdno>` block from the body; the engine writes it from the fields.",
            ),
            DomainError::BodyDoesNotParse { .. } => Some(
                "Fix the Typst at that line (an unescaped `#`, an unclosed `_` or `*`, a raw block left open) and send the whole body again.",
            ),
            DomainError::LinkNeedsEscape { leaf, .. } => Some(match leaf.as_str() {
                "end of run" => {
                    "Close the link with `]]` on the same line, or backslash-escape the `[[` if it is not a link, and send the body again."
                }
                "Space" | "Parbreak" | "Escape" => {
                    "Keep the whole `[[…]]` on one line, with no line break inside it, and send the body again."
                }
                _ => {
                    "Backslash-escape the markup character inside the `[[…]]`, such as `\\#` for an anchor, and send the body again."
                }
            }),
            DomainError::FormatUnsupportedForType { .. } => {
                Some("Omit `format` to write Markdown, or choose a note type that admits Typst.")
            }
            DomainError::VarsUnsupportedForTypst => {
                Some("Drop `vars`: put what the template would have filled into the Typst body.")
            }
            DomainError::StemTakenByOtherFormat { .. } => Some(
                "If that is the note you meant, use it; otherwise ask the person for another name, do not suffix it yourself.",
            ),
            DomainError::SectionRevisionUnavailableForTypst => Some(
                "Revise the whole body instead: read the note, change the Typst, and send all of it.",
            ),
            DomainError::AssetPathOutsideFolder { .. } => Some(
                "Move the file into the folder being adopted, or change the path to one inside it.",
            ),
            DomainError::AssetPathNotLiteral { .. } => Some(
                "Write the path as a string literal, so every file the document reaches for can be found and copied.",
            ),
            _ => None,
        }
    }
}

/// Errors from domain-level business logic.
///
/// Wraps core errors via `From` conversions and adds
/// domain-specific variants for business rule violations.
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    /// A slug the person typed breaks a naming rule (RFC 0007 §5.7).
    #[error("invalid slug '{slug}': {why}")]
    InvalidSlug {
        slug: String,
        why: crate::vault::slug::SlugRejection,
    },

    /// A slug the person typed is already another note's name: `holder` is
    /// that note, `kind` how it holds the name (RFC 0007 §6.2).
    #[error("slug '{slug}' is taken by '{holder}' (as its {})", .kind.as_str())]
    SlugTaken {
        slug: String,
        holder: VaultPath,
        kind: crate::vault::slug::TakenBy,
    },

    /// The slug was given twice: as the explicit slug and as a `slug` field of
    /// a custom note. Either would name the note, and the two could disagree.
    #[error(
        "the slug was given both as the explicit slug and as the '{field}' field; pass one of them"
    )]
    SlugGivenTwice { field: String },

    /// A portfolio created from a question (`question` is the question's
    /// slug) was given a different explicit slug. The pair is correlated by
    /// slug, so the portfolio takes the question's.
    #[error(
        "a portfolio created from a question takes the question's slug '{question_slug}', not '{slug}'; drop the slug or pass '{question_slug}'"
    )]
    PortfolioSlugMismatch { question_slug: String, slug: String },

    /// A rename was asked both to relink and to take the new slug `to`
    /// (RFC 0007 §8 S8, T30). A relink keeps the note's slug, so a relink
    /// request carries no new one: refused rather than one of the two
    /// silently winning.
    #[error(
        "a relink keeps the note's slug, so it takes no new one ('{to}'); relink, or rename to '{to}', not both"
    )]
    RelinkTakesNoSlug { to: String },

    /// A note the rename verb does not rename (RFC 0007 §5.2). `reason` is
    /// `archived` (an archived action note, whose own frozen prefix would
    /// change), `folder` (a folder note, or a question whose portfolio
    /// shares its slug, until folder renames land), `type <name>` (a type
    /// outside the renamable set), or what stops the alias being recorded.
    #[error("note '{path}' cannot be renamed: {reason}")]
    NoteNotRenamable { path: String, reason: String },

    /// Another note shares the renamed note's slug (RFC 0007 §5.2): a link
    /// naming it resolves to neither, so the links meant for this note
    /// cannot be told apart and would change referent.
    #[error(
        "cannot rename '{path}': its slug '{slug}' is shared with {}",
        .others.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
    )]
    StemShared {
        slug: String,
        path: VaultPath,
        others: Vec<VaultPath>,
    },

    /// Rewriting a referrer's reference fields would change its frontmatter
    /// somewhere else too (RFC 0007 §5.4): the rename is refused rather than
    /// write a value the person did not ask to change.
    #[error("rewriting the references in '{path}' would change more than them: {drift}")]
    ReferenceDrift {
        path: VaultPath,
        drift: cdno_core::frontmatter_edit::FrontmatterDrift,
    },

    /// A referrer's reference field holds the old slug in a form the text
    /// rewrite cannot address (a block or folded scalar, a value on the next
    /// line, a tag): every reader sees the slug there, so renaming without it
    /// would leave the note attached to a name that no longer exists
    /// (RFC 0007 §5.4).
    #[error(
        "the '{key}' field in '{path}' names the note in a form the rename cannot rewrite; write it as `{key}: <slug>` on one line by hand first, then rename"
    )]
    ReferenceUnaddressable { path: VaultPath, key: String },

    /// The rename planner computed two overlapping edits for one path. An
    /// internal invariant of the planner, not a refusal the caller can act on
    /// differently; nothing is written.
    #[error("internal error: the rename planned overlapping edits in '{path}'")]
    RenameEditOverlap { path: VaultPath },

    /// The rename was asked to write the plan the caller previewed
    /// (`expected`), but the plan built under the write lock hashes to
    /// `actual`: a referrer changed, or the day rolled over, since the
    /// preview (RFC 0007 §6.2). Nothing is written; preview again.
    #[error(
        "the vault changed since the rename was previewed (plan {expected}, now {actual}); preview it again"
    )]
    StalePlan { expected: String, actual: String },

    /// A git operation (a merge, a rebase, a cherry-pick, ...) is paused
    /// mid-way in the vault's repository (RFC 0007 §5.8): renaming now would
    /// write through a half-finished merge. Never overridable; finish or
    /// abort the operation, then rename. The domain never raises it itself:
    /// an interface's freshness check does, before the lock and again in the
    /// [`rename_note`](crate::Vault::rename_note) guard under it. `reason` is
    /// git's fact (`PausedGitOp::reason`), not interface wording.
    #[error(
        "a git operation is in progress in the vault ({reason}); finish or abort it, then rename"
    )]
    GitOperationInProgress { reason: String },

    /// The vault's branch is `behind` commits behind its upstream as of the
    /// last fetch (RFC 0007 §5.8): renaming now would rewrite notes the
    /// other clone has already changed. The fact only: each interface
    /// appends its own remedy where it renders the refusal. Raised by an
    /// interface's freshness check (before the lock, or in the
    /// [`rename_note`](crate::Vault::rename_note) guard), never by the domain.
    #[error("the vault is {behind} commit{} behind its upstream", if *.behind == 1 { "" } else { "s" })]
    BehindUpstream { behind: u32 },

    /// A file write of the rename failed (`source`), and restoring the
    /// files already written failed too for `failed`: those paths may hold
    /// the renamed content (or none) while the rest of the vault does not.
    /// Check them by hand (version control has the previous content);
    /// reconciliation heals the index at the next open.
    #[error(
        "the rename failed part-way ({source}) and could not restore {}; check them by hand",
        .failed.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
    )]
    RenameIncomplete {
        failed: Vec<VaultPath>,
        source: StoreError,
    },

    #[error("project cap reached ({current}/{max}), counted: {active_projects:?}")]
    ProjectCapReached {
        current: usize,
        max: usize,
        active_projects: Vec<String>,
    },

    #[error(
        "Current State for '{slug}' is {chars} characters (limit {max}) \u{2014} summarise it; the detail belongs in the daily log (the previous state is auto-logged there on every change)"
    )]
    StateTooLong {
        slug: String,
        chars: usize,
        max: usize,
    },

    /// The project exists but not as a map the verb accepts: an active map
    /// at `projects/<slug>.md` for the verbs that start, switch or resume
    /// work and for parking, an active or parked one (folder and status
    /// agreeing) for every other project verb. `status` is its
    /// frontmatter's, so a caller can tell parked from completed from
    /// dropped (and, when it reads `active`, a map filed in the wrong
    /// folder); `closed` is its closure date when it has one.
    #[error(
        "project is not active: {slug} ({})",
        crate::error::describe_project_status(*.status, *.closed)
    )]
    ProjectNotActive {
        slug: String,
        status: crate::frontmatter::ProjectStatus,
        closed: Option<chrono::NaiveDate>,
    },

    #[error("project is not parked or closed: {0}")]
    ProjectNotParked(String),

    #[error("commitment is not active: {0}")]
    CommitmentNotActive(String),

    #[error(
        "commitment '{slug}' is already due on {due} \u{2014} a reschedule must move the date, and \
         writing an unchanged one would log a slip that never happened"
    )]
    CommitmentAlreadyDue {
        slug: String,
        due: chrono::NaiveDate,
    },

    /// A verb that acts on the focus found nothing to act on: `pause_action`
    /// with nothing open, `resume_action` with no carried focus and no
    /// resumable pause. The text names what each verb needs, so it is true
    /// whichever raised it; each interface gives the person its verb's own
    /// message ([`NO_FOCUS_TO_PAUSE_MESSAGE`], [`NO_FOCUS_TO_RESUME_MESSAGE`]).
    #[error("nothing to act on: pause needs an action in focus, resume a carried focus or a pause")]
    NoFocus,

    /// A start, or a resume of a pause, was attempted while an action is
    /// already in focus (RFC 0005 §5.1, §5.3, D5): focus is one slot, and
    /// neither verb displaces it.
    /// `focus` is the open one; `same_action` is true when the refused start
    /// targeted that very action; `carried` is true when the focus was
    /// started on an earlier day. The message differs by case because the
    /// remedy does: switching to the focused action is itself refused.
    #[error("{}", focus_open_message(.focus, *.same_action, *.carried))]
    FocusOpen {
        focus: crate::CurrentFocus,
        same_action: bool,
        carried: bool,
    },

    /// A hand-off was attempted while an action on that project is in focus
    /// (RFC 0006 §7.2, "Three guards"): you do not hand off a map while you
    /// are in the middle of its work. `action` is the focused action as
    /// logged. Pausing first, then handing off, succeeds.
    #[error(
        "an action on project '{slug}' is in focus ({action}) \u{2014} pause it first: a project \
         is not handed off while you are in the middle of its work"
    )]
    ProjectInFocus { slug: String, action: String },

    /// A hand-off named a holder that names you or nobody: a value on the
    /// configured `[projects] holder_sentinels` list (RFC 0006 §7.2, §8.2).
    /// Written, it would read as no holder and the map would still count,
    /// so the verb refuses instead. `value` is the name as given.
    #[error(
        "'{value}' names you or nobody \u{2014} if you cannot name who has the project, it is \
         yours: take it back instead of handing it off"
    )]
    HolderNotAName { value: String },

    #[error("no action matching '{query}' on project '{slug}'")]
    ActionNotFound { slug: String, query: String },

    #[error("ambiguous action match for '{query}' on project '{slug}': {candidates:?}")]
    AmbiguousAction {
        slug: String,
        query: String,
        candidates: Vec<String>,
    },

    #[error("action on project '{slug}' is already promoted to an action note: {line}")]
    ActionAlreadyPromoted { slug: String, line: String },

    #[error(
        "bullet on project '{slug}' has no energy tag (expected `(deep|medium|light)`): {line}"
    )]
    BulletMissingEnergy { slug: String, line: String },

    #[error(
        "milestone '{title}' on project '{slug}' is hard but has no target date \u{2014} a hard deadline with no date is not a thing; supply a date, or record it as a soft target to leave it undated"
    )]
    HardMilestoneRequiresDate { slug: String, title: String },

    #[error("no milestone matching '{query}' on project '{slug}'")]
    MilestoneNotFound { slug: String, query: String },

    #[error("ambiguous milestone match for '{query}' on project '{slug}': {candidates:?}")]
    AmbiguousMilestone {
        slug: String,
        query: String,
        candidates: Vec<String>,
    },

    #[error("no periodic commitment matching '{query}' on stewardship '{slug}'")]
    PeriodicNotFound { slug: String, query: String },

    #[error(
        "ambiguous periodic commitment match for '{query}' on stewardship '{slug}': {candidates:?}"
    )]
    AmbiguousPeriodic {
        slug: String,
        query: String,
        candidates: Vec<String>,
    },

    #[error(
        "periodic commitment '{title}' on stewardship '{slug}' has an unreadable recurrence \u{2014} \
         fix the line to one of: daily, weekly, monthly, every N months, yearly"
    )]
    PeriodicRecurrenceUnreadable { slug: String, title: String },

    #[error(
        "periodic commitment line on stewardship '{slug}' has no rewritable `next:` date: `{line}` \
         \u{2014} the schedule was left untouched rather than logging a move that did not happen"
    )]
    PeriodicDateUnwritable { slug: String, line: String },

    #[error("no waiting-on item matching '{query}' on project '{slug}'")]
    WaitingOnNotFound { slug: String, query: String },

    #[error("ambiguous waiting-on match for '{query}' on project '{slug}': {candidates:?}")]
    AmbiguousWaitingOn {
        slug: String,
        query: String,
        candidates: Vec<String>,
    },

    #[error(
        "project '{slug}' exists at more than one location: {} \u{2014} keep one",
        .candidates.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
    )]
    AmbiguousProject {
        slug: String,
        candidates: Vec<VaultPath>,
    },

    /// Closing a project with open actions or milestones (RFC 0004 §5.3).
    /// Data, not prose: the CLI renders the report as a list and MCP
    /// returns it with its hash, which a cascade must echo back.
    #[error(
        "project '{slug}' has {} open item(s) \u{2014} tick the ones that are done, or let them go explicitly",
        .report.open_count()
    )]
    ProjectHasOpenItems {
        slug: String,
        report: crate::vault::OpenItemsReport,
    },

    #[error("ambiguous slug '{0}' \u{2014} matches more than one note across domains")]
    AmbiguousSlug(String),

    #[error(
        "stewardship '{0}' is flat \u{2014} only expanded stewardships have a tracking/ subdir; convert it by moving to stewardships/{0}/_index.md first"
    )]
    TrackingOnFlatStewardship(String),

    #[error("required field '{field}' cannot be empty")]
    EmptyField { field: &'static str },

    #[error(
        "malformed wikilink target '{value}' \u{2014} pass the bare path (e.g. 'projects/foo'), not [[\u{2026}]]"
    )]
    MalformedWikilink { value: String },

    #[error("missing section '{0}' in note")]
    MissingSection(&'static str),

    #[error("section '## {section}' is append-only and cannot be replaced")]
    HistorySectionNotReplaceable { section: String },

    #[error("heading `{heading}` is not allowed in `## {section}`: {reason}")]
    HistoryEntryHeadingInvalid {
        section: String,
        heading: String,
        reason: String,
    },

    #[error("frontmatter has no field '{0}' to rewrite")]
    MissingFrontmatterField(String),

    #[error("frontmatter field '{field}' cannot be written as YAML: {reason}")]
    UnrepresentableFrontmatterValue { field: String, reason: String },

    #[error(
        "date {date} is outside the plausible range {earliest}..={latest} \u{2014} an entry that far \
         from today is almost always a typo, and backdating silently reshapes a trend"
    )]
    ImplausibleDate {
        date: chrono::NaiveDate,
        earliest: chrono::NaiveDate,
        latest: chrono::NaiveDate,
    },

    #[error(
        "template '{note_type}' references prompted variable(s) {names:?} with no value \u{2014} \
         provide a value for each (the CLI `--var name=value` flag, the MCP `vars` parameter, or a \
         static default under [variables] in .cuaderno/config.toml)"
    )]
    UnresolvedPrompts {
        note_type: String,
        names: Vec<String>,
    },

    #[error("unknown note type '{note_type}'")]
    UnknownNoteType { note_type: String },

    #[error(
        "custom note type '{name}' shadows a built-in type — pick a different name in [note_types]"
    )]
    ReservedTypeName { name: String },

    #[error(
        "schema field '[schemas.{note_type}.fields.{field}]' redeclares the engine-owned key \
         '{field}' — the engine writes it; remove the declaration"
    )]
    ReservedSchemaField { note_type: String, field: String },

    #[error(
        "'{note_type}' is a built-in note type — create it with its own command \
         (e.g. `cdno {note_type} create`); `note`/`create_note` is for config-defined custom types"
    )]
    BuiltinTypeNotCustom { note_type: String },

    #[error(
        "note type '{note_type}' has no declared field '{field}' — declare it under \
         [schemas.{note_type}.fields.{field}] to make it settable"
    )]
    UndeclaredSchemaField { note_type: String, field: String },

    #[error(
        "field '{field}' on note type '{note_type}' is not settable — add `settable = true` under \
         [schemas.{note_type}.fields.{field}] to allow it"
    )]
    FieldNotSettable { note_type: String, field: String },

    /// A Typst body that already carries a `<cdno>` block (RFC 0008 §5.7):
    /// the engine writes the block, so a second one is refused.
    #[error("the Typst body already carries a `<cdno>` metadata block")]
    BodyCarriesMetadataBlock,

    /// A Typst body whose parse holds an Error node; `line` is the first
    /// one's, counted in the body (RFC 0008 §5.7, P8).
    #[error("the Typst body does not parse: line {line}: {message}")]
    BodyDoesNotParse { line: usize, message: String },

    /// A `[[` in a Typst body whose interior is not one inline run (RFC 0008
    /// §5.5); `leaf` names what broke it.
    #[error("line {line}: the link is not one inline run ({})", link_fault_reason(.leaf))]
    LinkNeedsEscape { line: usize, leaf: String },

    /// `format: typst` for a note type that does not admit Typst (RFC 0008
    /// §5.9).
    #[error("note type '{note_type}' does not admit Typst notes")]
    FormatUnsupportedForType { note_type: String },

    /// Template variables were passed with `format: typst`; they belong to
    /// Markdown templates (RFC 0008 §5.7).
    #[error("`vars` are template variables and apply to Markdown notes only, not to Typst")]
    VarsUnsupportedForTypst,

    /// A `.typ` and an `.md` would share a stem in one folder (RFC 0008 §5.7,
    /// P6); `holder` is the file already there.
    #[error(
        "stem '{stem}' is taken by '{holder}', a note of the other format; the two would make `[[{stem}]]` ambiguous"
    )]
    StemTakenByOtherFormat { stem: String, holder: VaultPath },

    /// A section revision aimed at a Typst note (RFC 0008 §5.8): only the
    /// whole body can be revised.
    #[error("a Typst note is revised as a whole body, not by section")]
    SectionRevisionUnavailableForTypst,

    /// An adopted file reaches for a path that lands outside the folder being
    /// copied (RFC 0008 §5.7).
    #[error("line {line}: the path '{path}' lands outside the folder being adopted")]
    AssetPathOutsideFolder { path: String, line: usize },

    /// An adopted file's path argument is not a string literal (RFC 0008
    /// §5.7), so it cannot be resolved or rewritten.
    #[error("line {line}: the path argument of `{call}` is not a string literal")]
    AssetPathNotLiteral { call: String, line: usize },

    #[error("value for field '{field}' on note type '{note_type}' {reason}")]
    InvalidFieldValue {
        note_type: String,
        field: String,
        reason: String,
    },

    #[error("note type '{note_type}' requires field '{field}'")]
    MissingRequiredField { note_type: String, field: String },

    #[error(
        "note type '{note_type}' has no field '{field}' — declare it under [note_types.{note_type}]"
    )]
    UnknownField { note_type: String, field: String },

    #[error("note '{path}' cannot be revised: {reason}")]
    NoteNotRevisable { path: String, reason: String },

    /// The revision itself is malformed: a section heading that cannot
    /// serve as a wikilink anchor, or section content that would add a
    /// heading at the target section's level or higher. A different
    /// argument succeeds, so this is the caller's to fix.
    #[error("invalid revision: {reason}")]
    RevisionInvalid { reason: String },

    #[error(
        "note '{path}' changed since it was read (expected hash {expected}, found {actual}) \u{2014} \
         read it again and reapply the revision"
    )]
    StaleRevision {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("no built-in template for variant '{variant}' of '{note_type}'")]
    UnknownTemplateVariant { note_type: String, variant: String },

    #[error("a custom template already exists at {path} — pass force to overwrite it")]
    TemplateAlreadyExists { path: String },

    /// The vault records a newer method version than this binary implements.
    /// From the write gate, always a newer major, as re-read under the write
    /// lock (RFC 0006 §8.4): writes are refused and reads keep working. From
    /// the migration runner, any newer version, minor included: a binary
    /// never stamps a vault back to an older version.
    #[error("{}", crate::error::method_version_ahead_message(*.vault, *.binary))]
    MethodVersionAhead {
        vault: cdno_core::config::MethodVersion,
        binary: cdno_core::config::MethodVersion,
    },

    /// The vault records an older major method version than this binary
    /// implements, as re-read under the write lock. `vault` is `None` when
    /// the vault is unstamped, which is read as 1.0. Writes are refused until
    /// the vault is migrated.
    #[error(
        "this vault {}, an older major version than the RLM {binary} this cdno implements \
         \u{2014} run `cdno migrate --dry-run` to see what migrating it changes, then \
         `cdno migrate`, before writing to it",
        crate::error::describe_vault_method_version(*.vault)
    )]
    VaultNeedsMigration {
        vault: Option<cdno_core::config::MethodVersion>,
        binary: cdno_core::config::MethodVersion,
    },

    /// A file migration planned a write the runner never makes: a note
    /// under `journal/`, a note of an append-only type, or the vault's own
    /// `.cuaderno/` files (RFC 0006 §8.4). Refused before anything is staged,
    /// so nothing is written.
    #[error(
        "file migration \"{step}\" planned a write to {path}, which a migration never touches \
         ({reason}) \u{2014} nothing was written"
    )]
    MigrationTouchesProtectedPath {
        step: String,
        path: String,
        reason: String,
    },

    /// `.cuaderno/config.toml` changed between the migration runner's first
    /// read of it and the stamp: the config verbs write without the vault
    /// lock, and stamping the older text would overwrite the edit. Nothing
    /// was written.
    #[error(
        ".cuaderno/config.toml changed while the migration was being prepared \u{2014} \
         nothing was written; run `cdno migrate` again"
    )]
    ConfigChangedDuringMigration,

    /// The config with the method version stamped would not open (or could
    /// not be edited), so the migration runner refuses to write it. Nothing
    /// was written.
    #[error(
        "the stamped .cuaderno/config.toml would not open: {reason} \u{2014} nothing was \
         written; fix the config and run `cdno migrate` again"
    )]
    MigrationStampInvalid { reason: String },

    #[error(transparent)]
    Validation(#[from] ValidationError),

    #[error(transparent)]
    Store(#[from] StoreError),

    #[error(transparent)]
    Index(#[from] IndexError),

    #[error(transparent)]
    Parse(#[from] ParseError),

    #[error(transparent)]
    Manipulation(#[from] ManipulationError),

    #[error(transparent)]
    Transaction(#[from] TransactionError),

    #[error(transparent)]
    Path(#[from] PathError),

    #[error(transparent)]
    Template(#[from] TemplateError),

    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// How [`DomainError::ProjectNotActive`] names a project's state: `parked`
/// with the verbs that need an active map (a parked map takes every other
/// project write, RFC 0006 §8.1), `completed on 2026-09-29`, `dropped on …`,
/// or, for a map whose status says `active` but which is not at
/// `projects/<slug>.md`, the mismatch.
pub(crate) fn describe_project_status(
    status: crate::frontmatter::ProjectStatus,
    closed: Option<chrono::NaiveDate>,
) -> String {
    use crate::frontmatter::ProjectStatus;
    match (status, closed) {
        // Markdown on purpose: message text naming the Markdown path.
        (ProjectStatus::Active, _) => "marked active but not at projects/<slug>.md".to_owned(),
        (ProjectStatus::Parked, _) => "parked \u{2014} starting, switching or resuming work \
                                        needs an active map, and so does parking"
            .to_owned(),
        (closed_status, Some(date)) => format!("{} on {date}", closed_status.as_str()),
        (closed_status, None) => closed_status.as_str().to_owned(),
    }
}

/// The message of [`DomainError::MethodVersionAhead`]: a newer major
/// refuses writes, while a newer minor (reached only from the migration
/// runner) refuses the migration and leaves writes and reads working.
pub(crate) fn method_version_ahead_message(
    vault: cdno_core::config::MethodVersion,
    binary: cdno_core::config::MethodVersion,
) -> String {
    if vault.major > binary.major {
        format!(
            "this vault follows RLM {vault}, a newer major version than the RLM {binary} this \
             cdno implements \u{2014} upgrade cdno to write to it; reads keep working"
        )
    } else {
        format!(
            "this vault follows RLM {vault}, a newer version than the RLM {binary} this cdno \
             implements \u{2014} upgrade cdno to migrate it; writes and reads keep working"
        )
    }
}

/// How [`DomainError::VaultNeedsMigration`] names the vault's version:
/// `follows RLM 0.9`, or `is unstamped (read as 1.0)` for a vault with no
/// recorded version, so an unstamped vault is never said to follow 1.0.
/// Public so the CLI's skew notice names the version in the same words.
pub fn describe_vault_method_version(vault: Option<cdno_core::config::MethodVersion>) -> String {
    let described = crate::method_version::describe_method_version(vault);
    match vault {
        Some(_) => format!("follows {described}"),
        None => format!("is {described}"),
    }
}

fn focus_open_message(focus: &crate::CurrentFocus, same_action: bool, carried: bool) -> String {
    let named = format!("{}: {}", focus.project, focus.action);
    match (same_action, carried) {
        (true, true) => format!(
            "that action is already in focus, carried over from an earlier day \u{2014} {named}. \
             Resume it instead of starting it"
        ),
        (true, false) => format!("that action is already in focus \u{2014} {named}"),
        (false, _) => format!(
            "an action is already in focus \u{2014} {named}. Switch to the new action, or pause \
             or complete this one first"
        ),
    }
}
