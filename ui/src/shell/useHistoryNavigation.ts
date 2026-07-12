// Back/forward navigation from the mouse side buttons, the keyboard, and
// (on macOS) a native bridge (user request 2026-07-12). Three input paths
// funnel to react-router history:
//
//  - DOM mouse side buttons (X1/X2 = MouseEvent.button 3/4) — works where
//    the webview delivers them (Windows/WebView2). macOS WKWebView does
//    NOT surface these as DOM events (tauri#10936), which is why the native
//    bridge below exists.
//  - Keyboard Cmd/Ctrl+[ (back) and Cmd/Ctrl+] (forward) — the reliable
//    path everywhere, matching the Finder/browser convention. Ignored while
//    typing in a field so it can't hijack an editor.
//  - Tauri `nav://back` / `nav://forward` events — emitted by the native
//    macOS NSEvent monitor (crates/cdno-tauri/src/mouse_nav.rs) so the
//    physical side buttons work on macOS too.
import { useEffect, useRef } from "react";
import { useNavigate } from "react-router";
import { listen } from "@tauri-apps/api/event";

const BACK_BUTTON = 3;
const FORWARD_BUTTON = 4;

/** True when the event originated in a text-editing surface, where the
 * keyboard shortcut must not steal the keystroke. */
function isEditable(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.tagName === "INPUT" ||
      target.tagName === "TEXTAREA" ||
      target.isContentEditable)
  );
}

export function useHistoryNavigation() {
  const navigate = useNavigate();
  // `useNavigate`'s identity changes on every navigation under BrowserRouter,
  // so read it through a ref and attach the listeners once (`[]` deps, like
  // the ⌘K handler) — re-running the effect per route change would needlessly
  // re-register the native bridge and briefly detach it.
  const navigateRef = useRef(navigate);
  navigateRef.current = navigate;

  useEffect(() => {
    const back = () => navigateRef.current(-1);
    const forward = () => navigateRef.current(1);

    // Swallow the side buttons' mousedown so a webview that DOES handle
    // them natively can't also run its own step (a double navigation).
    function suppressMouse(event: MouseEvent) {
      if (event.button === BACK_BUTTON || event.button === FORWARD_BUTTON) {
        event.preventDefault();
      }
    }
    function onMouseUp(event: MouseEvent) {
      if (event.button === BACK_BUTTON) {
        event.preventDefault();
        back();
      } else if (event.button === FORWARD_BUTTON) {
        event.preventDefault();
        forward();
      }
    }
    function onKeyDown(event: KeyboardEvent) {
      if (!(event.metaKey || event.ctrlKey) || isEditable(event.target)) return;
      if (event.key === "[") {
        event.preventDefault();
        back();
      } else if (event.key === "]") {
        event.preventDefault();
        forward();
      }
    }
    window.addEventListener("mousedown", suppressMouse);
    window.addEventListener("mouseup", onMouseUp);
    window.addEventListener("keydown", onKeyDown);

    // The native macOS bridge. `listen` is async and rejects outside a
    // Tauri webview (e.g. jsdom tests) — swallow that; detach on unmount,
    // handling the resolve-after-unmount race (mirrors CaptureBar).
    let cancelled = false;
    let unlistenBack: (() => void) | undefined;
    let unlistenForward: (() => void) | undefined;
    void listen("nav://back", back)
      .then((fn) => (cancelled ? fn() : (unlistenBack = fn)))
      .catch(() => {});
    void listen("nav://forward", forward)
      .then((fn) => (cancelled ? fn() : (unlistenForward = fn)))
      .catch(() => {});

    return () => {
      window.removeEventListener("mousedown", suppressMouse);
      window.removeEventListener("mouseup", onMouseUp);
      window.removeEventListener("keydown", onKeyDown);
      cancelled = true;
      unlistenBack?.();
      unlistenForward?.();
    };
  }, []);
}
