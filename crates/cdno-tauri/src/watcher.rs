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

use cdno_core::config::IgnoreSet;
use cdno_core::index::VaultIndex;
use cdno_core::path::VaultPath;
use cdno_core::reconcile::reconcile;
use cdno_core::store::VaultStore;
use cdno_core::watcher::FileEvent;

use crate::events::{
    self, Origin, VAULT_CHANGED, VaultArea, VaultChanged, WATCHER_STATUS, WatcherStatus,
};
use crate::state::AppState;

/// Handles the watcher thread needs, cloned out of the bootstrap
/// before `Vault` swallowed the store/index Arcs.
pub struct WatcherDeps {
    pub store: Arc<dyn VaultStore>,
    pub index: Arc<dyn VaultIndex>,
    pub ignore: Arc<IgnoreSet>,
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

fn handle_batch(app: &AppHandle, deps: &WatcherDeps, batch: Vec<FileEvent>) {
    let mut rescan = false;
    let mut external: Vec<VaultPath> = Vec::new();

    let state = app.state::<AppState>();
    let journal = &state.journal;
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

    if !rescan && external.is_empty() {
        // Pure self-echo (or noise) — reconcile is still cheap
        // insurance, but there is nothing to tell the frontend.
        run_reconcile(deps);
        return;
    }

    // Reconcile ALWAYS runs before the emit so the frontend's
    // refetches hit an index that already reflects the change.
    let ok = run_reconcile(deps);
    let _ = app.emit(
        WATCHER_STATUS,
        WatcherStatus {
            state: if ok { "ok" } else { "degraded" },
        },
    );

    let payload = if rescan {
        // Events were dropped somewhere — invalidate everything.
        VaultChanged {
            origin: Origin::External,
            areas: all_areas(),
            paths: Vec::new(),
        }
    } else {
        let mut areas: Vec<VaultArea> = external.iter().filter_map(events::classify).collect();
        areas.sort_by_key(|a| format!("{a:?}"));
        areas.dedup();
        if areas.is_empty() {
            return; // nothing any view renders
        }
        VaultChanged {
            origin: Origin::External,
            areas,
            paths: external.iter().map(|p| p.to_string()).collect(),
        }
    };
    let _ = app.emit(VAULT_CHANGED, payload);
}

/// Repair the index; `false` (→ degraded pill + poll fallback in the
/// frontend) only on catastrophic failure, not per-file errors.
fn run_reconcile(deps: &WatcherDeps) -> bool {
    match reconcile(&deps.store, &deps.index, &deps.ignore) {
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
/// (plus the config file itself). The `.cdno-wip-*` atomic-write temp
/// files carry no `.md` extension, so this also drops our own write
/// staging.
fn is_note_path(path: &VaultPath) -> bool {
    let p = path.as_path();
    if p.starts_with(cdno_core::paths::CUADERNO_DIR) {
        return p.file_name().and_then(|f| f.to_str()) == Some("config.toml");
    }
    p.extension().and_then(|e| e.to_str()) == Some("md")
}

fn all_areas() -> Vec<VaultArea> {
    vec![
        VaultArea::Projects,
        VaultArea::Actions,
        VaultArea::Daily,
        VaultArea::Weekly,
        VaultArea::Commitments,
        VaultArea::Portfolios,
        VaultArea::Stewardships,
        VaultArea::Questions,
        VaultArea::Inbox,
        VaultArea::Config,
    ]
}
