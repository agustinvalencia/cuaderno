//! macOS bridge for the mouse back/forward side buttons.
//!
//! WKWebView on macOS does not surface the side buttons to web content as
//! DOM events (tauri#10936), so the frontend can't see them. A local
//! `NSEvent` monitor catches them and emits a nav event the webview turns
//! into history navigation. A *local* monitor only observes events
//! delivered to this app (no Accessibility permission) and runs on the
//! main thread, where AppKit dispatches events.
//!
//! Two delivery mechanisms, because macOS mice expose the side buttons
//! either way depending on the mouse / its System Settings / driver:
//!  - **Swipe gestures** (`NSEventTypeSwipe`): the common default — the
//!    button fires a horizontal swipe. `deltaX == +1` is back (swipe left),
//!    `-1` is forward (swipe right), matching AppKit's convention and Safari.
//!    (Verified against a real mouse, 2026-07-12.) This path also gives
//!    trackpad two/three-finger horizontal swipes the same navigation.
//!  - **Other-mouse buttons** (`otherMouseUp`): mice that expose the side
//!    buttons as real buttons report `buttonNumber` 3 (back) / 4 (forward).

use std::ptr::NonNull;

use block2::RcBlock;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
use tauri::{AppHandle, Emitter};

/// Emitted for "back". The webview maps it to `navigate(-1)` (see
/// `ui/src/shell/useHistoryNavigation.ts`).
pub const NAV_BACK: &str = "nav://back";
/// Emitted for "forward" → `navigate(1)`.
pub const NAV_FORWARD: &str = "nav://forward";

// otherMouse buttonNumber for the side buttons: 3 = back (X1), 4 = forward (X2).
const BUTTON_BACK: isize = 3;
const BUTTON_FORWARD: isize = 4;

// A committed horizontal swipe reports deltaX ±1; a companion phase event
// reports 0. Threshold well above 0 to act only on the real swipe.
const SWIPE_THRESHOLD: f64 = 0.5;

/// Install the local NSEvent monitor. Call once from `setup`, on the main
/// thread.
pub fn install(app: AppHandle) {
    let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit passes a live NSEvent for the duration of the call.
        let ev = unsafe { event.as_ref() };
        let nav = match ev.r#type() {
            // A discrete "swipe between pages" gesture: the mapped mouse side
            // button, or a trackpad page-swipe. This is NOT two-finger
            // scrolling (that's NSEventTypeScrollWheel, not in the mask), so
            // panning a wide table/code block doesn't navigate. Direction is
            // in deltaX.
            NSEventType::Swipe => {
                let dx = ev.deltaX();
                if dx > SWIPE_THRESHOLD {
                    Some(NAV_BACK)
                } else if dx < -SWIPE_THRESHOLD {
                    Some(NAV_FORWARD)
                } else {
                    None // the companion deltaX == 0 phase event
                }
            }
            // A real side button: back = 3, forward = 4.
            _ => match ev.buttonNumber() {
                BUTTON_BACK => Some(NAV_BACK),
                BUTTON_FORWARD => Some(NAV_FORWARD),
                _ => None,
            },
        };
        if let Some(name) = nav {
            let _ = app.emit(name, ());
        }
        // Return the event unchanged so normal handling continues.
        event.as_ptr()
    });

    // SAFETY: AppKit class method; must run on the main thread, which the
    // Tauri `setup` closure does.
    let mask = NSEventMask::OtherMouseUp | NSEventMask::Swipe;
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) };

    // The monitor fires only while its token is retained; it lives for the
    // whole app run (installed once at startup), so leak the token rather
    // than track it — dropping it would silently stop the monitor.
    std::mem::forget(monitor);
}
