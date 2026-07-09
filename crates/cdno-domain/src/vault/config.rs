//! Config inspection (#365, PR1): read the raw `.cuaderno/config.toml`
//! and dry-run the exact validation `Vault::new` runs, without touching
//! anything on disk.
//!
//! Both surfaces are read-only. [`Vault::read_config_raw`] returns the
//! file's verbatim text plus a content hash (the same xxhash the index
//! and reconciliation use) — the hash is surfaced now so a later
//! compare-and-swap write path (PR3) and this read agree on identity.
//! [`validate_config_str`] is the single source of truth for "would this
//! config open?": it composes the same `toml::from_str::<VaultConfig>`
//! → `ignore_set()` → `TypeRegistry::validate` sequence [`Vault::new`]
//! performs, so the dry-run gate here and the future save gate cannot
//! drift.

use cdno_core::config::VaultConfig;
use cdno_core::error::StoreError;
use cdno_core::hash::content_hash;
use cdno_core::path::VaultPath;
use cdno_core::paths::CONFIG_FILE;

use super::Vault;
use crate::error::DomainError;
use crate::type_registry::TypeRegistry;

/// The raw `.cuaderno/config.toml` text plus a content hash, returned by
/// [`Vault::read_config_raw`]. Serialised verbatim to the desktop Config
/// inspector.
///
/// `hash` is `cdno_core::hash::content_hash` of `content` — the same
/// fingerprint reconciliation uses. It is load-bearing for a future
/// compare-and-swap write (PR3): the read hands the UI a hash it can
/// echo back on save so a concurrent hand-edit is detected rather than
/// clobbered.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ConfigDocument {
    /// The verbatim file text — comments, ordering, and `[variables]`
    /// all preserved. Empty when the file is absent (a vault always has
    /// a config, but the read is defensive).
    pub content: String,
    /// A 16-char lowercase-hex content hash of `content`.
    pub hash: String,
}

/// A human-readable reason a candidate config would fail to open —
/// either a TOML syntax error (with the line/column TOML reports) or a
/// domain validation error (a reserved-name shadow, a bad folder, a
/// redeclared period key, …).
///
/// This is the SAME error [`validate_config_str`] returns for both the
/// PR1 dry-run and the eventual PR3 save gate, so the two cannot
/// disagree. `line`/`col` are populated only for a TOML parse failure;
/// validation errors carry a message alone.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, thiserror::Error)]
#[error("{message}")]
pub struct ConfigValidationError {
    /// The full, human-readable message. For a TOML parse error this is
    /// TOML's own rendering (which already names the line/column and
    /// shows the offending snippet); for a validation error it is the
    /// domain error's `Display`.
    pub message: String,
    /// 1-based line of a TOML parse error, when TOML reports a span.
    pub line: Option<u32>,
    /// 1-based column of a TOML parse error, when TOML reports a span.
    pub col: Option<u32>,
}

impl Vault {
    /// Read `.cuaderno/config.toml` verbatim through the store, with its
    /// content hash. A missing file yields an empty-content document
    /// rather than an error — a vault always has a config, but the read
    /// stays defensive so the inspector never hard-fails on a fresh or
    /// partially-set-up vault.
    pub fn read_config_raw(&self) -> Result<ConfigDocument, DomainError> {
        let path = VaultPath::new(CONFIG_FILE)?;
        let content = match self.store.read_file(&path) {
            Ok(content) => content,
            // Absence is not an error here — hand back an empty document.
            Err(StoreError::NotFound(_)) => String::new(),
            Err(err) => return Err(err.into()),
        };
        let hash = content_hash(&content);
        Ok(ConfigDocument { content, hash })
    }
}

/// Dry-run the exact validation `Vault::new` runs against a candidate
/// config string, without mutating anything. Pure.
///
/// Composes, in the SAME order as [`Vault::new`]:
/// 1. `toml::from_str::<VaultConfig>` — parse (a syntax error carries
///    the line/column);
/// 2. `VaultConfig::ignore_set` — compile the `ignore` globs;
/// 3. `TypeRegistry::validate` — the `[note_types.*]` / `[schemas.*]`
///    checks (reserved-name shadowing, folder legality, redeclared
///    period keys, `values`-on-non-string, `list = true`, unknown
///    field-spec keys, dangling `title_field`/`date_field`, …).
///
/// `Vault::new` additionally runs `reconcile`, which is index I/O, not
/// config validation — deliberately excluded so this stays pure and the
/// gate reflects exactly "is this config well-formed and openable".
pub fn validate_config_str(content: &str) -> Result<(), ConfigValidationError> {
    // 1. Parse. TOML's error Display already names the line/column and
    //    shows the snippet, so it is the message verbatim; the span (if
    //    any) is turned into structured line/col for the editor.
    let config: VaultConfig = match toml::from_str(content) {
        Ok(config) => config,
        Err(err) => {
            let (line, col) = err.span().map(|span| line_col(content, span.start)).unzip();
            return Err(ConfigValidationError {
                message: err.to_string(),
                line,
                col,
            });
        }
    };

    // 2. + 3. The structural + domain checks, mapped to a message-only
    //    validation error (no span — these are semantic, not positional).
    config
        .ignore_set()
        .map_err(|err| ConfigValidationError::from_message(err.to_string()))?;
    TypeRegistry::validate(&config)
        .map_err(|err| ConfigValidationError::from_message(err.to_string()))?;
    Ok(())
}

impl ConfigValidationError {
    /// A validation error carrying only a message (no source position).
    fn from_message(message: String) -> Self {
        Self {
            message,
            line: None,
            col: None,
        }
    }
}

/// The 1-based `(line, column)` of `byte_offset` within `content`.
/// Column counts characters, not bytes, so a multi-byte UTF-8 glyph
/// advances the column by one — matching how an editor renders a
/// caret. An offset past the end clamps to the final position.
fn line_col(content: &str, byte_offset: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut col = 1u32;
    for (idx, ch) in content.char_indices() {
        if idx >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
