// The log-card ordering preference (user request 2026-07-12). Daily and
// project log cards read oldest-first (chronological, as written) by
// default; a reader can flip to newest-first. Persisted in localStorage,
// defensively — a preference, never data — like the theme/metrics stores,
// so the choice is remembered and consistent across every log surface.
import { useSyncExternalStore } from "react";

export type LogOrder = "oldest" | "newest";

const STORAGE_KEY = "cuaderno-log-order";
const listeners = new Set<() => void>();

function read(): LogOrder {
  try {
    return globalThis.localStorage?.getItem(STORAGE_KEY) === "newest"
      ? "newest"
      : "oldest";
  } catch {
    return "oldest";
  }
}

export function setLogOrder(order: LogOrder): void {
  try {
    if (order === "newest") {
      globalThis.localStorage?.setItem(STORAGE_KEY, "newest");
    } else {
      globalThis.localStorage?.removeItem(STORAGE_KEY);
    }
  } catch {
    // Preference persistence is best-effort.
  }
  listeners.forEach((notify) => notify());
}

export function toggleLogOrder(): void {
  setLogOrder(read() === "newest" ? "oldest" : "newest");
}

/** Reactive read of the preference. */
export function useLogOrder(): LogOrder {
  return useSyncExternalStore(
    (notify) => {
      listeners.add(notify);
      return () => listeners.delete(notify);
    },
    read,
    () => "oldest",
  );
}

/** Return `entries` in the chosen order. Input is assumed chronological
 * (oldest-first, as the vault writes them); newest-first is a reversed
 * copy — the input is never mutated. */
export function orderLogs<T>(entries: readonly T[], order: LogOrder): T[] {
  return order === "newest" ? [...entries].reverse() : [...entries];
}
