// The "show metrics" preference (plan §3.11, user amendment
// 2026-07-07): progress bars and quantitative graphics are hidden by
// default — calm is the default posture — but available for the
// moments they help. Persisted in localStorage; every metric surface
// renders nothing (not an empty frame) when this is off.
import { useSyncExternalStore } from "react";

const STORAGE_KEY = "cuaderno-show-metrics";
const listeners = new Set<() => void>();

function read(): boolean {
  // Defensive: a broken/absent storage (test environments, exotic
  // webviews) means "default off", never a crash — this is a
  // preference, not data.
  try {
    return globalThis.localStorage?.getItem(STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

export function setShowMetrics(on: boolean): void {
  try {
    if (on) {
      globalThis.localStorage?.setItem(STORAGE_KEY, "true");
    } else {
      globalThis.localStorage?.removeItem(STORAGE_KEY);
    }
  } catch {
    // Preference persistence is best-effort.
  }
  listeners.forEach((notify) => notify());
}

export function toggleMetrics(): void {
  setShowMetrics(!read());
}

/** Reactive read of the preference. */
export function useMetrics(): boolean {
  return useSyncExternalStore(
    (notify) => {
      listeners.add(notify);
      return () => listeners.delete(notify);
    },
    read,
    () => false,
  );
}
