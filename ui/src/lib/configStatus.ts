// Config health as a tiny module store (same useSyncExternalStore
// pattern as lib/watcherStatus.ts). The backend emits `config:status`
// after an external `.cuaderno/config.toml` edit: `health: "invalid"`
// with a `message` when the on-disk config failed to open (the app kept
// the last good config), `health: "deferred"` when a busy vault kept an
// otherwise-fine config from applying (#384), or `health: "valid"` to
// clear a prior notice (GH #365 PR4). The event bridge writes it here,
// and the shell reads it to show a non-red banner. A module store (not
// React state) because the writer is the pre-render event bridge, which
// has no component to live in.

import { useSyncExternalStore } from "react";

import type { ConfigHealth } from "../api/bindings/ConfigHealth";

export interface ConfigStatusState {
  health: ConfigHealth;
  message: string | null;
}

// A stable default reference for the initial and server snapshots, so
// useSyncExternalStore never sees a fresh object it mistakes for a change.
const DEFAULT: ConfigStatusState = { health: "valid", message: null };

let state: ConfigStatusState = DEFAULT;
const listeners = new Set<() => void>();

export function setConfigStatus(next: ConfigStatusState): void {
  if (next.health === state.health && next.message === state.message) return;
  state = next;
  listeners.forEach((notify) => notify());
}

export function getConfigStatus(): ConfigStatusState {
  return state;
}

/** Reactive read of the config health. */
export function useConfigStatus(): ConfigStatusState {
  return useSyncExternalStore(
    (notify) => {
      listeners.add(notify);
      return () => listeners.delete(notify);
    },
    getConfigStatus,
    // Server snapshot: assume valid — the banner is an error notice,
    // never a default.
    () => DEFAULT,
  );
}
