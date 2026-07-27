// Stewardship Detail: charts appear only for an expanded stewardship
// with series; a flat one has no charts pane; recent entries open the
// reader; the log form submits with the template-derived vars.
import { afterEach, beforeAll, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useParams } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { ConfigDocument } from "../../api/bindings/ConfigDocument";
import type { StewardshipDetail as StewardshipDetailData } from "../../api/bindings/StewardshipDetail";
import type { TrackingSeries } from "../../api/bindings/TrackingSeries";
import { setShowMetrics } from "../../lib/metrics";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
import StewardshipDetail, {
  capSeries,
  DEFAULT_CHART_CAP,
  isDrawable,
  metricKeyOf,
  metricsGatedSeries,
} from "./StewardshipDetail";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

// The metrics toggle persists via localStorage; jsdom's here doesn't work
// (mirrors NoteContent.test.tsx).
beforeAll(() => {
  const store = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    value: {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, String(v)),
      removeItem: (k: string) => void store.delete(k),
      clear: () => store.clear(),
      key: (i: number) => [...store.keys()][i] ?? null,
      get length() {
        return store.size;
      },
    },
    configurable: true,
  });
});

const EXPANDED: StewardshipDetailData = {
  slug: "health",
  name: "Health",
  context: "personal",
  variant: "expanded",
  body_markdown: "## Current Status\nConsistent.",
  series: [
    {
      name: "gym · Sets",
      points: [
        { date: "2026-07-01", value: 6 },
        { date: "2026-07-05", value: 4 },
      ],
      unit: null,
      label: null,
      mark: null,
    },
  ],
  recent: [
    {
      path: "stewardships/health/tracking/2026-07-05-gym.md",
      stewardship: "health",
      activity: "gym",
      date: "2026-07-05",
      duration_min: 55,
      routine: null,
      body_excerpt: "Felt strong",
    },
  ],
  tracking_count: 2,
};

// Two series that exercise the mark heuristic: an all-integer count
// (draws as a column) alongside a fractional measure (keeps the line).
const MIXED: StewardshipDetailData = {
  slug: "health",
  name: "Health",
  context: "personal",
  variant: "expanded",
  body_markdown: "## Current Status\nConsistent.",
  series: [
    {
      name: "gym · Sets",
      points: [
        { date: "2026-07-01", value: 6 },
        { date: "2026-07-05", value: 4 },
      ],
      unit: null,
      label: null,
      mark: null,
    },
    {
      name: "weigh-in · Weight (kg)",
      points: [
        { date: "2026-07-01", value: 78.4 },
        { date: "2026-07-05", value: 77.9 },
      ],
      unit: null,
      label: null,
      mark: null,
    },
  ],
  recent: [],
  tracking_count: 4,
};

// A declared `plot = "none"` series alongside an undeclared body-table one,
// both for the same activity — isolates #500's mark filter from #489's
// metrics gate, since only one of the two survives the filter and gating
// then has nothing left to narrow.
const NONE_AND_TABLE: StewardshipDetailData = {
  ...EXPANDED,
  series: [
    {
      name: "gym · Sets",
      points: [{ date: "2026-07-01", value: 6 }],
      unit: null,
      label: null,
      mark: null,
    },
    {
      name: "gym · Effort",
      points: [{ date: "2026-07-01", value: 7 }],
      unit: null,
      label: null,
      mark: "none",
    },
  ],
};

// An expanded stewardship whose only series are all declared `plot =
// "none"` — nothing left to draw, so the Trends pane should not appear at
// all rather than render empty.
const ALL_NONE: StewardshipDetailData = {
  ...EXPANDED,
  series: [
    {
      name: "gym · Effort",
      points: [{ date: "2026-07-01", value: 7 }],
      unit: null,
      label: null,
      mark: "none",
    },
  ],
};

// Two series for the SAME activity — the shape #489's metrics gate acts
// on: one always-visible status trend, one further series that needs
// metrics on.
const MULTI_METRIC: StewardshipDetailData = {
  ...EXPANDED,
  series: [
    {
      name: "gym · Sets",
      points: [{ date: "2026-07-01", value: 6 }],
      unit: null,
      label: null,
      mark: null,
    },
    {
      name: "gym · Reps",
      points: [{ date: "2026-07-01", value: 40 }],
      unit: null,
      label: null,
      mark: null,
    },
  ],
};

// Eight distinct activities, one series each, so the cap is the only thing
// under test — metrics gating never removes anything from a single-series
// activity.
const MANY_ACTIVITIES: StewardshipDetailData = {
  ...EXPANDED,
  series: Array.from({ length: 8 }, (_, i) => ({
    name: `activity-${i + 1} · Sets`,
    points: [{ date: "2026-07-01", value: i + 1 }],
    unit: null,
    label: null,
    mark: null,
  })),
};

// Two full stewardships, keyed by slug, each linking to the other — the
// vehicle for driving a REAL in-app navigation between them rather than two
// separate mounts. That distinction matters: a fresh mount always starts
// clean regardless of the bug, so only a navigation that changes `:slug`
// on an already-mounted, already-cached route exercises the reconcile-not-
// remount path the fix (`key={slug}`) is for.
const NAV_HEALTH: StewardshipDetailData = {
  ...MANY_ACTIVITIES,
  slug: "health",
  name: "Health",
  body_markdown: "## Current Status\nSee also [[mood]].",
};

const NAV_MOOD: StewardshipDetailData = {
  ...EXPANDED,
  slug: "mood",
  name: "Mood",
  body_markdown: "## Current Status\nSee also [[health]].",
  recent: [],
  tracking_count: 0,
  // 7 activities, one fewer than NAV_HEALTH's 8, so each stewardship's
  // "N more" reveal button reads distinctly and a leaked cap state is
  // unmistakable rather than a coincidental match.
  series: Array.from({ length: 7 }, (_, i) => ({
    name: `activity-${i + 1} · Sets`,
    points: [{ date: "2026-07-01", value: i + 1 }],
    unit: null,
    label: null,
    mark: null,
  })),
};

// A single DECLARED series (`mark: "line"`, backed by a real
// `[tracking.gym.metrics.reps]` table) — the shape the plot-kind picker
// (#490) needs: a body-table (`mark: null`) series has nothing to persist
// a pick into, so the picker only ever appears here.
const DECLARED: StewardshipDetailData = {
  ...EXPANDED,
  series: [
    {
      name: "gym · reps",
      points: [
        { date: "2026-07-01", value: 6 },
        { date: "2026-07-05", value: 4 },
      ],
      unit: null,
      label: null,
      mark: "line",
    },
  ],
};

// Two declared series whose (activity, metric) pairs collide under a naive
// space-joined key: `("morning run", "pace")` and `("morning", "run pace")`
// both space-join to `"morning run pace"`. Both are free text (activity
// names and metric field names come from tracking-note frontmatter), so
// either can legitimately contain a space — the encoding itself must not
// collide, staging must stay independent per pair.
const COLLIDING_KEYS: StewardshipDetailData = {
  ...EXPANDED,
  series: [
    {
      name: "morning run · pace",
      points: [{ date: "2026-07-01", value: 5.2 }],
      unit: null,
      label: null,
      mark: "line",
    },
    {
      name: "morning · run pace",
      points: [{ date: "2026-07-01", value: 6.1 }],
      unit: null,
      label: null,
      mark: "line",
    },
  ],
};

const CONFIG_TOML = '[tracking.gym.metrics.reps]\ntype = "int"\nplot = "line"\n';
const CONFIG_HASH = "cafef00dcafef00d";

/** A minimal stand-in for the real `config_set_metric_plot` surgical
 * writer, faithful enough for these tests: swaps, inserts, or drops the
 * `plot` line under `[tracking.gym.metrics.reps]` in whatever `content` is
 * actually PASSED — not a fixed constant. The compose-from-current-draft
 * tests below stage several successive edits against content that has
 * moved on since the fixture's baseline, and a mock that ignored its own
 * `content` argument would silently paper over exactly the bug those
 * tests exist to catch. */
function mockApplyPlot(content: string, plot: string): string {
  if (plot === "none") {
    return content.replace(/plot = "[a-z]+"\n/, "");
  }
  return /plot = "[a-z]+"/.test(content)
    ? content.replace(/plot = "[a-z]+"/, `plot = "${plot}"`)
    : content.replace(
        "[tracking.gym.metrics.reps]\n",
        `[tracking.gym.metrics.reps]\nplot = "${plot}"\n`,
      );
}

/** How save_config should answer for the plot-kind picker's persist
 * action: resolve with a new document, or reject with a tagged
 * ConfigSaveError (mirrors Config.test.tsx's own SaveOutcome). */
type SaveOutcome = { ok: true; content: string; hash: string } | { ok: false; error: unknown };

/** Mounts DECLARED with the config commands the picker drives wired up:
 * `read_config` serves the fixed baseline, `config_set_metric_plot`
 * mimics the real surgical writer closely enough for these tests (swap or
 * drop the `plot` line), and `save_config` answers per `opts.save`
 * (defaulting to a success echoing the posted content/hash back). */
function renderDeclaredWith(
  opts: { calls?: Array<{ cmd: string; args: unknown }>; save?: SaveOutcome } = {},
) {
  const calls = opts.calls ?? [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    switch (cmd) {
      case "get_stewardship_detail":
        return DECLARED;
      case "get_tracking_template_fields":
        return [];
      case "read_config":
        return { content: CONFIG_TOML, hash: CONFIG_HASH };
      case "config_set_metric_plot": {
        const { content, plot } = args as { content: string; plot: string };
        return mockApplyPlot(content, plot);
      }
      case "validate_config":
        return undefined;
      case "save_config": {
        if (opts.save === undefined) {
          const { content, expectedHash } = args as { content: string; expectedHash: string };
          return { content, hash: expectedHash };
        }
        if (opts.save.ok) return { content: opts.save.content, hash: opts.save.hash };
        throw opts.save.error;
      }
      default:
        return undefined;
    }
  });
  return mountDetail(DECLARED);
}

const FLAT: StewardshipDetailData = {
  slug: "finances",
  name: "Finances",
  context: "household",
  variant: "flat",
  body_markdown: "## Current Status\nSteady.",
  series: [],
  recent: [],
  tracking_count: 0,
};

// The note page opening on `path` is now a navigation to `/note/<path>`;
// this stand-in route surfaces the navigated path so a test can assert a
// click opened the right note.
function NotePathProbe() {
  return <div data-testid="reader-path">{useParams()["*"] ?? ""}</div>;
}

function renderDetail(
  fixture: StewardshipDetailData,
  onCall?: (cmd: string, args: unknown) => void,
  at?: string,
) {
  mockIPC((cmd, args) => {
    onCall?.(cmd, args);
    if (cmd === "get_stewardship_detail") return fixture;
    if (cmd === "get_tracking_template_fields")
      return [{ name: "mood", prompt: "How did it feel?" }];
    return undefined;
  });
  return mountDetail(fixture, at);
}

/** For the cases that need a mock which can throw — the write error
 * paths, which a return-only mock cannot reach. */
function renderDetailWith(handler: (cmd: string, args: unknown) => unknown) {
  mockIPC(handler);
  return mountDetail(EXPANDED);
}

/** Returns the render result PLUS the `QueryClient` itself — the
 * compose-timing tests below need to drive a `read_config` refetch
 * directly (standing in for the watcher-driven invalidation `vault:changed`
 * triggers in the real app, which isn't wired up under this mock harness),
 * and `render()` alone gives no way back to the client that owns it. */
function mountDetail(fixture: StewardshipDetailData, at?: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const result = render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={[at ?? `/stewardships/${fixture.slug}`]}>
          {/* ReaderProvider needs a Router above it (it navigates); the
              `/note/*` stand-in route surfaces the opened path. */}
          <ReaderProvider>
            <Routes>
              <Route path="/stewardships/:slug" element={<StewardshipDetail />} />
              <Route path="/note/*" element={<NotePathProbe />} />
            </Routes>
          </ReaderProvider>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
  return { ...result, client };
}

afterEach(() => {
  cleanup();
  clearMocks();
  // The metrics store is module-global; reset so ordering between tests
  // cannot leak an "on" toggle into a test that assumes the default off.
  setShowMetrics(false);
});

test("an expanded stewardship with series shows the charts pane", async () => {
  renderDetail(EXPANDED);
  expect(await screen.findByText("Health")).toBeDefined();
  // The Trends section and its series caption render.
  expect(screen.getByRole("region", { name: "Trends" })).toBeDefined();
  expect(screen.getByText("gym · Sets")).toBeDefined();
});

test("an all-integer series draws as a column and a fractional series keeps the line", async () => {
  renderDetail(MIXED);
  await screen.findByText("Health");
  // The mark shows through as the figure's data-chart-kind — a
  // DOM-level signal that does not depend on Recharts' SVG internals
  // (which do not lay out under jsdom's zero-size container).
  const integerFigure = screen.getByText("gym · Sets").closest("figure");
  const fractionalFigure = screen.getByText("weigh-in · Weight (kg)").closest("figure");
  expect(integerFigure?.getAttribute("data-chart-kind")).toBe("column");
  expect(fractionalFigure?.getAttribute("data-chart-kind")).toBe("line");
});

test("a flat stewardship has no charts pane", async () => {
  renderDetail(FLAT);
  expect(await screen.findByText("Finances")).toBeDefined();
  // No Trends region at all — absent, not an empty frame.
  expect(screen.queryByRole("region", { name: "Trends" })).toBeNull();
  // And the flat variant offers no log form (no tracking/ subdir).
  expect(screen.queryByRole("button", { name: "Log entry" })).toBeNull();
});

test("a recent entry opens the note page at its path", async () => {
  renderDetail(EXPANDED);
  fireEvent.click(await screen.findByText("Felt strong"));
  expect((await screen.findByTestId("reader-path")).textContent).toBe(
    "stewardships/health/tracking/2026-07-05-gym.md",
  );
});

test("the log form fetches template fields and submits with the vars map", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderDetail(EXPANDED, (cmd, args) => calls.push({ cmd, args }));

  fireEvent.click(await screen.findByRole("button", { name: "Log entry" }));
  fireEvent.change(screen.getByLabelText("Activity"), { target: { value: "gym" } });

  // The debounced fetch populates the dynamic "mood" field.
  const mood = await screen.findByLabelText("How did it feel?");
  fireEvent.change(mood, { target: { value: "strong" } });
  fireEvent.change(screen.getByLabelText("Notes"), { target: { value: "Good one." } });

  fireEvent.click(screen.getByRole("button", { name: "Log it" }));
  expect(await screen.findByText(/one more on the record/)).toBeDefined();

  const logged = calls.find((c) => c.cmd === "log_tracking_entry");
  expect(logged?.args).toMatchObject({
    stewardship: "health",
    activity: "gym",
    content: "Good one.",
    vars: { mood: "strong" },
  });
});

test("switching activity clears prior field values and submits only the new activity's vars", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  // Both activities' templates return a field of the SAME name ("mood"),
  // so an un-reset value would silently ride across the switch.
  renderDetail(EXPANDED, (cmd, args) => calls.push({ cmd, args }));

  fireEvent.click(await screen.findByRole("button", { name: "Log entry" }));

  // Activity A: fill the "mood" field.
  fireEvent.change(screen.getByLabelText("Activity"), { target: { value: "gym" } });
  const moodA = await screen.findByLabelText("How did it feel?");
  fireEvent.change(moodA, { target: { value: "strong" } });
  expect((moodA as HTMLInputElement).value).toBe("strong");

  // Switch to activity B — the same-named field must come up empty.
  fireEvent.change(screen.getByLabelText("Activity"), { target: { value: "swim" } });
  await waitFor(() =>
    expect((screen.getByLabelText("How did it feel?") as HTMLInputElement).value).toBe(""),
  );

  const moodB = screen.getByLabelText("How did it feel?");
  fireEvent.change(moodB, { target: { value: "calm" } });
  fireEvent.click(screen.getByRole("button", { name: "Log it" }));
  expect(await screen.findByText(/one more on the record/)).toBeDefined();

  const logged = calls.find((c) => c.cmd === "log_tracking_entry");
  expect(logged?.args).toMatchObject({
    activity: "swim",
    vars: { mood: "calm" },
  });
  // A's value never rode along.
  expect((logged?.args as { vars: Record<string, string> }).vars).toEqual({ mood: "calm" });
});

test("logging is a header action, not the bottom of a page of charts", async () => {
  // The form used to sit below the dashboard, every chart and the recent
  // list — several screens of scrolling to reach the most frequent write
  // on the page.
  renderDetail(EXPANDED);
  await screen.findByRole("heading", { name: "Health" });
  expect(screen.queryByLabelText("Activity")).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Log entry" }));

  expect(screen.getByLabelText("Activity")).toBeDefined();
  // One control owns the state: the header button becomes the closer
  // rather than the form growing a second toggle of its own.
  expect(screen.queryByRole("button", { name: "Log entry" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Close log" }));
  expect(screen.queryByLabelText("Activity")).toBeNull();
});

test("arriving with ?log=1 opens the form, so the list's log link is one click", async () => {
  renderDetail(EXPANDED, undefined, "/stewardships/health?log=1");
  expect(await screen.findByLabelText("Activity")).toBeDefined();
});

test("the recent list says how many there are in all", async () => {
  // tracking_count came over the wire and nothing read it, so the page
  // showed a few entries and you could not tell whether there were six or
  // six hundred.
  renderDetail(EXPANDED);
  await screen.findByRole("heading", { name: "Health" });
  const recent = within(screen.getByRole("region", { name: "Recent tracking" }));
  expect(recent.getByText("2 in all")).toBeDefined();
  expect(recent.getByRole("button", { name: "see all" })).toBeDefined();
});

test("recent rows carry the duration and routine that already came over the wire", async () => {
  // The fixture had `routine: null`, so the routine branch never rendered
  // and could have been deleted with the suite still green.
  renderDetail({
    ...EXPANDED,
    recent: [{ ...EXPANDED.recent[0], routine: "push day" }],
  });
  const recent = within(await screen.findByRole("region", { name: "Recent tracking" }));
  expect(recent.getByText("55 min")).toBeDefined();
  expect(recent.getByText("push day")).toBeDefined();
});

test("a failed log keeps the form, and everything typed into it", async () => {
  // Closing rode `onSettled`, which fires on failure too — so a disk
  // error or a vault lock unmounted the only copy of what was typed.
  renderDetailWith((cmd) => {
    if (cmd === "get_stewardship_detail") return EXPANDED;
    if (cmd === "get_tracking_template_fields") return [];
    if (cmd === "log_tracking_entry") throw new Error("vault is read-only");
    return undefined;
  });

  fireEvent.click(await screen.findByRole("button", { name: "Log entry" }));
  fireEvent.change(screen.getByLabelText("Activity"), { target: { value: "gym" } });
  fireEvent.change(screen.getByLabelText("Notes"), { target: { value: "felt strong" } });
  fireEvent.click(screen.getByRole("button", { name: "Log it" }));

  expect(await screen.findByText(/read-only/)).toBeDefined();
  expect((screen.getByLabelText("Notes") as HTMLTextAreaElement).value).toBe("felt strong");
  expect((screen.getByLabelText("Activity") as HTMLInputElement).value).toBe("gym");
});

test("a successful log closes the form", async () => {
  renderDetail(EXPANDED);
  fireEvent.click(await screen.findByRole("button", { name: "Log entry" }));
  fireEvent.change(screen.getByLabelText("Activity"), { target: { value: "gym" } });
  fireEvent.click(screen.getByRole("button", { name: "Log it" }));

  await waitFor(() => expect(screen.queryByLabelText("Activity")).toBeNull());
});

test("Cancel closes the form too, not only the header button", async () => {
  renderDetail(EXPANDED);
  fireEvent.click(await screen.findByRole("button", { name: "Log entry" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

  expect(screen.queryByLabelText("Activity")).toBeNull();
  expect(screen.getByRole("button", { name: "Log entry" })).toBeDefined();
});

test("an activity with a space in it keeps its whole name", async () => {
  // Series are named "<activity> · <column>". Splitting on whitespace
  // labelled the chip "morning", and two activities sharing a first word
  // collapsed into one — which removed the filter entirely, since it only
  // appears when there is more than one.
  renderDetail({
    ...MIXED,
    series: [
      { name: "morning run · Sets", points: [{ date: "2026-07-01", value: 3 }], unit: null, label: null, mark: null },
      { name: "morning swim · Laps", points: [{ date: "2026-07-02", value: 20 }], unit: null, label: null, mark: null },
    ],
  });
  await screen.findByRole("heading", { name: "Trends" });

  const filter = within(screen.getByRole("group", { name: "Filter charts by activity" }));
  expect(filter.getByRole("button", { name: "morning run" })).toBeDefined();
  expect(filter.getByRole("button", { name: "morning swim" })).toBeDefined();

  fireEvent.click(filter.getByRole("button", { name: "morning swim" }));
  const charts = within(screen.getByRole("group", { name: "Trend charts" }));
  expect(charts.queryByText(/morning run/)).toBeNull();
});

test("an expanded stewardship with nothing tracked says so calmly", async () => {
  // Not a prompt to catch up: it is a perfectly good state to be in. The
  // section used to be omitted entirely, leaving a bare button.
  renderDetail(MIXED);
  const recent = within(await screen.findByRole("region", { name: "Recent tracking" }));
  expect(recent.getByText(/Nothing tracked yet/)).toBeDefined();
});

test("charts can be narrowed to one activity", async () => {
  // A gym stewardship tracking sets, reps and weight across three
  // activities is nine charts in one column.
  renderDetail(MIXED);
  await screen.findByRole("heading", { name: "Trends" });
  const filter = within(screen.getByRole("group", { name: "Filter charts by activity" }));
  expect(filter.getByRole("button", { name: "gym" })).toBeDefined();

  fireEvent.click(filter.getByRole("button", { name: "weigh-in" }));

  // Scoped to the charts themselves — the chips carry the same words.
  const charts = within(screen.getByRole("group", { name: "Trend charts" }));
  expect(charts.getByText(/weigh-in/)).toBeDefined();
  expect(charts.queryByText(/gym/)).toBeNull();
});

test('a series declared plot = "none" is not drawn; a body-table (mark: null) series still is', async () => {
  // #500: declaring is an explicit act and is allowed to change what is
  // drawn, but only the literal "none" is suppressed — an undeclared
  // body-table series (mark: null) is not the same thing and must keep
  // drawing.
  renderDetail(NONE_AND_TABLE);
  await screen.findByRole("heading", { name: "Trends" });
  expect(screen.getByText("gym · Sets")).toBeDefined();
  expect(screen.queryByText("gym · Effort")).toBeNull();
});

// --- The plot-kind picker (#490): stages a change locally with immediate
//     preview, writes nothing until an explicit action, and persists
//     through the SAME validate -> compare-and-swap -> write -> live-reload
//     gate every other config edit uses. ---

test("a body-table series (mark: null) has no picker — there is nothing to persist a pick into", async () => {
  renderDetail(MIXED);
  await screen.findByRole("heading", { name: "Trends" });
  expect(screen.queryByRole("combobox", { name: /Chart type for/ })).toBeNull();
});

test("changing the picker previews immediately and stages with NO ipc call at all", async () => {
  // The redesign's whole point (#490 follow-up): staging is synchronous
  // and side-effect free, so there is no async gap for a background
  // "adopt on-disk change" reseed to land in and desync the draft from
  // its hash — the root cause of the silent-clobber bug this closes.
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderDeclaredWith({ calls });

  await screen.findByRole("heading", { name: "Trends" });
  const figure = screen.getByText("gym · reps").closest("figure");
  expect(figure?.getAttribute("data-chart-kind")).toBe("line");

  const select = screen.getByRole("combobox", { name: "Chart type for gym · reps" });
  const callsBeforeStage = calls.length;
  fireEvent.change(select, { target: { value: "column" } });

  // The preview updates immediately, with no IPC call of any kind fired
  // by the change — not just no save_config, NOTHING.
  expect(figure?.getAttribute("data-chart-kind")).toBe("column");
  expect(calls.length).toBe(callsBeforeStage);
  expect(calls.some((c) => c.cmd === "config_set_metric_plot")).toBe(false);
  expect(calls.some((c) => c.cmd === "save_config")).toBe(false);
});

test('picking "none" dims the chart and states it will stop drawing, without removing it from the grid', async () => {
  renderDeclaredWith();
  await screen.findByRole("heading", { name: "Trends" });

  fireEvent.change(screen.getByRole("combobox", { name: "Chart type for gym · reps" }), {
    target: { value: "none" },
  });

  expect(await screen.findByText(/Saving will stop drawing this chart/)).toBeDefined();
  // Still in the grid — a picker change is a preview, not a removal.
  expect(
    within(screen.getByRole("group", { name: "Trend charts" })).getByText("gym · reps"),
  ).toBeDefined();
});

test("the explicit action persists the staged pick through save_config, with the current hash", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderDeclaredWith({ calls });

  await screen.findByRole("heading", { name: "Trends" });
  fireEvent.change(screen.getByRole("combobox", { name: "Chart type for gym · reps" }), {
    target: { value: "column" },
  });

  fireEvent.click(await screen.findByRole("button", { name: "Save chart type as default" }));

  await waitFor(() => {
    const saved = calls.find((c) => c.cmd === "save_config");
    expect(saved?.args).toMatchObject({ expectedHash: CONFIG_HASH });
  });
  // The surgical compose happens as part of THIS action now, not the
  // staging step above — persist is where config_set_metric_plot fires.
  expect(calls.some((c) => c.cmd === "config_set_metric_plot")).toBe(true);
  // The persist bar clears once the save lands.
  await waitFor(() =>
    expect(screen.queryByRole("group", { name: "Unsaved chart type changes" })).toBeNull(),
  );
});

test("persist composes from the draft as it is at persist time, not a value captured when staged", async () => {
  // An external edit lands AFTER the pick is staged but BEFORE persist is
  // clicked, while nothing is dirty — `useConfigDraft`'s "adopt while
  // clean" effect adopts it, same as it always would between an unrelated
  // Settings-dialog visit and a Save here. Under the old queue-and-apply
  // design this window is exactly where the bug lived, because staging
  // itself captured a base early. Under the redesign staging never reads
  // the draft at all, so this must resolve cleanly: persist composes
  // against the NEW content, not whatever was current back when the pick
  // was made.
  const calls: Array<{ cmd: string; args: unknown }> = [];
  let onDisk = { content: CONFIG_TOML, hash: CONFIG_HASH };
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    switch (cmd) {
      case "get_stewardship_detail":
        return DECLARED;
      case "get_tracking_template_fields":
        return [];
      case "read_config":
        return onDisk;
      case "config_set_metric_plot": {
        const { content, plot } = args as { content: string; plot: string };
        return mockApplyPlot(content, plot);
      }
      case "validate_config":
        return undefined;
      case "save_config": {
        const { content, expectedHash } = args as { content: string; expectedHash: string };
        return { content, hash: expectedHash };
      }
      default:
        return undefined;
    }
  });
  const { client } = mountDetail(DECLARED);

  await screen.findByRole("heading", { name: "Trends" });
  fireEvent.change(screen.getByRole("combobox", { name: "Chart type for gym · reps" }), {
    target: { value: "column" },
  });

  // The external edit: a NEW unrelated metric appears, under a NEW hash.
  onDisk = {
    content: `${CONFIG_TOML}\n[tracking.gym.metrics.weight]\ntype = "float"\n`,
    hash: "hash-after-external-edit",
  };
  await act(() => client.refetchQueries({ queryKey: ["read_config"] }));

  fireEvent.click(await screen.findByRole("button", { name: "Save chart type as default" }));

  await waitFor(() => expect(calls.some((c) => c.cmd === "save_config")).toBe(true));
  const saved = calls.find((c) => c.cmd === "save_config");
  expect(saved?.args).toMatchObject({ expectedHash: "hash-after-external-edit" });
  const savedContent = (saved?.args as { content: string }).content;
  // Composed from the FRESH base (carries the external edit's new metric)
  // with the staged pick applied on top of it.
  expect(savedContent).toContain("weight");
  expect(savedContent).toContain('plot = "column"');
});

test("an external config change landing between a stage and a persist produces a conflict, not a clobber", async () => {
  // The high-severity case the redesign exists to fix: composing a
  // multi-step staged edit is itself async (one `config_set_metric_plot`
  // round trip per pick), so a genuine concurrent hand-edit CAN still land
  // inside that window. Persist must be caught by the save's own
  // compare-and-swap in that case — a CONFLICT — rather than the mismatch
  // silently passing and overwriting the newer file, which is exactly what
  // the old design's unconditional draft/hash write-back allowed.
  //
  // The mock's `save_config` here performs the same compare-and-swap the
  // real backend does: it checks the posted hash against "what's actually
  // on disk", not against whatever the client's own state currently says.
  let onDisk = { content: CONFIG_TOML, hash: CONFIG_HASH };
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    switch (cmd) {
      case "get_stewardship_detail":
        return DECLARED;
      case "get_tracking_template_fields":
        return [];
      case "read_config":
        return onDisk;
      case "config_set_metric_plot": {
        const { content, plot } = args as { content: string; plot: string };
        // The concurrent hand-edit: it lands the moment persist's own
        // compose step starts touching the file, well after the pick was
        // staged and well before the final save.
        onDisk = { ...onDisk, hash: "hash-from-a-concurrent-hand-edit" };
        return mockApplyPlot(content, plot);
      }
      case "validate_config":
        return undefined;
      case "save_config": {
        const { content, expectedHash } = args as { content: string; expectedHash: string };
        if (expectedHash !== onDisk.hash) {
          throw { kind: "conflict" };
        }
        onDisk = { content, hash: "hash-after-save" };
        return onDisk;
      }
      default:
        return undefined;
    }
  });
  mountDetail(DECLARED);

  await screen.findByRole("heading", { name: "Trends" });
  fireEvent.change(screen.getByRole("combobox", { name: "Chart type for gym · reps" }), {
    target: { value: "column" },
  });
  fireEvent.click(await screen.findByRole("button", { name: "Save chart type as default" }));

  await waitFor(() => {
    const status = screen
      .getAllByRole("status")
      .find((n) => n.textContent?.includes("changed on disk"));
    expect(status).toBeDefined();
  });
  // save_config was genuinely attempted (and refused) — a true clobber
  // would show no conflict notice at all, just a quiet, wrong success.
  expect(calls.some((c) => c.cmd === "save_config")).toBe(true);
  expect(onDisk.hash).toBe("hash-from-a-concurrent-hand-edit");
});

test("Discard reverts the staged pick without ever calling save_config", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderDeclaredWith({ calls });

  await screen.findByRole("heading", { name: "Trends" });
  const select = screen.getByRole("combobox", {
    name: "Chart type for gym · reps",
  }) as HTMLSelectElement;
  fireEvent.change(select, { target: { value: "column" } });

  fireEvent.click(await screen.findByRole("button", { name: "Discard chart type changes" }));

  await waitFor(() =>
    expect(screen.queryByRole("group", { name: "Unsaved chart type changes" })).toBeNull(),
  );
  expect(select.value).toBe("line");
  expect(calls.some((c) => c.cmd === "save_config")).toBe(false);
});

test("a compare-and-swap conflict is surfaced with a reload, never silently clobbered", async () => {
  renderDeclaredWith({ save: { ok: false, error: { kind: "conflict" } } });

  await screen.findByRole("heading", { name: "Trends" });
  fireEvent.change(screen.getByRole("combobox", { name: "Chart type for gym · reps" }), {
    target: { value: "column" },
  });
  fireEvent.click(await screen.findByRole("button", { name: "Save chart type as default" }));

  await waitFor(() => {
    const status = screen
      .getAllByRole("status")
      .find((n) => n.textContent?.includes("changed on disk"));
    expect(status).toBeDefined();
  });
  expect(screen.getByRole("button", { name: "Reload" })).toBeDefined();
});

test("the picker has no axe violations while a change is staged, or while a conflict is surfaced", async () => {
  const { container } = renderDeclaredWith();
  await screen.findByRole("heading", { name: "Trends" });
  fireEvent.change(screen.getByRole("combobox", { name: "Chart type for gym · reps" }), {
    target: { value: "none" },
  });
  await screen.findByRole("group", { name: "Unsaved chart type changes" });
  expect(
    await axe(container, { rules: { "color-contrast": { enabled: false } } }),
  ).toHaveNoViolations();

  cleanup();
  const conflict = renderDeclaredWith({ save: { ok: false, error: { kind: "conflict" } } });
  await screen.findByRole("heading", { name: "Trends" });
  fireEvent.change(screen.getByRole("combobox", { name: "Chart type for gym · reps" }), {
    target: { value: "column" },
  });
  fireEvent.click(await screen.findByRole("button", { name: "Save chart type as default" }));
  await screen.findByRole("button", { name: "Reload" });
  expect(
    await axe(conflict.container, { rules: { "color-contrast": { enabled: false } } }),
  ).toHaveNoViolations();
});

test("the picker is disabled with an accessible explanation before the config read resolves", async () => {
  let resolveRead!: (doc: ConfigDocument) => void;
  const pendingRead = new Promise<ConfigDocument>((resolve) => {
    resolveRead = resolve;
  });
  mockIPC((cmd) => {
    switch (cmd) {
      case "get_stewardship_detail":
        return DECLARED;
      case "get_tracking_template_fields":
        return [];
      case "read_config":
        return pendingRead;
      default:
        return undefined;
    }
  });
  mountDetail(DECLARED);

  const select = (await screen.findByRole("combobox", {
    name: "Chart type for gym · reps",
  })) as HTMLSelectElement;
  expect(select.disabled).toBe(true);
  // A greyed-out select alone doesn't say WHY — the reason is both visible
  // and wired to the control via aria-describedby.
  const describedBy = select.getAttribute("aria-describedby");
  expect(describedBy).not.toBeNull();
  expect(document.getElementById(describedBy as string)?.textContent).toMatch(/reading/i);

  await act(async () => {
    resolveRead({ content: CONFIG_TOML, hash: CONFIG_HASH });
    await pendingRead;
  });
  await waitFor(() => expect(select.disabled).toBe(false));
});

test("the picker is disabled with an accessible explanation when the config read errors", async () => {
  mockIPC((cmd) => {
    switch (cmd) {
      case "get_stewardship_detail":
        return DECLARED;
      case "get_tracking_template_fields":
        return [];
      case "read_config":
        throw new Error("vault is locked");
      default:
        return undefined;
    }
  });
  mountDetail(DECLARED);

  const select = (await screen.findByRole("combobox", {
    name: "Chart type for gym · reps",
  })) as HTMLSelectElement;
  await waitFor(() => expect(select.disabled).toBe(true));
  const describedBy = select.getAttribute("aria-describedby");
  expect(describedBy).not.toBeNull();
  expect(document.getElementById(describedBy as string)?.textContent).toMatch(
    /could not be read/i,
  );
});

test("the picker is disabled with an accessible explanation while a save is in flight", async () => {
  let resolveSave!: (doc: ConfigDocument) => void;
  const pendingSave = new Promise<ConfigDocument>((resolve) => {
    resolveSave = resolve;
  });
  mockIPC((cmd, args) => {
    switch (cmd) {
      case "get_stewardship_detail":
        return DECLARED;
      case "get_tracking_template_fields":
        return [];
      case "read_config":
        return { content: CONFIG_TOML, hash: CONFIG_HASH };
      case "config_set_metric_plot": {
        const { content, plot } = args as { content: string; plot: string };
        return mockApplyPlot(content, plot);
      }
      case "validate_config":
        return undefined;
      case "save_config":
        return pendingSave;
      default:
        return undefined;
    }
  });
  mountDetail(DECLARED);

  await screen.findByRole("heading", { name: "Trends" });
  const select = screen.getByRole("combobox", {
    name: "Chart type for gym · reps",
  }) as HTMLSelectElement;
  fireEvent.change(select, { target: { value: "column" } });
  fireEvent.click(await screen.findByRole("button", { name: "Save chart type as default" }));

  await waitFor(() => expect(select.disabled).toBe(true));
  const describedBy = select.getAttribute("aria-describedby");
  expect(describedBy).not.toBeNull();
  expect(document.getElementById(describedBy as string)?.textContent).toMatch(/saving/i);

  await act(async () => {
    resolveSave({ content: CONFIG_TOML.replace('plot = "line"', 'plot = "column"'), hash: "new-hash" });
    await pendingSave;
  });
  await waitFor(() => expect(select.disabled).toBe(false));
});

test("staged picks for colliding activity/metric pairs stay independent", async () => {
  // A naive space-joined key collapses ("morning run", "pace") and
  // ("morning", "run pace") into the same staged entry — picking a plot
  // kind for one would silently apply to (and later clobber the preview
  // of) the other. This does not depend on persisting: staging alone must
  // already keep the two apart.
  mockIPC((cmd) => {
    switch (cmd) {
      case "get_stewardship_detail":
        return COLLIDING_KEYS;
      case "get_tracking_template_fields":
        return [];
      case "read_config":
        return { content: "", hash: "h" };
      default:
        return undefined;
    }
  });
  mountDetail(COLLIDING_KEYS);

  await screen.findByRole("heading", { name: "Trends" });
  const selectA = screen.getByRole("combobox", {
    name: "Chart type for morning run · pace",
  }) as HTMLSelectElement;
  const selectB = screen.getByRole("combobox", {
    name: "Chart type for morning · run pace",
  }) as HTMLSelectElement;

  fireEvent.change(selectA, { target: { value: "column" } });
  expect(selectA.value).toBe("column");
  expect(selectB.value).toBe("line");

  fireEvent.change(selectB, { target: { value: "area" } });
  // Staging B must not have overwritten A's independently-staged pick —
  // the exact failure a colliding key would cause.
  expect(selectA.value).toBe("column");
  expect(selectB.value).toBe("area");
});

test('a stewardship whose only series are all plot = "none" has no Trends pane', async () => {
  // Nothing survives the filter, so there is nothing to draw — the pane
  // should be absent, not an empty frame with a heading and no charts.
  renderDetail(ALL_NONE);
  await screen.findByText("Health");
  expect(screen.queryByRole("region", { name: "Trends" })).toBeNull();
});

test("with metrics off, only the first series per activity renders; with metrics on, all do", async () => {
  // #489: a single always-visible status trend per activity, every
  // further series for that activity behind useMetrics().
  renderDetail(MULTI_METRIC);
  await screen.findByRole("heading", { name: "Trends" });
  expect(screen.getByText("gym · Sets")).toBeDefined();
  expect(screen.queryByText("gym · Reps")).toBeNull();

  act(() => setShowMetrics(true));
  expect(await screen.findByText("gym · Reps")).toBeDefined();
});

test('beyond the cap, the extra series are hidden until "show all" is used, and the control says how many are hidden', async () => {
  const { container } = renderDetail(MANY_ACTIVITIES);
  await screen.findByRole("heading", { name: "Trends" });
  // DEFAULT_CHART_CAP is 6 of the 8 series the fixture declares, so 2 are
  // hidden — the count the control states below.
  expect(container.querySelectorAll("[data-chart-kind]")).toHaveLength(DEFAULT_CHART_CAP);

  const reveal = screen.getByRole("button", { name: "Show 2 more charts" });
  fireEvent.click(reveal);

  expect(container.querySelectorAll("[data-chart-kind]")).toHaveLength(8);
  expect(screen.getByRole("button", { name: "Show fewer charts" })).toBeDefined();
});

test("has no axe violations, with the cap's control both collapsed and revealed", async () => {
  const { container } = renderDetail(MANY_ACTIVITIES);
  await screen.findByRole("heading", { name: "Trends" });
  expect(
    await axe(container, { rules: { "color-contrast": { enabled: false } } }),
  ).toHaveNoViolations();

  fireEvent.click(screen.getByRole("button", { name: "Show 2 more charts" }));
  expect(
    await axe(container, { rules: { "color-contrast": { enabled: false } } }),
  ).toHaveNoViolations();
});

test("navigating from one stewardship to another resets the chip selection and the expanded cap", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "get_stewardship_detail") {
      const { slug } = args as { slug: string };
      return slug === "mood" ? NAV_MOOD : NAV_HEALTH;
    }
    if (cmd === "resolve_wikilink") {
      const { target } = args as { target: string };
      return target === "mood"
        ? { path: "stewardships/mood.md", note_type: "stewardship" }
        : { path: "stewardships/health.md", note_type: "stewardship" };
    }
    return undefined;
  });
  mountDetail(NAV_HEALTH);

  // Visit mood once so it is cached going forward — the route change back
  // to health below then reconciles the cached query instead of showing
  // the "Reading the vault…" interstitial, which is exactly what a
  // `:slug`-only route change does when the destination is already warm.
  await screen.findByText("mood");
  fireEvent.click(screen.getByText("mood"));
  await screen.findByRole("heading", { name: "Mood" });

  // On mood: reveal the capped charts and narrow to one activity — state
  // that must NOT ride along to the next stewardship.
  await screen.findByRole("heading", { name: "Trends" });
  fireEvent.click(screen.getByRole("button", { name: "Show 1 more chart" }));
  expect(screen.getByRole("button", { name: "Show fewer charts" })).toBeDefined();
  const moodFilter = within(screen.getByRole("group", { name: "Filter charts by activity" }));
  fireEvent.click(moodFilter.getByRole("button", { name: "activity-1" }));
  expect(moodFilter.getByRole("button", { name: "activity-1" }).getAttribute("aria-pressed")).toBe(
    "true",
  );

  // Health is already cached from the initial mount — this route change
  // only swaps `:slug`, the exact reconcile-not-remount path a stewardship-
  // to-stewardship navigation takes.
  fireEvent.click(screen.getByText("health"));
  await screen.findByRole("heading", { name: "Health" });

  // Health's own state, not mood's leftovers: the cap starts collapsed
  // again (2 of 8 hidden, not "Show fewer charts") and no chip is pressed.
  expect(await screen.findByRole("button", { name: "Show 2 more charts" })).toBeDefined();
  const healthFilter = within(screen.getByRole("group", { name: "Filter charts by activity" }));
  expect(
    healthFilter.getByRole("button", { name: "activity-1" }).getAttribute("aria-pressed"),
  ).toBe("false");
});

test("a stale activity selection does not strand the Trends grid empty", async () => {
  // A config edit setting `plot = "none"` on the selected activity, seen
  // through a watcher-driven refetch, is what this reproduces — the
  // component never unmounts, so a stale `activities` selection would
  // otherwise survive the refetch and filter the grid down to nothing
  // under a heading that still says "Trends". A log submission's own
  // invalidate-and-refetch (`onLogged` in the component) is the easiest
  // mounted refetch to drive without a real filesystem watcher.
  let weighInDropped = false;
  mockIPC((cmd) => {
    if (cmd === "get_stewardship_detail") {
      return weighInDropped
        ? { ...MIXED, series: [MIXED.series[0], { ...MIXED.series[1], mark: "none" }] }
        : MIXED;
    }
    if (cmd === "get_tracking_template_fields") return [];
    if (cmd === "log_tracking_entry") {
      weighInDropped = true;
      return undefined;
    }
    return undefined;
  });
  mountDetail(MIXED);

  await screen.findByRole("heading", { name: "Trends" });
  const filter = within(screen.getByRole("group", { name: "Filter charts by activity" }));
  fireEvent.click(filter.getByRole("button", { name: "weigh-in" }));
  expect(
    within(screen.getByRole("group", { name: "Trend charts" })).getByText(/weigh-in/),
  ).toBeDefined();

  fireEvent.click(screen.getByRole("button", { name: "Log entry" }));
  fireEvent.change(screen.getByLabelText("Activity"), { target: { value: "gym" } });
  fireEvent.click(screen.getByRole("button", { name: "Log it" }));
  await waitFor(() => expect(screen.queryByLabelText("Activity")).toBeNull());

  // weigh-in dropped out, leaving one activity — no chip row left to
  // un-press — and the surviving series draws instead of an empty grid.
  await waitFor(() =>
    expect(screen.queryByRole("group", { name: "Filter charts by activity" })).toBeNull(),
  );
  expect(
    within(screen.getByRole("group", { name: "Trend charts" })).getByText("gym · Sets"),
  ).toBeDefined();
});

// Pure functions, unit-tested without rendering a single chart.

function series(name: string, mark: TrackingSeries["mark"] = null): TrackingSeries {
  return { name, points: [{ date: "2026-07-01", value: 1 }], unit: null, label: null, mark };
}

test('isDrawable suppresses only a literal "none" mark', () => {
  expect(isDrawable(series("gym · Sets", null))).toBe(true);
  expect(isDrawable(series("gym · Sets", "line"))).toBe(true);
  expect(isDrawable(series("gym · Sets", "column"))).toBe(true);
  expect(isDrawable(series("gym · Sets", "none"))).toBe(false);
});

test("metricKeyOf recovers the raw config key from a series name, even across a grouped series", () => {
  // The metric segment is always LAST, whether or not a group sits between
  // it and the activity — see `metricKeyOf`'s own doc comment for why that
  // holds even when a group value legitimately contains the separator.
  expect(metricKeyOf("gym · reps")).toBe("reps");
  expect(metricKeyOf("gym · upper-body · reps")).toBe("reps");
  expect(metricKeyOf("weigh-in · weight")).toBe("weight");
});

test("metricsGatedSeries keeps one series per activity when metrics are off, and all of them when on", () => {
  const input = [series("gym · Sets"), series("gym · Reps"), series("weigh-in · Weight (kg)")];

  expect(metricsGatedSeries(input, false).map((s) => s.name)).toEqual([
    "gym · Sets",
    "weigh-in · Weight (kg)",
  ]);
  expect(metricsGatedSeries(input, true)).toEqual(input);
});

test("metricsGatedSeries preserves order and treats input order as which series is \"first\"", () => {
  const input = [series("gym · Reps"), series("gym · Sets")];
  // "Reps" is first here, so it is the one kept — the caller decides
  // which series is the status trend by what order it hands over.
  expect(metricsGatedSeries(input, false).map((s) => s.name)).toEqual(["gym · Reps"]);
});

test("capSeries leaves a list at or under the cap untouched", () => {
  const input = [series("a · x"), series("b · x"), series("c · x")];
  expect(capSeries(input, 6)).toEqual({ shown: input, hiddenCount: 0 });
  expect(capSeries(input, 3)).toEqual({ shown: input, hiddenCount: 0 });
});

test("capSeries truncates beyond the cap and reports how many were left off", () => {
  const input = Array.from({ length: 8 }, (_, i) => series(`activity-${i} · Sets`));
  const capped = capSeries(input, DEFAULT_CHART_CAP);
  expect(capped.shown).toHaveLength(DEFAULT_CHART_CAP);
  expect(capped.shown).toEqual(input.slice(0, DEFAULT_CHART_CAP));
  expect(capped.hiddenCount).toBe(2);
});

test("the always-on series for a grouped activity is its ungrouped one", () => {
  // Series arrive name-sorted, so taking the plain first would leave a
  // grouped activity represented by whichever category sorts earliest - one
  // slice standing in for the whole, which is not a status read.
  const s = (name: string): TrackingSeries => ({
    name,
    points: [{ date: "2026-07-01", value: 1 }],
    unit: null,
    label: null,
    mark: null,
  });
  const series = [
    s("practice · ear-training · minutes"),
    s("practice · harmony · minutes"),
    s("practice · minutes"),
  ];

  const gated = metricsGatedSeries(series, false);
  expect(gated.map((g) => g.name)).toEqual(["practice · minutes"]);
  // And with metrics on, everything is back, in the order it arrived.
  expect(metricsGatedSeries(series, true)).toHaveLength(3);
});

test("an activity with only grouped series still keeps one", () => {
  const s = (name: string): TrackingSeries => ({
    name,
    points: [{ date: "2026-07-01", value: 1 }],
    unit: null,
    label: null,
    mark: null,
  });
  const series = [s("spending · food · amount"), s("spending · transport · amount")];

  expect(metricsGatedSeries(series, false).map((g) => g.name)).toEqual([
    "spending · food · amount",
  ]);
});
