//! Canonical relative paths within a Cuaderno vault, and the one resolver
//! of where a vault's index lives ([`resolve_index_path`]), which may be
//! outside it.
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
//! - Helper functions for the year-partitioned subtrees: the journal, and
//!   the `_done/` archives under `commitments/`, `actions/` and
//!   `projects/`. Year partitioning keeps a multi-year vault from piling
//!   thousands of siblings into one folder.
//!
//! Strings are POSIX-style relative paths (forward slashes), suitable
//! both as `&str` arguments to [`crate::path::VaultPath`] and as
//! arguments to [`std::path::Path::join`] (which handles the separator
//! conversion on Windows).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate};

use crate::error::IndexPathError;
use crate::path::VaultPath;

// Journal — daily, weekly, and monthly notes, year-partitioned. Use the
// helper functions below to build the actual paths.
pub const JOURNAL: &str = "journal";

// Projects — active under `projects/`, parked under `projects/_parked/`,
// and closed under `projects/_done/<year>/`.
pub const PROJECTS: &str = "projects";
pub const PROJECTS_PARKED: &str = "projects/_parked";
pub const PROJECTS_DONE: &str = "projects/_done";

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

/// Names of the note types the `cdno` binary ships with and can install into
/// an existing vault (`cdno config note-type install`, RFC 0003). The
/// registry itself (declaration blocks and templates) lives in `cdno-cli`;
/// only the names are here, so `cdno-mcp` can name the install command in a
/// refusal without depending on the CLI crate. A `cdno-cli` test pins the two
/// lists equal.
pub const BUNDLED_NOTE_TYPE_NAMES: &[&str] = &["concept"];

// `.cuaderno/` meta directory and its contents.
pub const CUADERNO_DIR: &str = ".cuaderno";
pub const CONFIG_FILE: &str = ".cuaderno/config.toml";
pub const TEMPLATES_DIR: &str = ".cuaderno/templates";
pub const INDEX_DB: &str = ".cuaderno/index.db";

/// Environment variable naming a per-machine location for the index
/// database file, in place of `<vault>/`[`INDEX_DB`] (#998).
///
/// Deliberately not a `config.toml` key: the config travels with the
/// vault, and this must differ per machine. A vault folder reachable from
/// two kernels at once (a host and a container bind-mounting it, a
/// network share) cannot share one SQLite file, because neither its file
/// locks nor its `-shm` map cross that boundary.
pub const INDEX_PATH_ENV: &str = "CUADERNO_INDEX_PATH";

/// Where the index database of one vault lives, as decided by
/// [`resolve_index_path`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexLocation {
    /// The database file. Its `-wal` and `-shm` sidecars, and any sibling
    /// file a rebuild writes, sit next to it: SQLite and
    /// [`crate::index::SqliteIndex`] derive them from this path.
    pub path: PathBuf,
    /// True when the path came from an override rather than being
    /// `<root>/`[`INDEX_DB`]. An overridden index is bound to one vault and
    /// is only discarded when it is recognisably an index
    /// ([`crate::index::SqliteIndex::open_location`]).
    pub overridden: bool,
}

/// The index location for the vault at `root`, given the raw override
/// value, if any: `cdno-mcp-server --index-path`, else [`INDEX_PATH_ENV`]
/// as read by [`index_override_from_env`].
///
/// This is the one place every binary decides where the index lives.
///
/// - No value, or an empty / whitespace-only one, means
///   `root.join(`[`INDEX_DB`]`)`: today's location, unchanged.
/// - An absolute value is the database file itself.
/// - A relative value is refused, never resolved: neither the vault root
///   nor the working directory is an obviously right base, and guessing
///   would put the index somewhere the operator did not choose.
/// - A value that names a directory (an existing one, or any value ending
///   in a path separator) is refused; it must name the file.
/// - A value whose file name has a note extension (`.md`, `.typ`) is
///   refused: it is far likelier a mistyped path to a note than an index,
///   and an index must never be mistaken for a note by the vault walk.
///
/// Pure apart from the directory check: callers read the environment and
/// pass the value in, so the policy is testable without touching it.
pub fn resolve_index_path(
    root: &Path,
    override_value: Option<&OsStr>,
) -> Result<IndexLocation, IndexPathError> {
    let default = || IndexLocation {
        path: root.join(INDEX_DB),
        overridden: false,
    };
    let Some(value) = override_value else {
        return Ok(default());
    };
    let text = value.to_string_lossy();
    if text.trim().is_empty() {
        return Ok(default());
    }
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(IndexPathError::Relative(text.into_owned()));
    }
    if text.ends_with(std::path::is_separator) || path.is_dir() {
        return Err(IndexPathError::Directory(text.into_owned()));
    }
    let has_note_extension = path.extension().and_then(OsStr::to_str).is_some_and(|ext| {
        NoteFormat::ALL
            .iter()
            .any(|f| ext.eq_ignore_ascii_case(f.extension()))
    });
    if has_note_extension {
        return Err(IndexPathError::NoteExtension(text.into_owned()));
    }
    Ok(IndexLocation {
        path: path.to_path_buf(),
        overridden: true,
    })
}

/// The raw value of [`INDEX_PATH_ENV`], for the binaries to pass to
/// [`resolve_index_path`]. The one place the variable is read; the
/// domain's bootstrap takes the value as an argument.
pub fn index_override_from_env() -> Option<std::ffi::OsString> {
    std::env::var_os(INDEX_PATH_ENV)
}

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

/// Directory holding closed projects for the given year:
/// `projects/_done/<year>`.
pub fn projects_done_dir(year: i32) -> String {
    format!("{PROJECTS_DONE}/{year}")
}

/// Vault-relative path of the daily note for `date`:
/// `journal/<year>/daily/YYYY-MM-DD.md`.
pub fn daily_note_relpath(date: NaiveDate) -> String {
    format!(
        // Markdown on purpose: the journal is machine-written Markdown.
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
        // Markdown on purpose: the journal is machine-written Markdown.
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
        // Markdown on purpose: the journal is machine-written Markdown.
        "{}/{}.md",
        journal_monthly_dir(date.year()),
        date.format("%Y-%m")
    )
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
        projects_done_dir(year),
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

/// The on-disk format of a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteFormat {
    Markdown,
    Typst,
}

impl NoteFormat {
    /// Every format, in note-resolution order.
    pub const ALL: [NoteFormat; 2] = [NoteFormat::Markdown, NoteFormat::Typst];

    /// The name a tool reports: `"markdown"` or `"typst"`.
    pub const fn name(self) -> &'static str {
        match self {
            NoteFormat::Markdown => "markdown",
            NoteFormat::Typst => "typst",
        }
    }

    /// The file extension, without the dot.
    pub const fn extension(self) -> &'static str {
        match self {
            NoteFormat::Markdown => "md",
            NoteFormat::Typst => "typ",
        }
    }
}

/// The format a path would have as a note, from its extension alone,
/// ASCII case-insensitively. Says nothing about whether it is a note.
pub fn note_format(path: &VaultPath) -> Option<NoteFormat> {
    let ext = path.as_path().extension()?.to_str()?;
    NoteFormat::ALL
        .into_iter()
        .find(|f| ext.eq_ignore_ascii_case(f.extension()))
}

/// The path without its note extension: `concepts/x.md` becomes
/// `concepts/x`, `portfolios/p/_index.md` becomes `portfolios/p/_index`.
/// A path with no note extension is returned unchanged.
pub fn note_address(path: &VaultPath) -> String {
    let text = path.to_string();
    match note_format(path) {
        Some(format) => text[..text.len() - format.extension().len() - 1].to_string(),
        None => text,
    }
}

/// The paths an address may name as a file of its own, one per format, in
/// resolution order: `<a>.md`, `<a>.typ`. The folder index is not among
/// them; a site that never tried `_index.md` must not start to, so it asks
/// for this rather than [`note_candidates`]. May return fewer entries than
/// formats (none for an address `VaultPath` refuses, such as `../x`); no
/// caller indexes `[0]`.
pub fn note_format_candidates(address: &str) -> Vec<VaultPath> {
    NoteFormat::ALL
        .iter()
        .filter_map(|f| VaultPath::new(format!("{address}.{}", f.extension())).ok())
        .collect()
}

/// The paths an address may name, in resolution order: `<a>.md`,
/// `<a>.typ`, `<a>/_index.md`. Pure; the caller filters against the index
/// (a `.typ` that is not indexed is not a note). May return fewer entries
/// than that (none for an address `VaultPath` refuses, such as `../x`); no
/// caller indexes `[0]`. The folder index is always `_index.md`, whatever
/// format the folder's other notes have.
pub fn note_candidates(address: &str) -> Vec<VaultPath> {
    let mut all = note_format_candidates(address);
    // Markdown on purpose: a folder note is `_index.md` whatever its siblings are.
    all.extend(VaultPath::new(format!("{address}/_index.md")).ok());
    all
}
