use std::path::PathBuf;

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

    /// A pattern in the config `ignore` list failed to compile. The
    /// `globset` error's own message names the offending glob.
    #[error("invalid ignore glob: {0}")]
    InvalidGlob(#[from] globset::Error),
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

    #[error("failed to open index at {path}: {source}")]
    Open {
        path: String,
        source: std::io::Error,
    },

    #[error("migration {version} failed: {reason}")]
    Migration { version: u32, reason: String },

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
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
/// non-fatal: reconciliation on next startup will heal the index.
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
    /// the affected notes. Reconciliation will heal it.
    #[error("index stale after successful commit ({} error(s))", .0.len())]
    IndexStale(Vec<IndexError>),
}
