//! Tauri backend for the Cuaderno desktop app.
//!
//! A thin shell in the same sense as the CLI and MCP crates: commands
//! parse arguments, stamp "today", and call `cdno-domain` methods
//! directly — domain types serialise over the IPC bridge as-is, no
//! DTO layer (plan §3.5). The interesting machinery is the live-
//! refresh pipeline: `FsFileWatcher` (cdno-core) feeds a dedicated
//! watcher thread that reconciles the index and emits `vault:changed`
//! area events the frontend maps to query invalidations.

pub mod commands;
pub mod error;
pub mod events;
pub mod state;
pub mod watcher;
pub mod with_vault;

mod clock;

use std::sync::Arc;

use tauri::Manager;

use cdno_domain::bootstrap::open_vault;

use crate::state::{AppState, WriteJournal};
use crate::watcher::WatcherDeps;

/// Environment variable naming the vault root — same contract as the
/// CLI and the MCP binaries. Upward discovery and a persisted app
/// setting are later polish; for now a missing/invalid vault is a
/// hard, explicit startup error.
const ENV_VAULT_PATH: &str = "CUADERNO_VAULT_PATH";

/// Build and run the app. Called from `main`.
///
/// # Panics
///
/// Panics (with a readable message) when the vault cannot be opened —
/// there is nothing sensible to render without one, and Tauri's setup
/// hook has no error channel to the user beyond aborting.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cdno_tauri=info".into()),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let root = std::env::var(ENV_VAULT_PATH).map_err(|_| {
                format!("{ENV_VAULT_PATH} is not set — point it at your vault root")
            })?;
            let root = std::path::PathBuf::from(root);

            // Startup reconciliation runs inside open_vault, before
            // any window content loads — the correctness backstop the
            // watcher's incremental repairs lean on.
            let opened = open_vault(&root)?;
            if opened.report.added + opened.report.updated + opened.report.removed > 0 {
                tracing::info!(
                    added = opened.report.added,
                    updated = opened.report.updated,
                    removed = opened.report.removed,
                    "startup reconciliation applied changes",
                );
            }

            app.manage(AppState {
                vault: Arc::new(opened.vault),
                journal: WriteJournal::default(),
            });

            // Live updates: FsFileWatcher -> mpsc -> watcher thread ->
            // reconcile + emit. The watcher handle must outlive setup,
            // so it is managed as state (dropping it would stop the
            // platform watcher).
            let (tx, rx) = std::sync::mpsc::channel();
            let mut fs_watcher = cdno_core::watcher::FsFileWatcher::new(&root);
            cdno_core::watcher::FileWatcher::watch(&mut fs_watcher, tx)?;
            app.manage(std::sync::Mutex::new(fs_watcher));

            let deps = WatcherDeps {
                store: opened.store,
                index: opened.index,
                ignore: opened.ignore,
            };
            let watcher_app = app.handle().clone();
            std::thread::spawn(move || watcher::run(watcher_app, deps, rx));

            let clock_app = app.handle().clone();
            std::thread::spawn(move || clock::run(clock_app));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::orientation::get_orientation,
            commands::orientation::get_today,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the cuaderno app");
}
