// Backend event subscriptions -> query invalidation (plan §2.5).
//
// Ordering matters: listeners are registered BEFORE the first query
// fetch (awaited in main.tsx ahead of mounting the QueryClient), and
// one global invalidation fires after attach — sealing the startup
// race where an event lands between first fetch and listen().
import { listen } from "@tauri-apps/api/event";
import type { QueryClient } from "@tanstack/react-query";
import type { VaultChanged } from "./bindings/VaultChanged";
import type { ConfigStatus } from "./bindings/ConfigStatus";
import { invalidateAreas, invalidateDateDependent } from "../lib/invalidation";
import { setConfigStatus } from "../lib/configStatus";

export interface WatcherStatusPayload {
  state: "ok" | "degraded";
}

export async function attachEventBridge(
  client: QueryClient,
  onWatcherStatus?: (status: WatcherStatusPayload) => void,
): Promise<void> {
  try {
    await listen<VaultChanged>("vault:changed", (event) => {
      invalidateAreas(client, event.payload.areas);
    });
    await listen<string>("clock:day-changed", () => {
      invalidateDateDependent(client);
    });
    await listen<WatcherStatusPayload>("watcher:status", (event) => {
      onWatcherStatus?.(event.payload);
    });
    // An external config.toml edit was re-read: {valid:false, message} when
    // it failed to open (the app kept the last good config), or {valid:true}
    // to clear a prior notice. Writes straight to the module store the shell
    // banner reads — no callback to thread (GH #365 PR4).
    await listen<ConfigStatus>("config:status", (event) => {
      setConfigStatus({
        valid: event.payload.valid,
        message: event.payload.message ?? null,
      });
    });
  } finally {
    // Catch anything emitted before (or between) listener
    // registrations — runs even when a later listen() rejects, so a
    // partially-attached bridge still starts from fresh data.
    await client.invalidateQueries();
  }
}
