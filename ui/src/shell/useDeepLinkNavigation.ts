// Open a note when a `cuaderno://note/<path>` deep link fires (user request
// 2026-07-13). The Rust deep-link handler (crates/cdno-tauri/src/deeplink.rs)
// parses the URL and emits the vault path as `deeplink:open-note`; here we
// navigate the reader to `/note/<path>`. The path is not trusted — it flows
// through the reader's read_note → VaultPath guard, which rejects `..`/absolute
// paths, so a malicious deep link can't read outside the vault.
import { useEffect, useRef } from "react";
import { useNavigate } from "react-router";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

export function useDeepLinkNavigation() {
  const navigate = useNavigate();
  // `useNavigate`'s identity changes on every navigation under BrowserRouter,
  // so read it through a ref and subscribe once (`[]` deps, like
  // `useHistoryNavigation`) rather than re-registering the listener per route.
  const navigateRef = useRef(navigate);
  navigateRef.current = navigate;

  useEffect(() => {
    // Cold start: the app was launched *by* a deep link, so the Rust handler
    // fired before this listener existed and buffered the path — drain it once
    // on mount. (A warm link, arriving while the app runs, rides the event
    // below instead; the buffer it also sets is never re-read.)
    void invoke<string | null>("take_pending_deeplink")
      .then((path) => {
        if (path) navigateRef.current(`/note/${path}`);
      })
      .catch(() => {});

    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<string>("deeplink:open-note", (event) => {
      const path = event.payload;
      if (path) navigateRef.current(`/note/${path}`);
    })
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
