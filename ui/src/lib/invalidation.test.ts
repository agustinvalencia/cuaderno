import { expect, test } from "vitest";
import { QueryClient } from "@tanstack/react-query";

import { invalidateAreas, invalidateDateDependent } from "./invalidation";

/** Seed a resolved query so it becomes an entry the invalidator can mark. */
async function seed(client: QueryClient, key: string) {
  await client.fetchQuery({ queryKey: [key], queryFn: async () => "seeded" });
}

test("a vault change invalidates the queries mapped to its areas", async () => {
  const client = new QueryClient();
  await seed(client, "get_project");
  await seed(client, "list_stewardships");

  invalidateAreas(client, ["projects"]);

  expect(client.getQueryState(["get_project"])?.isInvalidated).toBe(true);
  expect(client.getQueryState(["list_stewardships"])?.isInvalidated).toBe(false);
});

test("the index-exclusion counts are refreshed by any vault change (#440)", async () => {
  // The reconcile that matters most is the one nobody asked for: notes moved
  // on disk into a folder an `ignore` glob already matches. That emits
  // whatever area those notes live in — never `config` — so keying the
  // exclusion query off areas would leave the notice showing stale counts.
  const client = new QueryClient();
  await seed(client, "get_index_exclusions");

  invalidateAreas(client, ["projects"]);

  expect(client.getQueryState(["get_index_exclusions"])?.isInvalidated).toBe(true);
});

test("the index-exclusion counts are refreshed even by an area-less change", async () => {
  // A batch of notes moved into an ignored folder can classify to no area
  // at all; the backend emits an empty-areas change precisely so the notice
  // still hears about it.
  const client = new QueryClient();
  await seed(client, "get_index_exclusions");

  invalidateAreas(client, []);

  expect(client.getQueryState(["get_index_exclusions"])?.isInvalidated).toBe(true);
});

test("the Now band refreshes on a daily-log change from any origin (#442)", async () => {
  // The band is read back from the day's log, so a `cdno log` in a terminal
  // or an `append_to_log` over MCP must reach it — the band's own buttons
  // invalidating it directly covers only in-app clicks, and the global
  // staleTime means nothing else would.
  const client = new QueryClient();
  await seed(client, "get_now");

  invalidateAreas(client, ["daily"]);

  expect(client.getQueryState(["get_now"])?.isInvalidated).toBe(true);
});

test("the Now band refreshes when an action changes elsewhere", async () => {
  const client = new QueryClient();
  await seed(client, "get_now");

  invalidateAreas(client, ["actions"]);

  expect(client.getQueryState(["get_now"])?.isInvalidated).toBe(true);
});

test("the Now band refreshes when the day rolls over", async () => {
  // It is scoped to a date; a Today page left open across midnight would
  // otherwise show yesterday's unfinished action.
  const client = new QueryClient();
  await seed(client, "get_now");

  invalidateDateDependent(client);

  expect(client.getQueryState(["get_now"])?.isInvalidated).toBe(true);
});
