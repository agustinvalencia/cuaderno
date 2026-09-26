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
use cdno_core::store::VaultStore;

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

/// Why a [`Vault::save_config_raw`] call was rejected — the three
/// distinct failure modes a config save can hit, tagged so the desktop
/// UI can react to each precisely (show the validation message inline,
/// prompt a reload on a concurrent edit, or toast a generic failure).
///
/// The ordering of the variants mirrors the save gate's own order:
/// [`Validation`](Self::Validation) is checked FIRST and is the
/// never-brick guarantee — a candidate that would not reopen is rejected
/// before anything is written; [`Conflict`](Self::Conflict) is the
/// compare-and-swap guard against a concurrent hand-edit; and
/// [`Internal`](Self::Internal) covers a store/disk fault.
#[derive(Debug, Clone, serde::Serialize, thiserror::Error)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ConfigSaveError {
    /// The candidate config would NOT reopen — the pre-write validation
    /// gate rejected it. Carries the SAME [`ConfigValidationError`] the
    /// dry-run `validate_config` returns, so the UI renders a save
    /// rejection and an on-demand check identically. When this is
    /// returned the on-disk file is byte-identical to before: nothing was
    /// written.
    #[error("{0}")]
    Validation(ConfigValidationError),
    /// The on-disk config changed since the editor read it (a concurrent
    /// hand-edit) — the compare-and-swap detected a hash mismatch and
    /// refused to clobber the newer file. The UI must reload before
    /// saving again. Nothing was written.
    #[error("the config changed on disk since it was opened; reload before saving")]
    Conflict,
    /// A store/disk failure while reading the current config or writing
    /// the new one — not user-fixable. The underlying error detail is
    /// carried in the message: this is a local single-user app, so there
    /// is no client/server boundary to withhold it from.
    #[error("{0}")]
    Internal(String),
}

impl Vault {
    /// Read `.cuaderno/config.toml` verbatim through the store, with its
    /// content hash. A missing file yields an empty-content document
    /// rather than an error — a vault always has a config, but the read
    /// stays defensive so the inspector never hard-fails on a fresh or
    /// partially-set-up vault.
    pub fn read_config_raw(&self) -> Result<ConfigDocument, DomainError> {
        read_config_from(&*self.store)
    }

    pub fn save_config_raw(
        &self,
        content: &str,
        expected_hash: &str,
    ) -> Result<ConfigDocument, ConfigSaveError> {
        save_config_to(&*self.store, content, expected_hash)
    }
}

/// Read `.cuaderno/config.toml` verbatim from a store, with its content hash.
///
/// Store-level rather than `Vault`-level because the CLI's `cdno config`
/// verbs must work on a vault that will NOT open — a broken config is
/// exactly when you need to read, check and fix it, and `Vault::new`
/// validates the config before it hands you a `Vault`. Taking the store
/// alone keeps one implementation shared with [`Vault::read_config_raw`].
pub fn read_config_from(store: &dyn VaultStore) -> Result<ConfigDocument, DomainError> {
    let path = VaultPath::new(CONFIG_FILE)?;
    let content = match store.read_file(&path) {
        Ok(content) => content,
        // Absence is not an error here — hand back an empty document.
        Err(StoreError::NotFound(_)) => String::new(),
        Err(err) => return Err(err.into()),
    };
    let hash = content_hash(&content);
    Ok(ConfigDocument { content, hash })
}

/// The validate-then-compare-and-swap-then-write core of a config save.
///
/// The ONLY path that writes `.cuaderno/config.toml`, structured so a config
/// that would not reopen can never reach the disk. Store-level for the same
/// reason as [`read_config_from`]: fixing a broken config must not require a
/// vault that opens. [`Vault::save_config_raw`] delegates here, so the
/// desktop and the CLI cannot drift on the gate.
///
/// The order is load-bearing — do not reorder:
///
/// 1. **VALIDATE FIRST (never-brick).** Run [`validate_config_str`], the
///    exact composition `Vault::new` runs. On ANY error return
///    [`ConfigSaveError::Validation`] and write NOTHING.
/// 2. **COMPARE-AND-SWAP.** Re-read the current on-disk config, hash it, and
///    compare to `expected_hash`. A mismatch means a concurrent hand-edit
///    landed underneath the editor; return [`ConfigSaveError::Conflict`] and
///    write nothing rather than silently clobber the newer file. (A tiny
///    TOCTOU window between this read and the write is accepted
///    deliberately — this is a single-user tool.)
/// 3. **WRITE VERBATIM**, confined to [`CONFIG_FILE`] by [`VaultPath`], so
///    comments, key order and the `[variables]` block survive exactly.
///    Config is not an append-only note, so this is a plain confined
///    `write_file` — no `VaultTransaction`, and no cross-process lock.
pub fn save_config_to(
    store: &dyn VaultStore,
    content: &str,
    expected_hash: &str,
) -> Result<ConfigDocument, ConfigSaveError> {
    // STEP 1 — VALIDATE FIRST. The hard precondition: a candidate that would
    // not reopen is rejected here, before the compare-and-swap read or any
    // write. This single early return is the never-brick guarantee.
    validate_config_str(content).map_err(ConfigSaveError::Validation)?;

    let path =
        VaultPath::new(CONFIG_FILE).map_err(|err| ConfigSaveError::Internal(err.to_string()))?;

    // STEP 2 — COMPARE-AND-SWAP against the current on-disk config. A missing
    // file hashes as empty content (matching `read_config_from`), so a
    // first-ever write carries the empty-content hash as its baseline.
    let current = match store.read_file(&path) {
        Ok(current) => current,
        Err(StoreError::NotFound(_)) => String::new(),
        Err(err) => return Err(ConfigSaveError::Internal(err.to_string())),
    };
    if content_hash(&current) != expected_hash {
        return Err(ConfigSaveError::Conflict);
    }

    // STEP 3 — WRITE the buffer verbatim. Reaching here means the candidate
    // validated AND the on-disk file is the one the caller last saw.
    store
        .write_file(&path, content)
        .map_err(|err| ConfigSaveError::Internal(err.to_string()))?;

    // Re-read the persisted file for an authoritative content + hash, rather
    // than trusting the in-memory buffer.
    let saved = store
        .read_file(&path)
        .map_err(|err| ConfigSaveError::Internal(err.to_string()))?;
    let hash = content_hash(&saved);
    Ok(ConfigDocument {
        content: saved,
        hash,
    })
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
