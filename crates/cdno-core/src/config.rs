use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use crate::error::ConfigError;

/// Top-level vault configuration, loaded from `.cuaderno/config.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct VaultConfig {
    pub vault: VaultMeta,
    #[serde(default)]
    pub schemas: HashMap<String, SchemaExtension>,
    #[serde(default)]
    pub variables: Variables,
    /// Glob patterns for files to exclude from the index (and therefore
    /// from reconciliation, search, and lint). Matched against each
    /// file's vault-relative path. Empty by default: nothing is ignored
    /// unless explicitly listed, since markdown is the source of truth
    /// and silently dropping a note would be data loss to retrieval.
    /// Typical use is fencing off repo scaffolding that lives in the
    /// vault dir but isn't a note — `CLAUDE.md`, `README.md`.
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// The `[vault]` section — basic vault metadata.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct VaultMeta {
    pub name: String,
    pub max_active_projects: u8,
}

impl Default for VaultMeta {
    fn default() -> Self {
        Self {
            name: String::from("My Vault"),
            max_active_projects: 5,
        }
    }
}

/// Per-type schema extension: `[schemas.<type>]`.
///
/// Adds vault-specific required fields on top of the built-in ones.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SchemaExtension {
    #[serde(default)]
    pub extra_required: Vec<String>,
}

/// The `[variables]` and `[variables.prompt]` sections.
///
/// Static variables are available in all templates.
/// Prompted variables trigger interactive input when unresolved.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Variables {
    #[serde(flatten)]
    pub static_vars: HashMap<String, String>,
    #[serde(default)]
    pub prompt: HashMap<String, String>,
}

impl VaultConfig {
    /// Load configuration from `.cuaderno/config.toml` within the given vault root.
    ///
    /// Returns the default config if the file does not exist.
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load(vault_root: &Path) -> Result<Self, ConfigError> {
        let config_path = vault_root.join(crate::paths::CONFIG_FILE);

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let contents =
            std::fs::read_to_string(&config_path).map_err(|source| ConfigError::Read {
                path: config_path.clone(),
                source,
            })?;

        toml::from_str(&contents).map_err(|source| ConfigError::Parse {
            path: config_path,
            source,
        })
    }

    /// Returns the schema extension for a given note type, if any.
    pub fn schema_for(&self, note_type: &str) -> Option<&SchemaExtension> {
        self.schemas.get(note_type)
    }

    /// Returns all extra required fields for a given note type.
    /// Returns an empty slice if no schema extension is defined.
    pub fn extra_required_fields(&self, note_type: &str) -> &[String] {
        self.schemas
            .get(note_type)
            .map(|s| s.extra_required.as_slice())
            .unwrap_or_default()
    }

    /// Resolve a variable by name. Checks static variables only.
    /// Prompted variables are not resolved here — the caller is
    /// responsible for interactive resolution.
    pub fn resolve_variable(&self, name: &str) -> Option<&str> {
        self.variables.static_vars.get(name).map(String::as_str)
    }

    /// Returns the prompt message for a prompted variable, if defined.
    pub fn prompt_for_variable(&self, name: &str) -> Option<&str> {
        self.variables.prompt.get(name).map(String::as_str)
    }

    /// Compile the `ignore` glob list into a matcher. Returns an error
    /// if any pattern is malformed — surfaced at vault-open time rather
    /// than silently ignoring an unparseable rule.
    pub fn ignore_set(&self) -> Result<IgnoreSet, ConfigError> {
        IgnoreSet::compile(&self.ignore)
    }
}

/// A compiled set of `ignore` globs, matched against vault-relative
/// paths during reconciliation. Wraps `globset` so that dependency
/// stays an implementation detail of this crate — callers construct an
/// `IgnoreSet` and never name `GlobSet` themselves.
#[derive(Debug, Clone)]
pub struct IgnoreSet {
    set: GlobSet,
}

impl IgnoreSet {
    /// An ignore set that matches nothing — the default when a vault
    /// configures no `ignore` patterns, and what tests use to assert
    /// the unchanged-by-default behaviour.
    pub fn empty() -> Self {
        Self {
            set: GlobSet::empty(),
        }
    }

    /// Compile a list of glob patterns. Gitignore-style `**` semantics;
    /// each pattern is matched against a file's vault-relative path.
    pub fn compile(patterns: &[String]) -> Result<Self, ConfigError> {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            builder.add(Glob::new(pattern)?);
        }
        Ok(Self {
            set: builder.build()?,
        })
    }

    /// Whether `path` (vault-relative) matches any ignore glob.
    pub fn is_match(&self, path: &Path) -> bool {
        self.set.is_match(path)
    }
}
