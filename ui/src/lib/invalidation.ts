// VaultArea -> query-key-prefix map (plan §2.5). Coarse on purpose:
// refetches are index-backed and cheap, and a too-wide invalidation
// is self-healing while a too-narrow one shows stale data.
import type { QueryClient } from "@tanstack/react-query";
import type { VaultArea } from "../api/bindings/VaultArea";

export type { VaultArea };

const AREA_TO_PREFIXES: Record<VaultArea, string[]> = {
  // A milestone edit (in the app or in nvim) changes both the project
  // map and the commitments timeline, so projects invalidates both.
  // The weekly bundle composes projects, daily logs, commitments, AND
  // stewardships, so an edit in any of those areas must also refresh an
  // open review — hence get_weekly_bundle rides those four lists.
  projects: [
    "get_orientation",
    "list_projects",
    "get_project",
    "get_commitments",
    "get_weekly_bundle",
  ],
  actions: ["get_orientation", "list_all_actions"],
  daily: ["get_orientation", "read_daily", "get_weekly_bundle"],
  weekly: ["get_weekly_bundle"],
  commitments: ["get_orientation", "get_commitments", "get_weekly_bundle"],
  portfolios: ["list_portfolios", "get_portfolio"],
  // A tracking-log write (or an external edit under stewardships/)
  // touches both the list and the open detail — the detail composes the
  // series, recent entries, and count that a new note changes.
  stewardships: [
    "get_orientation",
    "list_stewardships",
    // No "get_stewardship" here: react-query matches query keys
    // element-wise, and there is no query with that prefix — the live
    // detail query is "get_stewardship_detail". A dead entry matches
    // nothing, so it's dropped rather than kept as noise.
    "get_stewardship_detail",
    "get_weekly_bundle",
  ],
  questions: ["get_strategic_bundle"],
  inbox: ["list_inbox"],
  // The config area covers both config.toml and the template files
  // under .cuaderno/templates/. An edit to a custom tracking template
  // (or config.toml) changes which fields the log form gathers, so it
  // refreshes the template-field query.
  config: ["get_orientation", "get_tracking_template_fields"],
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
