//! Canonical relative paths within a Cuaderno vault.
//!
//! Single source of truth for the on-disk layout defined by the
//! Research Logbook Method. All callers — `cdno init`, the indexer,
//! domain operations that compose paths — read from here so the layout
//! lives in exactly one place.
//!
//! Two flavours of API live side-by-side:
//!
//! - `pub const` strings for the static parts of the layout
//!   (`PROJECTS`, `INBOX`, `CUADERNO_DIR`, …). Use these when no date
//!   information is involved.
//! - Helper functions for the year-partitioned subtrees (journal,
//!   `commitments/_done/`). High-frequency append-only folders are
//!   year-partitioned so they don't accumulate thousands of siblings
//!   over a multi-year vault lifetime.
//!
//! Strings are POSIX-style relative paths (forward slashes), suitable
//! both as `&str` arguments to [`crate::path::VaultPath`] and as
//! arguments to [`std::path::Path::join`] (which handles the separator
//! conversion on Windows).

use chrono::{Datelike, NaiveDate};

use crate::path::VaultPath;

// Journal — daily, weekly, and monthly notes, year-partitioned. Use the
// helper functions below to build the actual paths.
pub const JOURNAL: &str = "journal";

// Projects — active under `projects/`, parked under `projects/_parked/`.
pub const PROJECTS: &str = "projects";
pub const PROJECTS_PARKED: &str = "projects/_parked";

// Knowledge layer.
pub const PORTFOLIOS: &str = "portfolios";
pub const STEWARDSHIPS: &str = "stewardships";

// Commitments — open under `commitments/`. The `_done/` parent of
// fulfilled commitments is year-partitioned via [`commitments_done_dir`].
pub const COMMITMENTS: &str = "commitments";
pub const COMMITMENTS_DONE: &str = "commitments/_done";

// Actions — heavy-form action notes live under `actions/` while active.
// Completed notes move to the year-partitioned `_done/` via
// [`actions_done_dir`] (see design §5.11).
pub const ACTIONS: &str = "actions";
pub const ACTIONS_DONE: &str = "actions/_done";

// Questions split by domain.
pub const QUESTIONS_RESEARCH: &str = "questions/research";
pub const QUESTIONS_LIFE: &str = "questions/life";

// Uncategorised captures.
pub const INBOX: &str = "inbox";

/// Top-level folders owned by the built-in note types (plus the `.cuaderno/`
/// meta dir). A config-defined custom type may not claim one of these as its
/// `folder` — it would drop notes alongside built-in notes. `questions` has no
/// top-level note itself (only `questions/research` + `questions/life`) but is
/// reserved so a custom type can't mix into that tree.
pub const RESERVED_TOP_LEVEL_FOLDERS: &[&str] = &[
    JOURNAL,
    PROJECTS,
    PORTFOLIOS,
    STEWARDSHIPS,
    COMMITMENTS,
    ACTIONS,
    "questions",
    INBOX,
    CUADERNO_DIR,
];

// `.cuaderno/` meta directory and its contents.
pub const CUADERNO_DIR: &str = ".cuaderno";
pub const CONFIG_FILE: &str = ".cuaderno/config.toml";
pub const TEMPLATES_DIR: &str = ".cuaderno/templates";
pub const INDEX_DB: &str = ".cuaderno/index.db";

/// Directory holding daily notes for the given calendar year:
/// `journal/<year>/daily`.
pub fn journal_daily_dir(year: i32) -> String {
    format!("{JOURNAL}/{year}/daily")
}

/// Directory holding weekly notes for the given ISO week year:
/// `journal/<iso_year>/weekly`.
///
/// Takes the ISO week year specifically because ISO weeks straddle
/// calendar years (e.g. ISO week 1 of 2026 starts Mon 29 Dec 2025).
/// Using the ISO year keeps the folder name and the filename's `YYYY`
/// component consistent.
pub fn journal_weekly_dir(iso_year: i32) -> String {
    format!("{JOURNAL}/{iso_year}/weekly")
}

/// Directory holding monthly notes for the given calendar year:
/// `journal/<year>/monthly`.
///
/// Unlike weekly notes, a month never straddles calendar years, so this
/// takes the plain calendar year of the month (the month's own year),
/// keeping the folder name and the filename's `YYYY` component
/// consistent.
pub fn journal_monthly_dir(year: i32) -> String {
    format!("{JOURNAL}/{year}/monthly")
}

/// Directory holding fulfilled commitments for the given year:
/// `commitments/_done/<year>`.
pub fn commitments_done_dir(year: i32) -> String {
    format!("{COMMITMENTS_DONE}/{year}")
}

/// Directory holding completed action notes for the given year:
/// `actions/_done/<year>`.
pub fn actions_done_dir(year: i32) -> String {
    format!("{ACTIONS_DONE}/{year}")
}

/// Vault-relative path of the daily note for `date`:
/// `journal/<year>/daily/YYYY-MM-DD.md`.
pub fn daily_note_relpath(date: NaiveDate) -> String {
    format!(
        "{}/{}.md",
        journal_daily_dir(date.year()),
        date.format("%Y-%m-%d")
    )
}

/// Vault-relative path of the weekly note covering `date`:
/// `journal/<iso_year>/weekly/YYYY-WNN.md`.
pub fn weekly_note_relpath(date: NaiveDate) -> String {
    let iso = date.iso_week();
    format!(
        "{}/{}-W{:02}.md",
        journal_weekly_dir(iso.year()),
        iso.year(),
        iso.week()
    )
}

/// Vault-relative path of the monthly note covering `date`:
/// `journal/<year>/monthly/YYYY-MM.md`.
///
/// Keyed by the calendar month, so any day in the month resolves to the
/// same note. The `<year>` folder is the month's calendar year (months
/// never straddle years, unlike ISO weeks).
pub fn monthly_note_relpath(date: NaiveDate) -> String {
    format!(
        "{}/{}.md",
        journal_monthly_dir(date.year()),
        date.format("%Y-%m")
    )
}

/// The evidence stub that owns `path` as an attachment artefact, if any.
///
/// Filing an artefact writes a stub note beside a folder of the same
/// stem, and everything inside that folder belongs to the stub:
///
/// ```text
/// portfolios/<portfolio>/<stem>.md   <- the evidence stub (kind: file)
/// portfolios/<portfolio>/<stem>/…    <- the artefacts it owns
/// ```
///
/// Membership is by *location*, not extension. A markdown file inside an
/// artefact folder is a document that was filed, not a note: it carries
/// no frontmatter contract, and its searchable abstract lives in the stub.
/// Treating it as a note means reconciliation fails on it forever.
///
/// The walk goes from the nearest ancestor outwards rather than testing a
/// fixed depth, so the rule survives any intervening folder — which is
/// what lets a portfolio grow grouping subfolders without this changing.
/// It also means an artefact nested inside an artefact folder (a filed
/// directory tree keeps its internal structure) resolves to the same
/// owning stub as its siblings.
///
/// Only ancestors strictly inside `portfolios/` count. The
/// stub-beside-folder shape is a portfolio convention, and applying it
/// vault-wide would be actively harmful: an expanded stewardship is
/// `stewardships/<slug>/` and a flat one is `stewardships/<slug>.md`, so
/// a vault holding both spellings of one slug would see the expanded
/// folder's notes silently vanish from the index.
///
/// `stub_exists` decides whether a candidate stub is really there —
/// callers that already hold the filesystem's path set answer from it,
/// others hit the store.
pub fn owning_artefact_stub(
    path: &VaultPath,
    stub_exists: impl Fn(&VaultPath) -> bool,
) -> Option<VaultPath> {
    let mut ancestor = path.as_path().parent();
    while let Some(dir) = ancestor {
        // The shallowest folder that can own artefacts is
        // `portfolios/<portfolio>/<stem>`; anything with fewer than three
        // components sits above the layout, so the walk is done.
        if !dir.starts_with(PORTFOLIOS) || dir.components().count() < 3 {
            return None;
        }
        // `with_file_name`, never `with_extension`: the latter *replaces*
        // a trailing dotted segment, so a folder named `run-v1.2` would
        // yield the stub `run-v1.md` and pair with the wrong note.
        let stub = dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| dir.with_file_name(format!("{name}.md")))
            .and_then(|candidate| VaultPath::new(candidate).ok());
        if let Some(stub) = stub
            && stub_exists(&stub)
        {
            return Some(stub);
        }
        ancestor = dir.parent();
    }
    None
}

/// Every directory `cdno init` creates for a fresh vault, given
/// today's date so the journal and `_done` year subfolders exist
/// from day one. Subsequent years self-create on first write via
/// `create_dir_all`.
pub fn init_dirs(today: NaiveDate) -> Vec<String> {
    let year = today.year();
    let iso_year = today.iso_week().year();
    vec![
        journal_daily_dir(year),
        journal_weekly_dir(iso_year),
        journal_monthly_dir(year),
        PROJECTS.into(),
        PROJECTS_PARKED.into(),
        PORTFOLIOS.into(),
        STEWARDSHIPS.into(),
        COMMITMENTS.into(),
        commitments_done_dir(year),
        ACTIONS.into(),
        actions_done_dir(year),
        QUESTIONS_RESEARCH.into(),
        QUESTIONS_LIFE.into(),
        INBOX.into(),
        CUADERNO_DIR.into(),
        TEMPLATES_DIR.into(),
    ]
}
