use std::path::PathBuf;

/// Errors originating from the core layer.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("failed to read config at {path}")]
    ConfigRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse config at {path}")]
    ConfigParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("no template found for type {note_type}{}", variant.as_ref().map(|v| format!(" (variant: {v})")).unwrap_or_default())]
    TemplateNotFound {
        note_type: String,
        variant: Option<String>,
    },

    #[error("failed to read template at {path}")]
    TemplateRead {
        path: PathBuf,
        source: std::io::Error,
    },
}
