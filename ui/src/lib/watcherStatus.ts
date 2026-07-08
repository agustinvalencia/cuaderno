// Watcher health as a tiny module store (same useSyncExternalStore
// pattern as lib/metrics.ts). The backend emits `watcher:status`
// {state: ok|degraded} with every debounced batch (plan §3.1); the
// event bridge writes it here, and the shell reads it to show the
// grey "live updates paused" pill and to run the 60s poll fallback
// while degraded. A module store (not React state in main.tsx)
// because the writer is the pre-render event bridge, which has no
// component to live in.

import { useSyncExternalStore } from "react";

export type WatcherState = "ok" | "degraded";

let state: WatcherState = "ok";
const listeners = new Set<() => void>();

export function setWatcherState(next: WatcherState): void {
  if (next === state) return;
  state = next;
  listeners.forEach((notify) => notify());
}

export function getWatcherState(): WatcherState {
  return state;
}

/** Reactive read of the watcher health. */
export function useWatcherState(): WatcherState {
  return useSyncExternalStore(
    (notify) => {
      listeners.add(notify);
      return () => listeners.delete(notify);
    },
    getWatcherState,
    // Server snapshot: assume healthy — the pill is a degradation
    // notice, never a default.
    () => "ok",
  );
}
