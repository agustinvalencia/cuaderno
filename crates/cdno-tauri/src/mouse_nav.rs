//! macOS bridge for the mouse back/forward side buttons.
//!
//! WKWebView on macOS does not surface the X1/X2 side buttons to web
//! content as DOM mouse events (tauri#10936), so the frontend can't see
//! them. A local `NSEvent` monitor catches `otherMouseUp` for
//! `buttonNumber` 3 (back) / 4 (forward) and emits a nav event the webview
//! turns into history navigation. A *local* monitor only observes events
//! delivered to this app (no Accessibility permission) and runs on the
//! main thread, where AppKit dispatches events.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2_app_kit::{NSEvent, NSEventMask};
use tauri::{AppHandle, Emitter};

/// Emitted for the back side button (X1). The webview maps it to
/// `navigate(-1)` (see `ui/src/shell/useHistoryNavigation.ts`).
pub const NAV_BACK: &str = "nav://back";
/// Emitted for the forward side button (X2) → `navigate(1)`.
pub const NAV_FORWARD: &str = "nav://forward";

// NSEvent buttonNumber for the side buttons: 3 = back (X1), 4 = forward (X2).
const BUTTON_BACK: isize = 3;
const BUTTON_FORWARD: isize = 4;

/// Install the local NSEvent monitor. Call once from `setup`, on the main
/// thread.
pub fn install(app: AppHandle) {
    let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit passes a live NSEvent for the duration of the call.
        let button = unsafe { event.as_ref().buttonNumber() };
        match button {
            BUTTON_BACK => {
                let _ = app.emit(NAV_BACK, ());
            }
            BUTTON_FORWARD => {
                let _ = app.emit(NAV_FORWARD, ());
            }
            _ => {}
        }
        // Return the event unchanged so normal handling continues.
        event.as_ptr()
    });

    // SAFETY: AppKit class method; must run on the main thread, which the
    // Tauri `setup` closure does.
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::OtherMouseUp, &handler)
    };

    // The monitor fires only while its token is retained; it lives for the
    // whole app run (installed once at startup), so leak the token rather
    // than track it — dropping it would silently stop the monitor.
    std::mem::forget(monitor);
}
