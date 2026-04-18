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
    #[error("missing required field '{field}' in {note_type}")]
    MissingField { field: String, note_type: String },

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
}
