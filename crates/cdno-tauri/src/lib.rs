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
pub mod vault_locator;
pub mod watcher;
pub mod with_vault;

mod clock;
mod tray;

use std::path::PathBuf;
use std::sync::Arc;

use arc_swap::ArcSwap;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use cdno_domain::bootstrap::{BootstrapError, open_vault};

use crate::events::CAPTURE_SHOW;
use crate::state::{AppState, WriteJournal};
use crate::vault_locator::Resolution;
use crate::watcher::WatcherDeps;

/// Bring `label`'s window to the front (show + focus), tolerating a
/// missing window. Used by the single-instance guard (main), the
/// global capture shortcut (capture), and the tray menu (both).
fn surface_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Summon the floating capture window: show + focus, then emit
/// `capture:show` so the window's input re-focuses even when it was
/// already visible. Shared by the global shortcut and the tray's
/// "Quick capture" item so the two entry points cannot drift.
fn summon_capture<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    surface_window(app, "capture");
    if let Err(err) = app.emit(CAPTURE_SHOW, ()) {
        tracing::warn!(error = %err, "failed to emit capture:show");
    }
}

/// Open the vault at `root` and wire everything that depends on it:
/// the managed [`AppState`], the global capture shortcut, the
/// live-refresh watcher thread, the vault clock, and the tray. Called
/// exactly once per launch — synchronously from `setup` when the vault
/// resolved from the env var or the persisted setting, or from the main
/// thread once the first-launch picker returns a vault.
///
/// Takes an `&AppHandle` (not the `setup` `&App`) so both call sites can
/// share it; every capability it uses — `manage`, `global_shortcut`,
/// tray, window handles — is available on the handle.
fn init_with_vault(
    app: &tauri::AppHandle,
    root: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    // Startup reconciliation runs inside open_vault, before any window
    // content leans on the index — the correctness backstop the
    // watcher's incremental repairs build on.
    let opened = open_vault(&root)?;
    if opened.report.added + opened.report.updated + opened.report.removed > 0 {
        tracing::info!(
            added = opened.report.added,
            updated = opened.report.updated,
            removed = opened.report.removed,
            "startup reconciliation applied changes",
        );
    }

    // The store and index Arcs are shared three ways: the live vault owns
    // one clone, the watcher thread's deps another (for its reconcile
    // pass), and AppState retains a third so a config reload can rebuild
    // the vault on the same handles — no SQLite reopen (GH #365).
    //
    // The ignore matcher is a single `ArcSwap` shared by reference between
    // AppState and the watcher deps: a config reload swaps a fresh set in
    // via AppState and the watcher's next reconcile loads it (GH #365 PR4).
    let ignore = Arc::new(ArcSwap::from(opened.ignore));
    app.manage(AppState {
        vault: ArcSwap::from_pointee(opened.vault),
        store: opened.store.clone(),
        index: opened.index.clone(),
        ignore: ignore.clone(),
        journal: WriteJournal::default(),
        root: root.clone(),
    });

    // Global capture hotkey (⌘⇧C on macOS; SUPER maps to Cmd).
    // Registered from Rust — JS registration is racy (plan §3.6).
    // SUPER+SHIFT+C summons the floating capture window: show + focus,
    // then emit `capture:show` so the window's input re-focuses even
    // when it was already visible.
    let capture_shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyC);
    let registration =
        app.global_shortcut()
            .on_shortcut(capture_shortcut, |app, _shortcut, event| {
                // The handler fires on both press and release — act once, on
                // press.
                if event.state != ShortcutState::Pressed {
                    return;
                }
                summon_capture(app);
            });
    // A failed registration (the OS refused the hotkey, or it clashes
    // with another app) must not abort startup — the app is fully usable
    // without the global shortcut. Surfacing it as a toast is later
    // polish (plan §3.6); the log suffices for now.
    if let Err(err) = registration {
        tracing::warn!(error = %err, "failed to register the global capture shortcut");
    }

    // Live updates: FsFileWatcher -> mpsc -> watcher thread -> reconcile
    // + emit. The watcher handle must outlive setup, so it is managed as
    // state (dropping it would stop the platform watcher).
    let (tx, rx) = std::sync::mpsc::channel();
    let mut fs_watcher = cdno_core::watcher::FsFileWatcher::new(&root);
    cdno_core::watcher::FileWatcher::watch(&mut fs_watcher, tx)?;
    app.manage(std::sync::Mutex::new(fs_watcher));

    let deps = WatcherDeps {
        store: opened.store,
        index: opened.index,
        ignore,
    };
    let watcher_app = app.clone();
    std::thread::spawn(move || watcher::run(watcher_app, deps, rx));

    let clock_app = app.clone();
    std::thread::spawn(move || clock::run(clock_app));

    // Menu-bar tray (M10). A tray failure is cosmetic — log and carry
    // on, never crash startup over it.
    if let Err(err) = tray::init(app) {
        tracing::warn!(error = %err, "failed to create the tray icon");
    }

    // Reveal the main window. It starts hidden (tauri.conf.json main
    // window `visible: false`) so its webview can load — and, on the
    // first-launch picker path, flash its no-vault error — off-screen while
    // the vault is still being resolved. Now that state is managed and the
    // vault is open, show it. A show/focus failure is cosmetic (the window
    // is only hidden), so log and carry on rather than abort.
    if let Some(window) = app.get_webview_window("main") {
        if let Err(err) = window.show() {
            tracing::warn!(error = %err, "failed to show the main window");
        }
        if let Err(err) = window.set_focus() {
            tracing::warn!(error = %err, "failed to focus the main window");
        }
    } else {
        tracing::warn!("main window missing; cannot reveal it after init");
    }

    Ok(())
}

/// Spawn the first-launch picker on a background thread. Shared by the
/// `NeedsPicker` arm and the `Stored`-open-failure fall-through so the two
/// cannot drift. The thread must be background: `run_picker`'s blocking
/// dialogs post onto the main event loop, which is not pumping until
/// `setup` returns — the thread's first dialog simply blocks until then,
/// then the live loop services it.
fn spawn_picker(app: &tauri::AppHandle, config_dir: PathBuf) {
    let picker_app = app.clone();
    std::thread::spawn(move || run_picker(picker_app, config_dir));
}

/// First-launch folder-picker loop, run on a background thread (see the
/// call site for why it must not touch the main thread). Loops until the
/// user picks a folder that opens as a vault, then persists it and
/// finishes initialisation on the main thread; on cancel it explains
/// itself and exits cleanly rather than aborting to Console.app.
///
/// Validation calls `open_vault` and matches [`BootstrapError::NotAVault`]
/// so the "no `.cuaderno/`" message is precise. The proven-good path is
/// re-opened by [`init_with_vault`] on the main thread; the extra open
/// (one reconciliation pass, first launch only) is the price of keeping
/// every dialog off the main thread, where a blocking dialog deadlocks.
fn run_picker(app: tauri::AppHandle, config_dir: PathBuf) {
    loop {
        let picked = app
            .dialog()
            .file()
            .set_title("Choose your cuaderno vault")
            .blocking_pick_folder();

        let Some(folder) = picked.and_then(|p| p.into_path().ok()) else {
            // Cancelled. A calm explanation, then a graceful exit — never
            // the old silent, Console-only abort.
            app.dialog()
                .message(
                    "cuaderno needs a vault to open.\n\nRun `cdno init <path>` to create one, \
                     then relaunch the app.",
                )
                .blocking_show();
            std::process::exit(0);
        };

        match open_vault(&folder) {
            Ok(_) => {
                // Proven vault (the opened handles are dropped here; the
                // main thread re-opens them). Persist best-effort — a
                // failed write only costs the user a second pick next
                // launch, never a crash.
                if let Err(err) = vault_locator::write_setting(&config_dir, &folder) {
                    tracing::warn!(error = %err, "could not persist the chosen vault path");
                }
                let init_app = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Err(err) = init_with_vault(&init_app, folder) {
                        // The same path opened moments ago on this thread;
                        // a failure now is a genuine surprise. A blocking
                        // dialog here would deadlock the main thread, so
                        // log and abort loudly instead.
                        tracing::error!(error = %err, "failed to open the picked vault");
                        std::process::exit(1);
                    }
                    // The main window's webview loaded and fired its first
                    // queries before the vault existed — they failed (no
                    // managed state). Reload once so they refetch against
                    // the now-ready vault. First launch only.
                    if let Some(window) = init_app.get_webview_window("main") {
                        let _ = window.eval("window.location.reload()");
                    }
                });
                return;
            }
            Err(BootstrapError::NotAVault { .. }) => {
                app.dialog()
                    .message(
                        "That folder isn't a cuaderno vault (no .cuaderno/ found).\n\nPick the \
                         vault root, or run `cdno init` first.",
                    )
                    .blocking_show();
                // Re-open the picker.
            }
            Err(err) => {
                app.dialog()
                    .message(format!("Could not open that vault:\n\n{err}"))
                    .blocking_show();
                // Re-open the picker.
            }
        }
    }
}

/// Build and run the app. Called from `main`.
///
/// # Panics
///
/// Panics (with a readable message) only when an explicit
/// `CUADERNO_VAULT_PATH` override points at something that will not open
/// as a vault — an explicit override must fail loudly. A missing
/// override, or a persisted path that no longer opens, is no longer
/// fatal: the app falls back to a native folder picker, and a cancelled
/// picker exits cleanly (never a silent abort).
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
            // Surface the already-running instance's main window. If that
            // first instance is still pre-init (vault not yet picked, so the
            // main window is hidden and no state is managed), this reveals
            // the loading/error shell rather than a ready UI — acceptable:
            // the user asked to focus the app, and init's own show/focus
            // will replace the shell once the picker returns.
            surface_window(app, "main");
        }))
        .plugin(tauri_plugin_opener::init())
        // Native folder picker + message dialogs for first-launch vault
        // discovery (GH #331). Invoked only from Rust, so — like the
        // opener plugin — it needs no window capability entry.
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // Vault resolution: explicit env override, then a persisted
            // setting, then a native picker (GH #331). The persisted
            // path is validated with the same `.cuaderno/` marker check
            // open_vault uses for NotAVault, so a moved/deleted vault
            // falls through to the picker instead of crashing.
            let config_dir = app.path().app_config_dir().unwrap_or_else(|err| {
                tracing::warn!(error = %err, "no app config dir; the vault path won't persist");
                PathBuf::new()
            });
            let resolution = vault_locator::resolve(
                vault_locator::resolve_from_env(),
                &config_dir,
                |candidate| candidate.join(cdno_core::paths::CUADERNO_DIR).is_dir(),
            );

            match resolution {
                // Explicit override: open synchronously so the window loads
                // with state already managed. A failure propagates via `?`
                // as a hard startup error — an explicit override must fail
                // loudly, never fall through.
                Resolution::Env(root) => {
                    init_with_vault(app.handle(), root)?;
                }
                // Persisted path: also opened synchronously on the main
                // thread (the event loop is not pumping yet, so this stays
                // on the main thread — only the error branch below changes).
                // The path already passed the cheap `.cuaderno/` marker
                // check in `resolve`, but the full open can still fail —
                // corrupt config.toml, an unopenable index, or a TOCTOU
                // delete between check and open. That must NOT abort to
                // Console.app, the silent death #331 removes: warn (naming
                // the error) and fall through to the picker, exactly as
                // `NeedsPicker` does.
                Resolution::Stored(root) => {
                    if let Err(err) = init_with_vault(app.handle(), root) {
                        tracing::warn!(
                            error = %err,
                            "stored vault path failed to open; falling back to the picker",
                        );
                        spawn_picker(app.handle(), config_dir);
                    }
                }
                // No usable vault: prompt. The picker runs on a background
                // thread (see `spawn_picker`) because its blocking dialogs
                // would deadlock the not-yet-pumping main event loop.
                Resolution::NeedsPicker => {
                    spawn_picker(app.handle(), config_dir);
                }
            }

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
            commands::stewardships::list_stewardships,
            commands::stewardships::get_stewardship_detail,
            commands::stewardships::get_tracking_template_fields,
            commands::stewardships::log_tracking_entry,
            commands::portfolios::list_portfolios,
            commands::portfolios::get_portfolio,
            commands::portfolios::add_evidence,
            commands::strategic::get_strategic_bundle,
            commands::calendar::read_daily,
            commands::calendar::read_weekly,
            commands::calendar::read_monthly,
            commands::calendar::list_daily_dates,
            commands::templates::list_templates,
            commands::templates::read_template,
            commands::templates::list_template_placeholders,
            commands::templates::save_template,
            commands::templates::create_template,
            commands::config::read_config,
            commands::config::read_config_model,
            commands::config::parse_config_model,
            commands::config::validate_config,
            commands::config::save_config,
            commands::config::reload_config,
            commands::config::config_set_note_type,
            commands::config::config_remove_note_type,
            commands::config::config_set_schema_field,
            commands::config::config_remove_schema_field,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the cuaderno app");
}
