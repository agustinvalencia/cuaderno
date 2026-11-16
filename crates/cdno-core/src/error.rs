use std::path::PathBuf;

use crate::path::VaultPath;

/// Errors from vault configuration loading.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config at {path}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse config at {path}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },

    /// A pattern in the config `ignore` list failed to compile. Carries
    /// a rendered message (globset's own, which names the offending
    /// glob) rather than the foreign error type, so `globset` stays an
    /// implementation detail of this crate.
    #[error("invalid ignore glob: {0}")]
    InvalidGlob(String),

    /// A `[note_types.<name>]` declaration is structurally invalid (empty or
    /// vault-escaping folder, a folder shared with another custom type, or a
    /// template that isn't a bare filename). Carries a rendered message naming
    /// the offending type.
    #[error("invalid custom note type: {0}")]
    InvalidNoteType(String),

    /// A `[schemas.<type>.fields.<name>]` declaration is structurally invalid
    /// (an unimplemented `list = true`, `values` on a non-string field, or a
    /// `default` that doesn't type-check). Carries a rendered message naming
    /// the offending field.
    #[error("invalid schema field: {0}")]
    InvalidSchema(String),

    /// A `[tracking.<activity>]` declaration is structurally invalid (a
    /// reserved `window`, a blank key). Distinct from [`InvalidSchema`] so the
    /// message does not point the user at the unrelated
    /// `[schemas.<type>.fields.<name>]` section.
    ///
    /// [`InvalidSchema`]: Self::InvalidSchema
    #[error("invalid tracking declaration: {0}")]
    InvalidTracking(String),

    /// A `[focus]` declaration is structurally invalid (a window that exceeds
    /// the configured ceiling).
    #[error("invalid focus configuration: {0}")]
    InvalidFocus(String),

    /// A `[typst] note_types` list names an unknown or refused type. Raised by
    /// the type registry in `cdno-domain`, which owns the refusal list.
    #[error("invalid typst configuration: {0}")]
    InvalidTypst(String),

    /// `[vault] method_version` could not be read from the config file's text
    /// by [`crate::config::method_version_from_str`]: the text is not TOML, or
    /// the value is not `major.minor`. The message itself names the file, the
    /// key and the reason, because the write gate's refusal reaches an MCP
    /// client as this top-level message alone (no source chain).
    #[error(
        "could not read `[vault] method_version` from {}: {reason}",
        crate::paths::CONFIG_FILE
    )]
    MethodVersion { reason: toml::de::Error },
}

/// Errors from the surgical config editor ([`crate::config_edit`], #365
/// PR5b) — the `toml_edit`-backed writers the desktop Config form drives.
///
/// Deliberately tiny: these functions do NOT validate the config (the
/// save gate owns that — see `Vault::save_config_raw`). They can only
/// fail two ways: the incoming buffer is not parseable TOML, or an
/// existing key on the path to the edited table is the wrong shape (a
/// scalar where a table is required), which would otherwise be silently
/// clobbered.
#[derive(Debug, thiserror::Error)]
pub enum ConfigEditError {
    /// The incoming `config.toml` buffer failed to parse as TOML. Carries
    /// `toml_edit`'s own rendered message (which names the line/column and
    /// shows the offending snippet). In practice the form only ever hands
    /// in a buffer the app itself produced or the user validated, so this
    /// is a defensive guard, not an expected path.
    #[error("could not parse config as TOML: {0}")]
    Parse(String),

    /// A key on the dotted path to the edited table already exists but is
    /// not a `[table]` the form can edit in place — a scalar like
    /// `note_types = 5`, or an INLINE table (`person = { folder = "..." }`)
    /// where a `[note_types.person]` header was expected. Rewriting either
    /// would silently drop the user's value, so the edit refuses; the user
    /// can still change it from the Raw editor. Names the offending key.
    #[error("`{0}` is not an editable table; edit it from the Raw view instead")]
    NotATable(String),
}

/// Errors from VaultPath construction and validation.
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("path must be relative, got: {0}")]
    Absolute(String),

    #[error("path must not contain '..': {0}")]
    ParentTraversal(String),

    #[error("path must not be empty")]
    Empty,
}

/// Errors from file storage operations (VaultStore).
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("file not found: {0}")]
    NotFound(String),

    #[error("file already exists: {0}")]
    AlreadyExists(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("I/O error on {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },

    #[error("timed out after {0:?} waiting for the vault write lock")]
    LockTimeout(std::time::Duration),

    /// The vault write lock file every kernel sees (#1012) stayed held by
    /// another writer for the whole wait. `owner` is what its record says,
    /// in words; `age_secs` its age by the folder's clock.
    #[error(
        "timed out after {waited:?} waiting for the vault write lock: {lock} is held by {owner}, \
         and is {age_secs}s old; another cdno process is writing this vault, here or on another \
         machine. A lock left by a writer that crashed is broken by the next write once it is \
         {stale_after_secs}s old"
    )]
    LockBusy {
        lock: String,
        owner: String,
        age_secs: u64,
        stale_after_secs: u64,
        waited: std::time::Duration,
    },

    /// The store's own check inside a commit, right before a rename, an
    /// unlink or a copy into place (#1012), found the commit's vault write
    /// lock file no longer its own, and did nothing. Only a commit with a
    /// lock file can meet it, and the commit turns it into
    /// [`TransactionError::LockLost`]: it never reaches a caller as is.
    #[error("the vault write lock {lock} is no longer this write's; nothing was done")]
    LockLost { lock: String },

    /// The path escapes the vault root once symlinks are resolved —
    /// or its confinement could not be verified (fail closed).
    /// `VaultPath` already rejects `..`/absolute lexically; this
    /// variant is the filesystem-level backstop (GH #303).
    #[error("path resolves outside the vault: {0}")]
    OutsideVault(String),

    /// A conditional write, move or delete found the file holding
    /// something other than what the caller last observed there (#1001):
    /// another writer changed it, or created or removed it. The store
    /// wrote nothing to the path.
    #[error("{0} changed on disk since it was read")]
    Changed(VaultPath),
}

/// Errors from the vault index (VaultIndex).
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("index entry not found: {0}")]
    NotFound(String),

    #[error("index query failed: {0}")]
    Query(String),

    #[error("index update failed: {0}")]
    Update(String),

    /// The vault write lock (the index's `flock`) could not be taken for
    /// `what`. The lock's own error is kept, so a caller can tell a busy
    /// vault from a failure by its type.
    #[error("{what}: {source}")]
    Lock {
        what: String,
        #[source]
        source: StoreError,
    },

    #[error("failed to open index at {path}: {source}")]
    Open {
        path: String,
        source: std::io::Error,
    },

    #[error("migration {version} failed: {reason}")]
    Migration { version: u32, reason: String },

    /// An overridden index file is bound to another vault (#998). Nothing
    /// was read into it or changed.
    #[error(
        "the index file {index} (set by `CUADERNO_INDEX_PATH`, or `--index-path` on \
         `cdno-mcp-server`) belongs to the vault at {bound}, not the vault at {root}; \
         nothing was changed. Give each vault its own index file. To give this file to the \
         vault at {root}, first stop whatever serves the vault at {bound}, then run \
         `cdno reindex --rebind` in {root} (which discards the other vault's index), or \
         delete the file."
    )]
    BoundToOtherVault {
        index: String,
        bound: String,
        root: String,
    },

    /// An overridden path names a file that is not recognisably an index,
    /// so it is not discarded (#998).
    #[error(
        "refusing to use {path}: it is not a cuaderno index (not empty, and not a \
         database cuaderno created). Check `CUADERNO_INDEX_PATH` (or `--index-path` on \
         `cdno-mcp-server`); nothing was deleted."
    )]
    NotAnIndex { path: String },

    /// The index's connection was closed for a reopen and the file at its
    /// path could not be opened again yet (#1000). Nothing is wrong with
    /// the notes, and the index file itself may be sound; the server
    /// retries the reopen.
    #[error(
        "the index is being reopened: its files were replaced or deleted under this process, \
         and the file at its path could not be opened again yet"
    )]
    Detached,

    /// A reopen found the database file replaced while the old file's
    /// `-wal` or `-shm` were left at the path, and was not allowed to move
    /// them aside. Nothing was changed.
    #[error("{0}")]
    StaleSidecars(String),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// A per-machine index location override ([`crate::paths::INDEX_PATH_ENV`]
/// or `cdno-mcp-server --index-path`) that cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum IndexPathError {
    /// A relative value is refused rather than resolved, so the index
    /// never lands wherever the process happened to start.
    #[error(
        "`CUADERNO_INDEX_PATH` (or `--index-path` on `cdno-mcp-server`) must be an \
         absolute path to the index database file, got `{0}`"
    )]
    Relative(String),

    /// The value names a directory (an existing one, or it ends in a path
    /// separator); it must name the file.
    #[error(
        "`CUADERNO_INDEX_PATH` (or `--index-path` on `cdno-mcp-server`) names a \
         directory, not the index database file: `{0}`"
    )]
    Directory(String),

    /// The file name has a note extension, so the value is far likelier a
    /// mistyped path to a note than an index.
    #[error(
        "`CUADERNO_INDEX_PATH` (or `--index-path` on `cdno-mcp-server`) names a note \
         file (`.md` or `.typ`), not an index database: `{0}`"
    )]
    NoteExtension(String),
}

/// Errors from markdown/frontmatter parsing.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid frontmatter: {0}")]
    InvalidFrontmatter(String),

    #[error("missing frontmatter in {0}")]
    MissingFrontmatter(String),

    #[error("YAML parse error: {0}")]
    Yaml(String),

    #[error("{0}")]
    Typst(#[from] crate::typst::TypstError),
}

/// Errors from frontmatter field validation.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("missing required field '{field}'")]
    MissingField { field: String },

    #[error("invalid value for field '{field}': {reason}")]
    InvalidField { field: String, reason: String },
}

/// Errors from markdown section manipulation.
#[derive(Debug, thiserror::Error)]
pub enum ManipulationError {
    #[error("section not found: '{0}'")]
    SectionNotFound(String),

    #[error("ambiguous section match: '{0}'")]
    AmbiguousSection(String),
}

/// Errors from template loading and rendering.
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("no template found for type {note_type}{}", variant.as_ref().map(|v| format!(" (variant: {v})")).unwrap_or_default())]
    NotFound {
        note_type: String,
        variant: Option<String>,
    },

    #[error("failed to read template at {path}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    /// A custom-template loader (e.g. one backed by a `VaultStore`)
    /// failed. Carries a rendered message rather than a typed source so
    /// the engine stays independent of any particular I/O backend.
    #[error("failed to load custom template {name}: {message}")]
    Load { name: String, message: String },
}

/// Errors from [`VaultTransaction::commit`](crate::transaction::VaultTransaction::commit).
///
/// The two variants distinguish "vault may be in a transient
/// inconsistent state" (`FileWrite`) from "vault is correct but the
/// index is stale" (`IndexStale`). Callers can treat `IndexStale` as
/// non-fatal: the next reconciliation (the next CLI command, a server's
/// sweep or repair) will heal the index.
#[derive(Debug, thiserror::Error)]
pub enum TransactionError {
    /// A file operation failed. Previously-applied file ops were
    /// rolled back best-effort; `rollback_failures` lists any undo
    /// steps that themselves failed. On any failure here, startup
    /// reconciliation will bring the vault back to a consistent
    /// state on next open.
    #[error("file write failed: {source}")]
    FileWrite {
        source: StoreError,
        rollback_failures: Vec<StoreError>,
    },

    /// All file operations succeeded, but one or more index updates
    /// failed. The vault on disk is correct; the index is stale for
    /// the affected notes until the next reconciliation catches it up,
    /// or, when the failure means the index file is damaged
    /// ([`IndexError::is_corruption`]), a server's repair replaces it.
    ///
    /// The message carries the first underlying error, not only the
    /// count: a count alone hid a corrupt index for days, because every
    /// per-note failure read the same and none named its cause.
    #[error("index stale after successful commit ({} error(s)){}", .0.len(), first_cause(.0))]
    IndexStale(Vec<IndexError>),

    /// A file this commit was about to replace, append to or delete no longer
    /// holds what the operation read from it (#1001): another writer
    /// changed it in between, one the vault write lock did not exclude (a
    /// second kernel writing the same folder, or a program that does not
    /// take the lock). **Nothing was written**: the check runs before any
    /// file op, and a mismatch found during the file ops rolls the applied
    /// ones back first (a rollback that itself fails is a
    /// [`FileWrite`](Self::FileWrite) instead). The index is untouched.
    /// Running the whole operation again re-reads the file and is safe.
    #[error(
        "{} changed on disk while this write was being prepared, most likely by another \
         writer; nothing was written, run the operation again",
        join_paths(paths)
    )]
    Conflict { paths: Vec<VaultPath> },

    /// This writer's hold on the vault write lock file (#1012) is gone:
    /// another writer broke it (this one was suspended or stopped past the
    /// stale interval), so the commit stopped before its next file op, or
    /// right before a rename, and undid what it had applied. Each undo is
    /// conditional, so one that would overwrite the successor's change is
    /// refused and listed in `rollback_failures`. With none, nothing was
    /// written and running the operation again is safe; with some, those
    /// files may hold part of this write. The index is untouched.
    #[error("{}", lock_lost_message(lock, rollback_failures))]
    LockLost {
        lock: String,
        rollback_failures: Vec<StoreError>,
    },
}

fn lock_lost_message(lock: &str, rollback_failures: &[StoreError]) -> String {
    let head = format!(
        "the vault write lock {lock} was broken by another writer while this write held it, so \
         the write stopped"
    );
    if rollback_failures.is_empty() {
        format!("{head}; nothing it wrote was kept, run the operation again")
    } else {
        let failed = rollback_failures
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "{head}, and undoing what it had written failed for {} file(s), which may now hold \
             part of it: {failed}",
            rollback_failures.len()
        )
    }
}

fn join_paths(paths: &[VaultPath]) -> String {
    paths
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

impl TransactionError {
    /// True when the index failure underneath this error says the
    /// database file itself is damaged, rather than a row being
    /// rejected. A file failure never is: it is not the index.
    pub fn is_index_corruption(&self) -> bool {
        match self {
            Self::IndexStale(errors) => errors.iter().any(IndexError::is_corruption),
            Self::FileWrite { .. } | Self::Conflict { .. } | Self::LockLost { .. } => false,
        }
    }
}

/// `": <first error>"`, or nothing for an empty list.
fn first_cause(errors: &[IndexError]) -> String {
    errors.first().map(|e| format!(": {e}")).unwrap_or_default()
}

impl IndexError {
    /// True for the SQLite result codes meaning the database file is
    /// damaged or is not a database at all. The index is a rebuildable
    /// cache, so these are recovered by rebuilding it, never shown to
    /// the person as if their notes were at risk.
    pub fn is_corruption(&self) -> bool {
        matches!(
            self,
            Self::Sqlite(rusqlite::Error::SqliteFailure(e, _))
                if matches!(
                    e.code,
                    rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
                )
        )
    }

    /// True for [`IndexError::Detached`]: the index is between connections,
    /// not damaged.
    pub fn is_detached(&self) -> bool {
        matches!(self, Self::Detached)
    }
}
