// The event bridge (day-change QA, M10): clock:day-changed must
// invalidate the date-dependent queries, vault:changed must fan out
// through the area map, and watcher:status must reach the callback.
// The @tauri-apps/api/event module is mocked to hand the handlers
// back (the same pattern as CaptureBar.test.tsx) so events can be
// driven directly instead of through the IPC internals.
import { afterEach, expect, test, vi } from "vitest";
import { QueryClient } from "@tanstack/react-query";
import { attachEventBridge, type WatcherStatusPayload } from "./events";

const { handlers } = vi.hoisted(() => ({
  handlers: new Map<string, Array<(event: unknown) => void>>(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: (event: unknown) => void) => {
    const list = handlers.get(event) ?? [];
    list.push(handler);
    handlers.set(event, list);
    return Promise.resolve(() => {});
  },
}));

/** Drive a backend event through the mocked listen() registrations. */
function emit(event: string, payload: unknown) {
  for (const handler of handlers.get(event) ?? []) {
    handler({ event, id: 0, payload });
  }
}

afterEach(() => {
  handlers.clear();
});

test("clock:day-changed invalidates the date-dependent queries", async () => {
  const client = new QueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  await attachEventBridge(client);
  invalidate.mockClear(); // drop the bridge's own post-attach global invalidation

  emit("clock:day-changed", "2026-07-09");

  // The exact set lives in invalidateDateDependent (lib/invalidation.ts);
  // pinned here so a day roll never silently stops refreshing a view
  // whose "today" came from the previous date.
  for (const key of ["get_orientation", "get_commitments", "get_weekly_bundle"]) {
    expect(invalidate).toHaveBeenCalledWith({ queryKey: [key] });
  }
});

test("vault:changed fans out through the area map", async () => {
  const client = new QueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  await attachEventBridge(client);
  invalidate.mockClear();

  emit("vault:changed", { origin: "external", areas: ["inbox"], paths: ["inbox/x.md"] });

  expect(invalidate).toHaveBeenCalledWith({ queryKey: ["list_inbox"] });
});

test("watcher:status reaches the callback with its payload", async () => {
  const client = new QueryClient();
  const seen: WatcherStatusPayload[] = [];
  await attachEventBridge(client, (status) => seen.push(status));

  emit("watcher:status", { state: "degraded" });
  emit("watcher:status", { state: "ok" });

  expect(seen).toEqual([{ state: "degraded" }, { state: "ok" }]);
});

test("attaching ends with one global invalidation to seal the startup race", async () => {
  const client = new QueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  await attachEventBridge(client);
  expect(invalidate).toHaveBeenCalledTimes(1);
  expect(invalidate).toHaveBeenCalledWith();
});
