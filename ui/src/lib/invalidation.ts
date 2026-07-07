// VaultArea -> query-key-prefix map (plan §2.5). Coarse on purpose:
// refetches are index-backed and cheap, and a too-wide invalidation
// is self-healing while a too-narrow one shows stale data.
import type { QueryClient } from "@tanstack/react-query";
import type { VaultArea } from "../api/bindings/VaultArea";

export type { VaultArea };

const AREA_TO_PREFIXES: Record<VaultArea, string[]> = {
  projects: ["get_orientation", "list_projects", "get_project"],
  actions: ["get_orientation", "list_all_actions"],
  daily: ["get_orientation", "read_daily"],
  weekly: ["get_weekly_bundle"],
  commitments: ["get_orientation", "get_commitments"],
  portfolios: ["list_portfolios", "get_portfolio"],
  stewardships: ["get_orientation", "list_stewardships", "get_stewardship"],
  questions: ["get_strategic_bundle"],
  inbox: ["list_inbox"],
  config: ["get_orientation"],
};

export function invalidateAreas(client: QueryClient, areas: VaultArea[]): void {
  const prefixes = new Set(areas.flatMap((area) => AREA_TO_PREFIXES[area] ?? []));
  for (const prefix of prefixes) {
    void client.invalidateQueries({ queryKey: [prefix] });
  }
}

/** Everything date-dependent, for clock:day-changed. */
export function invalidateDateDependent(client: QueryClient): void {
  for (const prefix of ["get_orientation", "get_commitments", "get_weekly_bundle"]) {
    void client.invalidateQueries({ queryKey: [prefix] });
  }
}
