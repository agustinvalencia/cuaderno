//! Menu-bar tray icon (M10, plan §1.0): Quick capture / Open / Quit.
//!
//! Deliberately minimal and calm — no status text, no unread counts,
//! no dynamic icon swaps. The tray exists so the two whole-app verbs
//! (summon the capture window, surface the main window) stay reachable
//! when every window is closed or hidden. Failures here log a warning
//! and never abort startup: the app is fully usable without a tray.

use tauri::menu::{MenuBuilder, MenuEvent};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Runtime};

use crate::{summon_capture, surface_window};

/// Stable ids for the menu items; matched in [`on_menu_event`].
const ID_CAPTURE: &str = "quick-capture";
const ID_OPEN: &str = "open-main";
const ID_QUIT: &str = "quit";

/// Build and register the tray. The returned handle can be dropped —
/// Tauri keeps registered tray icons alive in the app's own state.
pub fn init<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text(ID_CAPTURE, "Quick capture")
        .text(ID_OPEN, "Open cuaderno")
        .separator()
        .text(ID_QUIT, "Quit")
        .build()?;

    let mut builder = TrayIconBuilder::with_id("cuaderno-tray")
        .tooltip("cuaderno")
        .menu(&menu)
        // Left click opens the same menu — the tray carries no
        // primary action of its own, so both buttons behave alike.
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event);

    // The codegen-embedded window icon doubles as the tray icon. It is
    // present on every desktop target (tauri-codegen embeds icons/
    // icon.png for Unix, .ico for Windows), but if it ever goes
    // missing fall back to a text-only tray rather than an invisible
    // one.
    match app.default_window_icon() {
        Some(icon) => builder = builder.icon(icon.clone()),
        None => {
            tracing::warn!("no embedded app icon; tray falls back to a title");
            builder = builder.title("cuaderno");
        }
    }

    builder.build(app)?;
    Ok(())
}

fn on_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        // Same behaviour as the global shortcut: show + focus the
        // capture window, then `capture:show` so its input refocuses.
        ID_CAPTURE => summon_capture(app),
        ID_OPEN => surface_window(app, "main"),
        ID_QUIT => app.exit(0),
        other => tracing::debug!(id = other, "unhandled tray menu event"),
    }
}
