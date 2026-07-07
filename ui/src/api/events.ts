// Backend event subscriptions -> query invalidation (plan §2.5).
//
// Ordering matters: listeners are registered BEFORE the first query
// fetch (awaited in main.tsx ahead of mounting the QueryClient), and
// one global invalidation fires after attach — sealing the startup
// race where an event lands between first fetch and listen().
import { listen } from "@tauri-apps/api/event";
import type { QueryClient } from "@tanstack/react-query";
import { invalidateAreas, invalidateDateDependent, type VaultArea } from "../lib/invalidation";

export interface VaultChangedPayload {
  origin: "self_write" | "external";
  areas: VaultArea[];
  paths: string[];
}

export interface WatcherStatusPayload {
  state: "ok" | "degraded";
}

export async function attachEventBridge(
  client: QueryClient,
  onWatcherStatus?: (status: WatcherStatusPayload) => void,
): Promise<void> {
  await listen<VaultChangedPayload>("vault:changed", (event) => {
    invalidateAreas(client, event.payload.areas);
  });
  await listen<string>("clock:day-changed", () => {
    invalidateDateDependent(client);
  });
  await listen<WatcherStatusPayload>("watcher:status", (event) => {
    onWatcherStatus?.(event.payload);
  });
  // Catch anything emitted before the listeners attached.
  await client.invalidateQueries();
}
