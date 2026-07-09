//! The watcher thread: debounced filesystem batches in, reconcile +
//! `vault:changed` emissions out (plan §3.1).
//!
//! Runs on a dedicated `std::thread` — never the blocking pool — and
//! owns the receiving end of the `FsFileWatcher` channel for the
//! process lifetime. Per batch: filter to note-relevant paths, check
//! the `WriteJournal` for self-echoes, run a full `reconcile()`
//! (path-agnostic repair — correctness never depends on event
//! fidelity), classify the surviving paths into areas, and emit.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use tauri::{AppHandle, Emitter, Manager};

use arc_swap::ArcSwap;

use cdno_core::config::IgnoreSet;
use cdno_core::index::VaultIndex;
use cdno_core::path::VaultPath;
use cdno_core::reconcile::reconcile;
use cdno_core::store::VaultStore;
use cdno_core::watcher::FileEvent;

use crate::commands::config::rebuild_and_swap;
use crate::events::{
    self, CONFIG_STATUS, ConfigStatus, Origin, VAULT_CHANGED, VaultArea, VaultChanged,
    WATCHER_STATUS, WatcherStatus,
};
use crate::state::{AppState, WriteJournal};

/// Handles the watcher thread needs, cloned out of the bootstrap
/// before `Vault` swallowed the store/index Arcs.
pub struct WatcherDeps {
    pub store: Arc<dyn VaultStore>,
    pub index: Arc<dyn VaultIndex>,
    /// The active ignore matcher, shared by reference with [`AppState`] so a
    /// config reload can swap a fresh set in and the next reconcile honours
    /// the new globs without a restart (GH #365 PR4). Loaded per reconcile
    /// call rather than cached, so a mid-session swap takes effect on the
    /// very next batch.
    pub ignore: Arc<ArcSwap<IgnoreSet>>,
}

/// Consume debounced batches until the sender (the `FsFileWatcher`,
/// owned by the app's setup state) goes away. Spawn via
/// `std::thread::spawn`.
pub fn run(app: AppHandle, deps: WatcherDeps, rx: Receiver<Vec<FileEvent>>) {
    while let Ok(batch) = rx.recv() {
        handle_batch(&app, &deps, batch);
    }
    tracing::debug!("watcher channel closed; watcher thread exiting");
}

/// What a debounced batch means for the frontend. Computed by the
/// pure [`plan_batch`] so the decision table is unit-testable without
/// an `AppHandle`.
#[derive(Debug, PartialEq)]
pub enum BatchPlan {
    /// Only our own echoes (or noise) — reconcile as insurance, emit
    /// no change event (the command already emitted a precise
    /// `origin: self` one).
    Quiet,
    /// Events were dropped somewhere — invalidate everything.
    Rescan,
    /// External edits in these areas.
    External {
        areas: Vec<VaultArea>,
        paths: Vec<String>,
        /// Whether the batch touched `.cuaderno/config.toml` itself — the
        /// trigger for a live vault rebuild (GH #365 PR4). Template `.md`
        /// files under `.cuaderno/templates/` also classify as
        /// `VaultArea::Config` but leave this `false`: they don't change the
        /// note-type registry, so they need only the ordinary refetch.
        config_changed: bool,
    },
}

/// Classify a batch against the self-write journal.
pub fn plan_batch(journal: &WriteJournal, batch: Vec<FileEvent>) -> BatchPlan {
    let mut rescan = false;
    let mut external: Vec<VaultPath> = Vec::new();
    for event in batch {
        match event {
            FileEvent::Rescan => rescan = true,
            FileEvent::Changed(path) | FileEvent::Removed(path) => {
                if !is_note_path(&path) {
                    continue;
                }
                // Self-echo: our own command already emitted a
                // precise origin:self event for this path.
                if journal.is_recent_self_write(&path) {
                    continue;
                }
                external.push(path);
            }
        }
    }

    if rescan {
        return BatchPlan::Rescan;
    }
    let mut areas: Vec<VaultArea> = external.iter().filter_map(events::classify).collect();
    areas.sort();
    areas.dedup();
    if areas.is_empty() {
        return BatchPlan::Quiet;
    }
    // A surviving (non-echo) edit to the config file itself means the
    // note-type registry may have changed — the watcher must rebuild the
    // live vault, not just refetch. Template edits under `.cuaderno/templates/`
    // are excluded (see `is_config_file`).
    let config_changed = external.iter().any(is_config_file);
    BatchPlan::External {
        areas,
        paths: external.iter().map(|p| p.to_string()).collect(),
        config_changed,
    }
}

/// The vault-relative config file the reload watches, distinct from the
/// template files that also classify as `VaultArea::Config` but don't
/// change the note-type registry. Matches the exact `.cuaderno/config.toml`
/// path, not any `config.toml` under `.cuaderno/`, so a stray
/// `.cuaderno/templates/config.toml` could never trigger a spurious rebuild.
fn is_config_file(path: &VaultPath) -> bool {
    path.as_path() == std::path::Path::new(cdno_core::paths::CONFIG_FILE)
}

fn handle_batch(app: &AppHandle, deps: &WatcherDeps, batch: Vec<FileEvent>) {
    let state = app.state::<AppState>();
    let plan = plan_batch(&state.journal, batch);

    // Reconcile ALWAYS runs (even for pure self-echo batches — cheap
    // insurance), and its health is ALWAYS reported: once write
    // commands land, self-echo is the common batch shape, and a
    // degraded index must not stay invisible until the next external
    // edit.
    let ok = run_reconcile(deps);
    let _ = app.emit(
        WATCHER_STATUS,
        WatcherStatus {
            state: if ok { "ok" } else { "degraded" },
        },
    );

    match plan {
        BatchPlan::Quiet => {}
        BatchPlan::Rescan => {
            let _ = app.emit(
                VAULT_CHANGED,
                VaultChanged {
                    origin: Origin::External,
                    areas: all_areas(),
                    paths: Vec::new(),
                },
            );
        }
        BatchPlan::External {
            areas,
            paths,
            config_changed,
        } if config_changed => handle_external_config_edit(app, areas, paths),
        BatchPlan::External { areas, paths, .. } => {
            let _ = app.emit(
                VAULT_CHANGED,
                VaultChanged {
                    origin: Origin::External,
                    areas,
                    paths,
                },
            );
        }
    }
}

/// Apply an external `.cuaderno/config.toml` edit: rebuild the live vault
/// (and its ignore set) from the new config, then tell the frontend what
/// happened (GH #365 PR4).
///
/// On a successful rebuild, one all-areas `vault:changed` refetches
/// everything the new config might have reshaped (note types, schemas,
/// folders), and a `config:status` valid clears any prior error banner.
///
/// On a rebuild failure, [`rebuild_and_swap`] has already guaranteed the
/// OLD vault stays live (never-brick), so the session is never left
/// vault-less. We surface the error as a `config:status` notice the UI
/// shows as a non-red banner, then STILL emit `vault:changed` for the
/// batch's non-config edits so those views refresh — a broken config edit
/// bundled with a real note edit must not swallow the note edit.
fn handle_external_config_edit(app: &AppHandle, areas: Vec<VaultArea>, paths: Vec<String>) {
    match rebuild_and_swap(app) {
        Ok(()) => {
            let _ = app.emit(
                VAULT_CHANGED,
                VaultChanged {
                    origin: Origin::External,
                    areas: all_areas(),
                    paths,
                },
            );
            let _ = app.emit(
                CONFIG_STATUS,
                ConfigStatus {
                    valid: true,
                    message: None,
                },
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "external config edit did not apply; keeping the last good config");
            let _ = app.emit(
                CONFIG_STATUS,
                ConfigStatus {
                    valid: false,
                    message: Some(e.to_string()),
                },
            );
            let _ = app.emit(
                VAULT_CHANGED,
                VaultChanged {
                    origin: Origin::External,
                    areas,
                    paths,
                },
            );
        }
    }
}

/// Repair the index; `false` (→ degraded pill + poll fallback in the
/// frontend) only on catastrophic failure, not per-file errors.
fn run_reconcile(deps: &WatcherDeps) -> bool {
    // Load the current ignore set per call so a config reload's swap
    // (GH #365 PR4) is honoured on the very next reconcile.
    let ignore = deps.ignore.load_full();
    match reconcile(&deps.store, &deps.index, &ignore) {
        Ok(report) => {
            if !report.errors.is_empty() {
                tracing::warn!(
                    errors = report.errors.len(),
                    "reconcile completed with per-file errors"
                );
            }
            true
        }
        Err(e) => {
            tracing::error!(error = %e, "watcher reconcile failed");
            false
        }
    }
}

/// Paths the UI could care about: markdown notes outside `.cuaderno/`
/// (plus the config file and the template files, which drive the log
/// form's dynamic fields). The `.cdno-wip-*` atomic-write temp files
/// carry no `.md` extension, so this also drops our own write staging.
fn is_note_path(path: &VaultPath) -> bool {
    let p = path.as_path();
    if p.starts_with(cdno_core::paths::CUADERNO_DIR) {
        // Template edits change which fields the log form gathers, so a
        // `.cuaderno/templates/*.md` touch must survive filtering; the
        // config file matters too. Everything else under `.cuaderno`
        // (index db, lock file) is churn no view renders.
        if p.starts_with(cdno_core::paths::TEMPLATES_DIR) {
            return p.extension().and_then(|e| e.to_str()) == Some("md");
        }
        return p.file_name().and_then(|f| f.to_str()) == Some("config.toml");
    }
    p.extension().and_then(|e| e.to_str()) == Some("md")
}

/// Every area the frontend renders — the invalidate-everything set,
/// shared by the watcher's `Rescan` plan and the config-reload command
/// (GH #365), which both mean "refetch the whole UI".
pub(crate) fn all_areas() -> Vec<VaultArea> {
    vec![
        VaultArea::Projects,
        VaultArea::Actions,
        VaultArea::Daily,
        VaultArea::Weekly,
        VaultArea::Monthly,
        VaultArea::Commitments,
        VaultArea::Portfolios,
        VaultArea::Stewardships,
        VaultArea::Questions,
        VaultArea::Inbox,
        VaultArea::Config,
    ]
}
