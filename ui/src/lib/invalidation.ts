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
    // The Strategic allocator's slots + parked shelf are project state,
    // so a park/activate (in-app or an external edit) must refresh it.
    "get_strategic_bundle",
    // A project's `core_question` is a backlink on the question it names.
    "list_questions",
  ],
  // `get_now` rides the action and daily areas: the Now band is read back
  // from the day's log, so a start or completion made ANYWHERE — the CLI,
  // an MCP tool, another window — must refresh it. The band's own buttons
  // invalidate it directly; without these entries every other origin left
  // it stale, which is precisely the claim the feature makes (#442).
  actions: ["get_orientation", "list_all_actions", "get_now"],
  // A daily-note edit (in the app, the CLI, or nvim) refreshes the
  // orientation, the open note reader, the composed review, AND the
  // calendar's grid marks + embedded daily panel (read_daily,
  // list_daily_dates) — so a fresh edit shows up on the calendar (#340).
  daily: [
    "get_orientation",
    "read_daily",
    "list_daily_dates",
    "get_weekly_bundle",
    "get_now",
  ],
  // The calendar panel's week jump reads the raw weekly note
  // (read_weekly), distinct from the composed get_weekly_bundle.
  weekly: ["get_weekly_bundle", "read_weekly"],
  // The monthly note (#228) is only read by the calendar panel's month
  // jump, so an edit refreshes just that read (#340).
  monthly: ["read_monthly"],
  commitments: ["get_strategic_bundle", "get_orientation", "get_commitments", "get_weekly_bundle"],
  // A portfolio links to the question it gathers evidence for, so its
  // edits change what the questions view shows.
  portfolios: ["list_portfolios", "get_portfolio", "get_strategic_bundle", "list_questions"],
  // A tracking-log write (or an external edit under stewardships/)
  // touches both the list and the open detail — the detail composes the
  // series, recent entries, and count that a new note changes.
  stewardships: ["get_strategic_bundle", 
    "get_orientation",
    "list_stewardships",
    // No "get_stewardship" here: react-query matches query keys
    // element-wise, and there is no query with that prefix — the live
    // detail query is "get_stewardship_detail". A dead entry matches
    // nothing, so it's dropped rather than kept as noise.
    "get_stewardship_detail",
    "get_weekly_bundle",
  ],
  // `list_questions` composes each question WITH its backlinks, so it goes
  // stale on more than a question edit: a project's `core_question`, a
  // portfolio's link, an evidence note's origin all change what the view
  // shows. The strategic bundle has always had the same exposure; it is
  // listed on those areas for the same reason.
  questions: ["get_strategic_bundle", "list_questions"],
  inbox: ["list_inbox"],
  // The config area covers both config.toml and the template files
  // under .cuaderno/templates/. An edit to a custom tracking template
  // (or config.toml) changes which fields the log form gathers, so it
  // refreshes the template-field query. The Templates view (#357) reads
  // the template list, the selected template's content, and its
  // placeholder set — so an external template edit (or the app's own
  // save) refreshes all three. The Config view (#365) reads both the
  // raw document and the structured projection, so an external
  // config.toml edit refreshes both the raw editor's baseline and the
  // structured panel.
  config: [
    "get_orientation",
    "get_tracking_template_fields",
    "list_templates",
    "read_template",
    "list_template_placeholders",
    "read_config",
    "read_config_model",
  ],
};

export function invalidateAreas(client: QueryClient, areas: VaultArea[]): void {
  const prefixes = new Set(areas.flatMap((area) => AREA_TO_PREFIXES[area] ?? []));
  // The index-exclusion counts (#440) are area-independent: EVERY reconcile
  // rewrites them, and the reconcile that matters most is the one nobody
  // asked for — notes moved on disk into a folder an existing `ignore` glob
  // already matches, which emits whatever area those notes live in (or none
  // at all) and never `config`. Keying this off areas would mean the counts
  // update in the backend and never reach the notice. It is a read of
  // recorded state, not an index query, so refetching it on any change is
  // cheap.
  prefixes.add("get_index_exclusions");
  for (const prefix of prefixes) {
    void client.invalidateQueries({ queryKey: [prefix] });
  }
}

/** Everything date-dependent, for clock:day-changed. `get_today` leads so a
 * view that caches "what day is it" (the calendar, Home) re-reads it when the
 * day rolls over — otherwise a surface left open across midnight keeps
 * yesterday's date (e.g. the quick-log composer's today-only gate). */
export function invalidateDateDependent(client: QueryClient): void {
  // `get_now` too: it is scoped to a date, so a Today page left open across
  // midnight would otherwise keep showing yesterday's unfinished action
  // while the backend has moved on to a new day.
  for (const prefix of [
    "get_today",
    "get_orientation",
    "get_commitments",
    "get_weekly_bundle",
    "get_now",
  ]) {
    void client.invalidateQueries({ queryKey: [prefix] });
  }
}
