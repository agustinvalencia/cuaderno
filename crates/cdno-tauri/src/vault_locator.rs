//! Startup vault-path resolution for the desktop app.
//!
//! A Finder-launched app inherits no shell environment, so the bare
//! `CUADERNO_VAULT_PATH` contract the CLI and MCP binaries rely on
//! leaves a GUI user with a silent abort to Console.app — the roughest
//! edge of early user testing (GH #331). This module layers three
//! sources into a single [`Resolution`]:
//!
//! 1. the `CUADERNO_VAULT_PATH` env var — an explicit override, honoured
//!    verbatim and expected to *fail loudly* if it points at a non-vault;
//! 2. a persisted setting at `<app_config_dir>/vault.json`, accepted only
//!    if it passes a cheap `.cuaderno/` marker check;
//! 3. otherwise, the caller must prompt (a native folder picker).
//!
//! **A stored path falls through to the picker on *any* failure, not just
//! a moved/deleted vault.** Two gates guard it: the cheap marker check
//! here (vault gone or never was) and the full `open_vault` in `lib.rs`
//! (marker present but the open fails — corrupt config, unopenable index,
//! or a TOCTOU delete between check and open). Either gate degrades to the
//! picker instead of aborting; only the explicit env override hard-fails.
//! The ordering and the marker-check fall-through are pure and unit-tested
//! here ([`resolve`] takes the check as a closure, so no dialog or
//! filesystem vault is needed to test the branching); the full-open
//! fall-through lives with the picker loop in `lib.rs`, which is
//! untestable UI and stays thin.
//!
//! Caveat: the marker check `stat`s `<candidate>/.cuaderno` during
//! resolve. A stored path on a hard-mounted but unreachable network share
//! can block that `stat` (kernel-level, uninterruptible), stalling
//! startup — a documented limitation, not something this seam can guard.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Env var naming the vault root — the same contract as the CLI and MCP
/// binaries. Treated as an explicit override: when set it is honoured
/// verbatim, and a bad value is a hard error rather than a silent
/// fall-through to the picker (a user who set the variable meant it).
pub const ENV_VAULT_PATH: &str = "CUADERNO_VAULT_PATH";

/// Basename of the persisted-setting file, resolved under the platform
/// app-config dir by the caller.
const SETTING_FILE: &str = "vault.json";

/// The persisted vault-path setting. Deliberately a one-field struct so
/// the on-disk shape is obvious and forward-compatible — serde ignores
/// unknown keys, so a future field never breaks an old file.
#[derive(Debug, Serialize, Deserialize)]
struct VaultSetting {
    vault_path: String,
}

/// Where the resolved vault comes from. `Env` and `Stored` each carry a
/// concrete root ready to open; `NeedsPicker` means nothing usable was
/// found and the caller must prompt the user.
#[derive(Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Explicit `CUADERNO_VAULT_PATH` override. Returned even when it
    /// fails to validate: the caller opens it and surfaces the error,
    /// because an explicit override must fail loudly, never fall through.
    Env(PathBuf),
    /// A previously persisted path that still opens as a vault.
    Stored(PathBuf),
    /// No override and no valid stored path — the caller shows the picker.
    NeedsPicker,
}

/// Read the env override, if present and non-empty.
pub fn resolve_from_env() -> Option<PathBuf> {
    std::env::var_os(ENV_VAULT_PATH)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Read the persisted vault path from `<config_dir>/vault.json`.
///
/// Returns `None` when the file is missing, unreadable, or corrupt — a
/// lost setting is never fatal here; the caller falls back to the
/// picker. (Whether the path still *opens* as a vault is a separate
/// check, applied by [`resolve`].)
pub fn read_setting(config_dir: &Path) -> Option<PathBuf> {
    let raw = fs::read_to_string(config_dir.join(SETTING_FILE)).ok()?;
    let setting: VaultSetting = serde_json::from_str(&raw).ok()?;
    Some(PathBuf::from(setting.vault_path))
}

/// Persist `vault` to `<config_dir>/vault.json`, creating the config dir
/// if it does not exist yet (first launch).
pub fn write_setting(config_dir: &Path, vault: &Path) -> io::Result<()> {
    fs::create_dir_all(config_dir)?;
    let setting = VaultSetting {
        vault_path: vault.to_string_lossy().into_owned(),
    };
    // Serialising a two-string struct cannot realistically fail; remap
    // the error into `io` anyway so the caller handles one error type.
    let json = serde_json::to_string_pretty(&setting)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(config_dir.join(SETTING_FILE), json)
}

/// Decide where the vault comes from, given the already-read env
/// override, the app-config dir, and a `validate` predicate answering
/// "does this path open as a vault?".
///
/// Ordering:
/// - an explicit env override wins unconditionally and is returned
///   *unvalidated* — the caller opens it and hard-fails on a bad path,
///   so an override never silently degrades to the picker;
/// - otherwise a stored path is used only if `validate` accepts it; a
///   stored path that no longer opens (vault moved or deleted) is
///   treated as absent and the result is `NeedsPicker`.
pub fn resolve(
    env: Option<PathBuf>,
    config_dir: &Path,
    validate: impl Fn(&Path) -> bool,
) -> Resolution {
    if let Some(root) = env {
        return Resolution::Env(root);
    }
    if let Some(stored) = read_setting(config_dir) {
        if validate(&stored) {
            return Resolution::Stored(stored);
        }
        tracing::warn!(
            path = %stored.display(),
            "persisted vault path no longer opens as a vault; falling back to the picker",
        );
    }
    Resolution::NeedsPicker
}
