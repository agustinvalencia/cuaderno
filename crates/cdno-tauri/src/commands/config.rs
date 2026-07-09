//! Config inspector (#365, PR1): read the raw `.cuaderno/config.toml`
//! and dry-run its validation. Both commands are pure reads — no
//! journal, no events — mirroring the Templates read posture.
//!
//! `read_config` hands the UI the verbatim file plus a content hash (the
//! hash is inert in PR1; it exists so a later compare-and-swap write —
//! PR3 — and this read agree on identity). `validate_config` runs the
//! exact composition `Vault::new` performs (`toml::from_str` →
//! `ignore_set` → `TypeRegistry::validate`) against a candidate string,
//! reporting `Ok(())` or a structured `{ message, line?, col? }` — the
//! same error type the PR3 save gate will reuse, so the dry-run and the
//! real gate cannot drift.

use cdno_domain::Vault;
use cdno_domain::vault::{ConfigDocument, ConfigValidationError, validate_config_str};

use crate::error::CmdError;
use crate::state::AppState;
use crate::with_vault::with_vault;

/// Read the raw config document (content + hash). Public and
/// synchronous — the test seam, exercised directly over the Memory
/// doubles.
pub fn read_config_impl(vault: &Vault) -> Result<ConfigDocument, CmdError> {
    Ok(vault.read_config_raw()?)
}

/// The raw `.cuaderno/config.toml` text plus its content hash — the
/// inspector's read. A pure read: no journal, no emit.
#[tauri::command]
pub async fn read_config(state: tauri::State<'_, AppState>) -> Result<ConfigDocument, CmdError> {
    with_vault(&state.vault, read_config_impl).await?
}

/// Dry-run the config validation `Vault::new` runs against `content`,
/// without touching the vault. `Ok(())` means the config would open;
/// `Err` carries a human-readable message (and, for a TOML syntax
/// error, the line/column). A pure read — it depends only on its input,
/// not on the open vault, so it never blocks the vault lock.
#[tauri::command]
pub async fn validate_config(content: String) -> Result<(), ConfigValidationError> {
    // The check is a small in-memory parse + validation pass; it doesn't
    // touch the store or index, so it runs inline rather than through
    // `with_vault` (there is no vault to borrow).
    validate_config_str(&content)
}
