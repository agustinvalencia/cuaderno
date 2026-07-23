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

/// A `cuaderno://note/<path>` deep link was opened; payload is the vault
/// path, which the frontend navigates the reader to.
pub const OPEN_NOTE_DEEPLINK: &str = "deeplink:open-note";

/// How many markdown files the most recent reconciliation left out of
/// the index, and why (#440). Every pass rewrites it — the startup one,
/// a config rebuild, and each of the watcher's.
///
/// A file absent from the index is absent from search, lint and backlinks
/// too, so the counts have to be reachable — the app previously logged
/// only added/updated/removed and dropped these on the floor, which is how
/// an over-broad `ignore` glob could evict every portfolio note and present
/// as "the Portfolios section is broken" rather than "my config is wrong".
///
/// Delivered as a queryable command rather than a startup event: the
/// webview may not be listening when reconciliation finishes, and a notice
/// the user never sees is the failure being fixed.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct IndexExclusions {
    /// Markdown excluded by the config `ignore` globs.
    pub ignored: usize,
    /// Markdown skipped as attachment artefacts owned by an evidence stub.
    /// Excluded by design, never a sign of misconfiguration.
    pub artefacts: usize,
    /// Markdown files that did make it into the index.
    pub indexed: usize,
    /// Whether `ignored` is large enough to suggest an over-broad glob
    /// rather than deliberate housekeeping. See [`glob_looks_over_broad`].
    pub ignore_looks_over_broad: bool,
    /// Bumped once per config rebuild, and only there.
    ///
    /// The notice is dismissible, and a dismissal has to survive ordinary
    /// vault growth: in the very vault this exists for, a glob swallowing a
    /// tree means every note filed into that tree changes the counts. But it
    /// must NOT survive the user editing their globs — swapping one
    /// over-broad pattern for another never shows an intermediate healthy
    /// state, so a dismissal keyed on the condition alone would silence the
    /// notice for the rest of the session. This counter is what the frontend
    /// keys on: it moves when the globs might have, and not otherwise.
    pub config_generation: u32,
}

/// The floor below which an `ignore` count is never worth a notice, however
/// large a share of a tiny vault it is.
const OVER_BROAD_MIN_FILES: usize = 5;
/// The share of all markdown above which an `ignore` count reads as a
/// mistake rather than housekeeping, as a percentage.
const OVER_BROAD_PERCENT: usize = 10;

/// Whether `ignored` out of `total` markdown files suggests an over-broad
/// `ignore` glob.
///
/// Proportional with a floor, because both halves matter: excluding
/// `CLAUDE.md` from a 168-note vault is 0.6% and must stay silent, while
/// the glob that started #440 excluded 54 of 221 files (24%) and had to
/// shout. The floor keeps a three-note scratch vault from tripping the
/// percentage on a single legitimate exclusion.
pub fn glob_looks_over_broad(ignored: usize, total: usize) -> bool {
    ignored >= OVER_BROAD_MIN_FILES && ignored * 100 > total * OVER_BROAD_PERCENT
}

impl IndexExclusions {
    /// Read the counts off a reconciliation report, carrying `generation`
    /// through — a reconcile does not change the config, so only the
    /// rebuild path advances it (see [`Self::next_generation`]).
    pub fn from_report(
        report: &cdno_core::reconcile::ReconciliationReport,
        generation: u32,
    ) -> Self {
        let total = report.scanned + report.ignored + report.artefacts;
        Self {
            ignored: report.ignored,
            artefacts: report.artefacts,
            indexed: report.scanned,
            ignore_looks_over_broad: glob_looks_over_broad(report.ignored, total),
            config_generation: generation,
        }
    }

    /// The generation a config rebuild should publish under. Saturating, so
    /// a very long session degrades to "stops advancing" rather than
    /// wrapping to a value the frontend has already seen.
    pub fn next_generation(previous: u32) -> u32 {
        previous.saturating_add(1)
    }
}

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

/// The health of the on-disk config after an external-edit reload
/// (GH #365 PR4, #384). Three outcomes the UI renders distinctly.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigHealth {
    /// The config opened cleanly; clears any prior notice.
    Valid,
    /// The config content is invalid (bad TOML/glob/note-type/schema); the
    /// app kept the last good config.
    Invalid,
    /// The reload was transiently blocked (the vault write lock was held,
    /// or an IO/index hiccup) — the config itself may be fine. The app kept
    /// the last good config and applies the change on the next config edit.
    /// Distinct from `Invalid` so the banner never cries "broken config" for
    /// mere contention (#384).
    Deferred,
}

/// Payload for [`CONFIG_STATUS`]. `message` carries the open/transient error
/// detail for `Invalid` / `Deferred`; `Valid` clears any prior notice and
/// carries no message. Exported to TS because the UI surfaces the notice as
/// a non-red banner (GH #365 PR4, #384).
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConfigStatus {
    pub health: ConfigHealth,
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
