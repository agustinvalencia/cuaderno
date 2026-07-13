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

use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_deep_link::DeepLinkExt;

use crate::events::OPEN_NOTE_DEEPLINK;

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
