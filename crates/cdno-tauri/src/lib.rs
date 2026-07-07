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

use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use cdno_domain::bootstrap::open_vault;

use crate::events::CAPTURE_SHOW;
use crate::state::{AppState, WriteJournal};
use crate::watcher::WatcherDeps;

/// Bring `label`'s window to the front (show + focus), tolerating a
/// missing window. Used both by the single-instance guard (main) and
/// the global capture shortcut (capture).
fn surface_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

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
        // Single-instance MUST be registered first (the plugin's own
        // requirement): a second launch is intercepted here and focuses
        // the already-running main window instead of spawning a
        // duplicate app.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            surface_window(app, "main");
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
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
                root: root.clone(),
            });

            // Global capture hotkey (⌘⇧C on macOS; SUPER maps to Cmd).
            // Registered from Rust — JS registration is racy (plan
            // §3.6). SUPER+SHIFT+C summons the floating capture window:
            // show + focus, then emit `capture:show` so the window's
            // input re-focuses even when it was already visible.
            let capture_shortcut =
                Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyC);
            let registration =
                app.global_shortcut()
                    .on_shortcut(capture_shortcut, |app, _shortcut, event| {
                        // The handler fires on both press and release —
                        // act once, on press.
                        if event.state != ShortcutState::Pressed {
                            return;
                        }
                        surface_window(app, "capture");
                        if let Err(err) = app.emit(CAPTURE_SHOW, ()) {
                            tracing::warn!(error = %err, "failed to emit capture:show");
                        }
                    });
            // A failed registration (the OS refused the hotkey, or it
            // clashes with another app) must not abort startup — the
            // app is fully usable without the global shortcut. Surfacing
            // it as a toast is later polish (plan §3.6); the log
            // suffices for now.
            if let Err(err) = registration {
                tracing::warn!(error = %err, "failed to register the global capture shortcut");
            }

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
            commands::actions::start_action,
            commands::actions::complete_action,
            commands::actions::add_action,
            commands::actions::promote_action,
            commands::actions::list_all_actions,
            commands::projects::update_project_state,
            commands::projects::get_project,
            commands::projects::add_waiting_on,
            commands::projects::resolve_waiting,
            commands::projects::park_project,
            commands::projects::activate_project,
            commands::notes::read_note,
            commands::notes::resolve_wikilink,
            commands::search::search_vault,
            commands::commitments::get_commitments,
            commands::commitments::complete_commitment,
            commands::commitments::complete_milestone,
            commands::weekly::get_weekly_bundle,
            commands::weekly::save_weekly_section,
            commands::capture::capture_quick,
            commands::capture::log_quick,
            commands::capture::list_inbox,
            commands::capture::discard_inbox_item,
            commands::capture::open_in_editor,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the cuaderno app");
}
