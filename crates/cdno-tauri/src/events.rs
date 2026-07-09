//! Event names and payloads on the Rust → webview channel, plus the
//! path → area classifier the invalidation map keys on.

use cdno_core::path::VaultPath;

/// Something in the vault changed. `areas` drives the frontend's
/// coarse query invalidation; `paths` is carried for finer-grained
/// invalidation later.
pub const VAULT_CHANGED: &str = "vault:changed";
/// Watcher health: `{ state: "ok" | "degraded" }`.
pub const WATCHER_STATUS: &str = "watcher:status";
/// The on-disk config was re-read after an external edit: `valid:false`
/// with a message when it failed to open (the app kept the last good
/// config), or `valid:true` to clear a prior notice (GH #365 PR4).
pub const CONFIG_STATUS: &str = "config:status";
/// The local calendar date rolled over (sleep past midnight, TZ
/// change) — invalidate everything date-dependent.
pub const CLOCK_DAY_CHANGED: &str = "clock:day-changed";
/// The global shortcut summoned the capture window (M3).
pub const CAPTURE_SHOW: &str = "capture:show";

#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct VaultChanged {
    pub origin: Origin,
    pub areas: Vec<VaultArea>,
    pub paths: Vec<String>,
}

#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// A command in this process performed the write and already
    /// invalidated precisely; the watcher suppresses its echo.
    SelfWrite,
    /// Some other writer (nvim, CLI, MCP, sync) touched the vault.
    External,
}

/// Coarse buckets the frontend maps to query-key prefixes. One area
/// per top-level vault directory, split where the journal folds two
/// note types into one tree.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultArea {
    Projects,
    Actions,
    Daily,
    Weekly,
    Monthly,
    Commitments,
    Portfolios,
    Stewardships,
    Questions,
    Inbox,
    Config,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WatcherStatus {
    pub state: &'static str,
}

/// Payload for [`CONFIG_STATUS`]. `valid:false` carries the open error's
/// `message` (the app kept the last good config); `valid:true` clears any
/// prior notice and carries no message. Exported to TS because the UI
/// surfaces the notice as a non-red banner (GH #365 PR4).
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConfigStatus {
    pub valid: bool,
    pub message: Option<String>,
}

/// Classify a vault-relative path into its area, or `None` for paths
/// no view renders (attachments at the root, unknown directories).
/// Daily/weekly/monthly notes live under
/// `journal/<year>/{daily,weekly,monthly}/`, so the journal split keys on
/// an inner path component rather than the first — the calendar view
/// reads all three note types, so each maps to its own area the
/// invalidation map wires back to the calendar's queries.
pub fn classify(path: &VaultPath) -> Option<VaultArea> {
    let p = path.as_path();
    let mut components = p.components().filter_map(|c| c.as_os_str().to_str());
    let first = components.next()?;
    match first {
        "projects" => Some(VaultArea::Projects),
        "actions" => Some(VaultArea::Actions),
        "commitments" => Some(VaultArea::Commitments),
        "portfolios" => Some(VaultArea::Portfolios),
        "stewardships" => Some(VaultArea::Stewardships),
        "questions" => Some(VaultArea::Questions),
        "inbox" => Some(VaultArea::Inbox),
        "journal" => {
            if p.components().any(|c| c.as_os_str() == "daily") {
                Some(VaultArea::Daily)
            } else if p.components().any(|c| c.as_os_str() == "weekly") {
                Some(VaultArea::Weekly)
            } else if p.components().any(|c| c.as_os_str() == "monthly") {
                Some(VaultArea::Monthly)
            } else {
                None
            }
        }
        ".cuaderno" => {
            // The config file and the template files both drive the UI
            // (config.toml holds vault settings; templates decide which
            // fields the tracking log form gathers), so both map to
            // Config. The index db and lock file are churn no view
            // renders.
            if p.starts_with(cdno_core::paths::TEMPLATES_DIR)
                && p.extension().and_then(|e| e.to_str()) == Some("md")
            {
                return Some(VaultArea::Config);
            }
            (p.file_name().and_then(|f| f.to_str()) == Some("config.toml"))
                .then_some(VaultArea::Config)
        }
        _ => None,
    }
}
