//! `cuaderno://note/<vault-path>` deep links → open that note in the centred
//! reader (user request 2026-07-13). The scheme is registered by
//! `tauri-plugin-deep-link` (config in `tauri.conf.json`); this module parses
//! an opened URL and, for a `note` link, surfaces the app and emits the vault
//! path to the frontend to navigate the reader there.
//!
//! The path is NOT trusted here: it flows through the reader's `read_note` →
//! `VaultPath` guard, which rejects any absolute path or `..` component, so a
//! `cuaderno://note/../../etc/passwd` link cannot read outside the vault —
//! exactly like any other reader navigation.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_deep_link::DeepLinkExt;

use crate::events::OPEN_NOTE_DEEPLINK;

/// Buffers a deep-link note path that arrived before the frontend could
/// receive it — the cold-start case, where the OS launches the app *by* the
/// `cuaderno://note/...` URL, so `on_open_url` fires at the start of the run
/// loop, hundreds of ms before React mounts a listener. Tauri doesn't queue
/// webview events for listeners that aren't registered yet, so a plain emit
/// would be dropped. The frontend drains this once on mount via
/// [`take_pending_deeplink`]; links opened while the app is already running
/// (a listener is mounted) ride the emitted event instead.
#[derive(Default)]
pub struct PendingDeepLink(Mutex<Option<String>>);

/// Return and clear any note path buffered before the frontend mounted.
/// Called once when the reader's deep-link hook first runs.
#[tauri::command]
pub fn take_pending_deeplink(pending: State<'_, PendingDeepLink>) -> Option<String> {
    pending.0.lock().ok().and_then(|mut slot| slot.take())
}

/// Extract the vault path from a `cuaderno://note/<path>` deep link, or `None`
/// for any other URL (wrong scheme/host, or an empty path). Vault paths are
/// URL-safe slug segments, so no percent-decoding is needed; anything
/// malformed is caught downstream by the reader's `VaultPath` guard.
pub fn note_path_from_deeplink(url: &str) -> Option<String> {
    let rest = url.strip_prefix("cuaderno://note/")?;
    // Drop any query/fragment a URL might carry, and a trailing slash.
    let path = rest.split(['?', '#']).next().unwrap_or(rest);
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return None;
    }
    Some(path.to_owned())
}

/// Register the deep-link handler. On a `cuaderno://note/<path>` open — whether
/// it launched the app cold (the plugin buffers the launch URL) or arrived
/// while running — surface the main window and emit the path for the reader.
pub(crate) fn install<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            let Some(path) = note_path_from_deeplink(url.as_str()) else {
                continue;
            };
            // Buffer for a not-yet-ready frontend (cold start) AND emit for a
            // ready one (warm). On cold start the emit lands with no listener
            // and is dropped, so the frontend's mount-time drain is what opens
            // the note; on a warm link the buffer is set but never re-read
            // (the drain runs once), and the event does the work.
            if let Some(pending) = handle.try_state::<PendingDeepLink>()
                && let Ok(mut slot) = pending.0.lock()
            {
                *slot = Some(path.clone());
            }
            surface_and_open(&handle, path);
        }
    });
}

/// Focus the app and hand the reader the path to open.
fn surface_and_open<R: Runtime>(app: &AppHandle<R>, path: String) {
    crate::surface_window(app, "main");
    if let Err(err) = app.emit(OPEN_NOTE_DEEPLINK, path) {
        tracing::warn!(error = %err, "failed to emit deeplink open-note");
    }
}
