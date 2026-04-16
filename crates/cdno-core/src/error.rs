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
}
