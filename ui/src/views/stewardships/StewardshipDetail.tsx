// Stewardship Detail (M7, plan §1.7) — the dashboard behind
// `/stewardships/:slug`. The body renders verbatim (status, habits,
// periodic commitments — the qualitative surface). Trend charts are
// STATUS visualisations, never goals: no target lines, no red zones,
// colours drawn from the calm context hues. Charts appear only for an
// expanded stewardship that has numeric tracking. Recent entries open
// in the note reader; the Log Entry form files a new tracking note,
// its dynamic fields derived from the tracking template's prompts.
import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams, useSearchParams } from "react-router";
import type { StewardshipDetail as StewardshipDetailData } from "../../api/bindings/StewardshipDetail";
import type { TrackingSeries } from "../../api/bindings/TrackingSeries";
import {
  errorMessage,
  getStewardshipDetail,
  getTrackingTemplateFields,
  logTrackingEntry,
  openInEditor,
  resolveWikilink,
} from "../../api/commands";
import {
  markForSeries,
  SERIES_COLORS,
  TrendChart,
  usePrefersReducedMotion,
} from "../../components/charts/TrendChart";
import Markdown from "../../components/markdown/Markdown";
import { contextDotClass } from "../../lib/contexts";
import { useMetrics } from "../../lib/metrics";
import { useReader } from "../../shell/reader";
import { shortDate } from "../../lib/dates";
import { ClampedText } from "../../components/ui/clamped-text";
import { SectionHeading } from "../../components/ui/section-heading";
import { useToast } from "../../shell/Toasts";

/** The activity a tracking series belongs to. Series are named
 * `"<activity> · <column>"`, and only the first separator splits them —
 * a column header may contain one. */
/** The separator the domain formats a series name with:
 * `activity · [group · ]metric`. */
const SERIES_SEPARATOR = " \u00b7 ";

export function activityOf(seriesName: string): string {
  const at = seriesName.indexOf(SERIES_SEPARATOR);
  return at === -1 ? seriesName : seriesName.slice(0, at);
}

/** A series declared `plot = "none"` (#500) stays collected and queryable
 * over MCP but is not drawn in the desktop \u2014 declaring an activity is an
 * explicit act, and is allowed to change what is drawn. This is a literal
 * `"none"`, not the absence of a declaration: a body-table series carries
 * `mark: null` and must still draw, so it is deliberately not filtered
 * here. Keeping the check this narrow is what keeps #485's body-table
 * suppression untouched \u2014 that rule keys on the frontmatter derivation's
 * full produced set, not on anything this filters out afterwards. */
export function isDrawable(series: TrackingSeries): boolean {
  return series.mark !== "none";
}

/** Series beyond an activity's first are the dense detail #489 decided
 * should sit behind `useMetrics()` \u2014 per-category lines, mean-aggregated
 * ratings \u2014 while a single calm status trend per activity keeps the
 * "status, not goals" exemption the toggle already grants trend charts.
 * "First" means first in `series` as given: callers that care which
 * series reads as the status trend must order it there before calling
 * this. Exported standalone so the rule is unit-testable without
 * rendering a chart. */
export function metricsGatedSeries(
  series: TrackingSeries[],
  metricsOn: boolean,
): TrackingSeries[] {
  if (metricsOn) return series;
  // Which one survives matters. Series arrive name-sorted, so taking the
  // plain first would leave a grouped activity represented by whichever
  // category sorts earliest — one slice of it standing in for the whole,
  // which is not a status read. An UNGROUPED series is the activity's own
  // number (its name is `activity · metric`, with no group segment), so
  // prefer one of those when the activity has one and fall back to the
  // first otherwise.
  const isUngrouped = (name: string) => name.split(SERIES_SEPARATOR).length === 2;
  const chosen = new Map<string, TrackingSeries>();
  for (const s of series) {
    const activity = activityOf(s.name);
    const current = chosen.get(activity);
    if (!current || (!isUngrouped(current.name) && isUngrouped(s.name))) {
      chosen.set(activity, s);
    }
  }
  // Preserve the incoming order rather than the Map's insertion order, so
  // the gated list reads the same way the ungated one does.
  const keep = new Set(chosen.values());
  return series.filter((s) => keep.has(s));
}

// Six is two full rows of the Trends grid's two-up (`lg`) layout \u2014 a
// browsable first screenful with no scroll, versus the "nine charts in one
// column, roughly 1600px of scroll" #489 named as the problem. It is a cap,
// not a second filter: unlike the activity chips or the metrics gate above,
// this bounds the count outright, so a grouped metric that fans out to a new
// chart per category (#483) cannot grow the default view on its own.
export const DEFAULT_CHART_CAP = 6;

/** Cap a series list to `cap`, reporting how many were left off so the
 * caller's "show all" control can say so. Applied last, after both filters
 * above \u2014 capping a set that already excludes undrawable and
 * metrics-gated series never hides something a reader could not have seen
 * anyway. */
export function capSeries(
  series: TrackingSeries[],
  cap: number,
): { shown: TrackingSeries[]; hiddenCount: number } {
  if (series.length <= cap) return { shown: series, hiddenCount: 0 };
  return { shown: series.slice(0, cap), hiddenCount: series.length - cap };
}

/** The stewardship's on-disk note path for open-in-editor: expanded
 * dashboards live in a folder's `_index.md`, flat ones as a single file. */
function editorPath(slug: string, variant: StewardshipDetailData["variant"]): string {
  return variant === "expanded" ? `stewardships/${slug}/_index.md` : `stewardships/${slug}.md`;
}

/** The slug of a resolved stewardship note path, for typed navigation
 * from a wikilink — `stewardships/<slug>/_index.md` or
 * `stewardships/<slug>.md`. */
function stewardshipSlugFromPath(path: string): string {
  const rest = path.replace(/^stewardships\//, "");
  if (rest.endsWith("/_index.md")) return rest.slice(0, -"/_index.md".length);
  return rest.replace(/\.md$/i, "");
}

/** Debounce a fast-changing value — used so the template-field fetch
 * fires on a settled activity, not on every keystroke. */
function useDebounced<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), delayMs);
    return () => clearTimeout(id);
  }, [value, delayMs]);
  return debounced;
}

export default function StewardshipDetail() {
  const { slug = "" } = useParams();
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["get_stewardship_detail", slug],
    queryFn: () => getStewardshipDetail(slug),
  });

  if (isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (isError) {
    return (
      <div className="p-8">
        <p className="text-ink">This stewardship could not be read.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(error)}</p>
      </div>
    );
  }

  return <StewardshipDetailBody slug={slug} data={data} />;
}

function StewardshipDetailBody({ slug, data }: { slug: string; data: StewardshipDetailData }) {
  const client = useQueryClient();
  const navigate = useNavigate();
  const { openReader } = useReader();
  const reducedMotion = usePrefersReducedMotion();
  const [search] = useSearchParams();
  const key = ["get_stewardship_detail", slug];

  const canLog = data.variant === "expanded";
  // The form used to sit below the dashboard, every chart and the recent
  // list — several screens of scrolling to reach the most frequent write
  // on the page. It is summoned from the header now, and the list's own
  // "log" link arrives with `?log=1` so logging is one click from there.
  const [logOpen, setLogOpen] = useState(search.get("log") === "1");
  // Which activities to chart. Empty means all — a filter narrows, it
  // never blanks. Ephemeral by design (#489): it resets on navigation
  // along with every other piece of state in this component, and that is
  // correct — persistent hiding is out-of-sight-out-of-mind.
  const [activities, setActivities] = useState<Set<string>>(new Set());
  // The cap's reveal state — same ephemeral treatment as the activity
  // filter above, for the same reason.
  const [showAllCharts, setShowAllCharts] = useState(false);
  const metricsOn = useMetrics();

  // A `plot = "none"` series is collected and queryable over MCP (#500)
  // but not drawn here — filtered first so it never counts towards an
  // activity's "first" series below, and never inflates the chip list
  // with an activity that has nothing left to chart.
  const drawableSeries = data.series.filter(isDrawable);
  const showCharts = data.variant === "expanded" && drawableSeries.length > 0;
  // A series is named "<activity> · <column>" (composed in
  // `cdno-domain`'s `context.rs`), and the activity is what a reader
  // filters by. A gym stewardship tracking sets, reps and weight across
  // three activities is nine charts in one column — roughly 1600px of
  // scroll with no way to narrow it.
  //
  // Split on the separator, not on whitespace: an activity is free text
  // and "morning run" is an ordinary thing to type. Splitting on a space
  // labelled the chip "morning", and two activities sharing a first word
  // collapsed into one chip — which took the filter away entirely, since
  // it only appears when there is more than one.
  const chartActivities = [...new Set(drawableSeries.map((s) => activityOf(s.name)))];
  const activityFilteredSeries =
    activities.size === 0
      ? drawableSeries
      : drawableSeries.filter((s) => activities.has(activityOf(s.name)));
  // #489: at most one series per activity is always visible; every further
  // series for that same activity needs metrics on. Then cap what survives
  // — "show all" reveals the rest without a second round-trip, by simply
  // raising the cap to the full length.
  const gatedSeries = metricsGatedSeries(activityFilteredSeries, metricsOn);
  const { shown: shownSeries, hiddenCount } = capSeries(
    gatedSeries,
    showAllCharts ? gatedSeries.length : DEFAULT_CHART_CAP,
  );
  // Wikilinks in the dashboard body resolve to typed navigation or open
  // the linked note in the shell reader (mirrors ProjectDetail).
  async function onWikilink(target: string) {
    let resolved;
    try {
      resolved = await resolveWikilink(target);
    } catch {
      return;
    }
    if (!resolved) return;
    if (resolved.note_type === "project") {
      navigate(`/projects/${resolved.path.split("/").pop()?.replace(/\.md$/i, "")}`);
    } else if (resolved.note_type === "stewardship") {
      navigate(`/stewardships/${stewardshipSlugFromPath(resolved.path)}`);
    } else {
      openReader(resolved.path);
    }
  }

  return (
    <div className="mx-auto max-w-3xl p-8">
      <header className="flex items-center gap-3">
        <span
          aria-hidden
          className={`h-3 w-3 shrink-0 rounded-full ${contextDotClass(data.context)}`}
        />
        <h1 className="min-w-0 flex-1 truncate text-xl font-semibold text-ink">
          {data.name || slug}
        </h1>
        <span className="shrink-0 rounded bg-bg-sunken px-2 py-0.5 text-xs text-ink-muted">
          {data.variant}
        </span>
        {canLog && (
          <button
            type="button"
            onClick={() => setLogOpen((open) => !open)}
            aria-expanded={logOpen}
            className="shrink-0 rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
          >
            {logOpen ? "Close log" : "Log entry"}
          </button>
        )}
        <button
          type="button"
          onClick={() => void openInEditor(editorPath(slug, data.variant))}
          className="shrink-0 rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
        >
          Open in editor
        </button>
      </header>

      {canLog && logOpen && (
        <LogEntry
          slug={slug}
          recentActivities={data.recent.map((e) => e.activity)}
          // Invalidation rides `onSettled`; closing does NOT. Closing
          // from there unmounted the form on a *failed* submit too, and
          // the form holds the only copy of what was typed — so a disk
          // error or a vault lock silently ate the entry.
          onLogged={() => void client.invalidateQueries({ queryKey: key })}
          onClose={() => setLogOpen(false)}
        />
      )}

      {/* Dashboard — the body as written. */}
      {/* Capped: a long body used to push Trends and Recent off-screen
          entirely. Expands in place. */}
      <section aria-label="Dashboard" className="mt-8">
        <ClampedText collapsedClass="max-h-96" resetKey={slug}>
          <Markdown body={data.body_markdown} onWikilink={onWikilink} />
        </ClampedText>
      </section>

      {/* Trend charts — expanded-only, and only when there's numeric
          tracking to draw. Status visualisations, not goal trackers. */}
      {showCharts && (
        <section aria-label="Trends" className="mt-10 border-t border-line pt-6">
          <div className="flex flex-wrap items-center gap-2">
            <SectionHeading>Trends</SectionHeading>
            <span className="text-xs text-ink-faint">status, not targets</span>
            {chartActivities.length > 1 && (
              <div role="group" aria-label="Filter charts by activity" className="ml-auto flex flex-wrap gap-1.5">
                {chartActivities.map((activity) => (
                  <button
                    key={activity}
                    type="button"
                    aria-pressed={activities.has(activity)}
                    onClick={() =>
                      setActivities((current) => {
                        const next = new Set(current);
                        if (!next.delete(activity)) next.add(activity);
                        return next;
                      })
                    }
                    className={`rounded-full border border-line px-2 py-0.5 text-xs ${
                      activities.has(activity)
                        ? "bg-bg-sunken font-medium text-ink"
                        : "text-ink-muted hover:text-ink"
                    }`}
                  >
                    {activity}
                  </button>
                ))}
              </div>
            )}
          </div>
          {/* Two up rather than one long column. */}
          <div
            role="group"
            aria-label="Trend charts"
            className="mt-3 grid grid-cols-1 gap-6 lg:grid-cols-2"
          >
            {shownSeries.map((series, index) => (
              // Count/volume series (all-integer values — reps, laps,
              // sessions) read better as calm columns; continuous
              // measures keep the line. The choice is cosmetic.
              <TrendChart
                key={series.name}
                series={series}
                color={SERIES_COLORS[index % SERIES_COLORS.length]}
                animate={!reducedMotion}
                kind={markForSeries(series)}
              />
            ))}
          </div>
          {/* Explicit reveal for what the cap held back (#489). One toggle,
              not a pair: `hiddenCount` is only ever nonzero while
              collapsed, since "show all" raises the cap to the full
              length — so this and the collapsed count never disagree. The
              label states what it is hiding rather than leaving that to be
              discovered. */}
          {(hiddenCount > 0 || (showAllCharts && gatedSeries.length > DEFAULT_CHART_CAP)) && (
            <button
              type="button"
              onClick={() => setShowAllCharts((shown) => !shown)}
              aria-expanded={showAllCharts}
              className="mt-3 rounded border border-line px-3 py-1 text-xs text-ink-muted hover:text-ink"
            >
              {showAllCharts
                ? "Show fewer charts"
                : `Show ${hiddenCount} more ${hiddenCount === 1 ? "chart" : "charts"}`}
            </button>
          )}
        </section>
      )}

      {/* Recent tracking — last few entries, opening the note reader. */}
      {canLog && (
        <section aria-label="Recent tracking" className="mt-10 border-t border-line pt-6">
          <div className="flex flex-wrap items-baseline gap-2">
            <SectionHeading>Recent tracking</SectionHeading>
            {/* The honest total behind the previewed few. It came over
                the wire and nothing read it, so the page showed five
                entries and you could not tell whether there were six or
                six hundred. */}
            <span className="text-xs text-ink-faint">
              {data.tracking_count} in all
            </span>
            {data.tracking_count > data.recent.length && (
              <button
                type="button"
                onClick={() => void openInEditor(`stewardships/${slug}/tracking`)}
                className="text-xs text-ink-faint underline decoration-dotted underline-offset-2 hover:text-ink"
              >
                see all
              </button>
            )}
          </div>
          {data.recent.length === 0 ? (
            // Not a prompt to catch up: an expanded stewardship with no
            // tracking yet is a perfectly good state to be in.
            <p className="mt-3 text-sm text-ink-muted">
              Nothing tracked yet. Log an entry when there is something to record.
            </p>
          ) : (
          <ul className="mt-3 space-y-1">
            {data.recent.map((entry) => (
              <li key={entry.path}>
                <button
                  type="button"
                  onClick={() => openReader(entry.path)}
                  className="flex w-full items-baseline gap-2 rounded border border-line bg-bg-surface px-3 py-2 text-left hover:bg-bg-sunken"
                >
                  <span className="shrink-0 text-sm text-ink">{entry.activity}</span>
                  <span className="shrink-0 text-xs text-ink-faint">{shortDate(entry.date)}</span>
                  {entry.routine && (
                    <span className="shrink-0 text-xs text-ink-faint">{entry.routine}</span>
                  )}
                  {entry.duration_min !== null && (
                    <span className="shrink-0 text-xs text-ink-faint">
                      {entry.duration_min} min
                    </span>
                  )}
                  {entry.body_excerpt && (
                    <span className="min-w-0 flex-1 truncate text-xs text-ink-muted">
                      {entry.body_excerpt}
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>
          )}
        </section>
      )}

      <p className="mt-8 text-xs text-ink-faint">
        <Link to="/stewardships" className="hover:text-ink-muted">
          ← all stewardships
        </Link>
      </p>
    </div>
  );
}

/** The inline (not modal) log form: activity (with a datalist of recent
 * activities), optional routine, content, plus template-derived dynamic
 * fields fetched for the typed activity. */
function LogEntry({
  slug,
  recentActivities,
  onLogged,
  onClose,
}: {
  slug: string;
  recentActivities: string[];
  /** Settled — success or failure. Refresh what the write may have
   * changed; never close over it. */
  onLogged: () => void;
  /** Close the form: the header owns that state now. Called on a
   * successful save and on Cancel, never on a failure — the draft is
   * the only copy of what was typed. */
  onClose: () => void;
}) {
  const { toast } = useToast();
  const [activity, setActivity] = useState("");
  const [routine, setRoutine] = useState("");
  const [content, setContent] = useState("");
  const [vars, setVars] = useState<Record<string, string>>({});

  const debouncedActivity = useDebounced(activity.trim(), 300);
  const fields = useQuery({
    queryKey: ["get_tracking_template_fields", debouncedActivity],
    queryFn: () => getTrackingTemplateFields(debouncedActivity),
    enabled: debouncedActivity.length > 0,
  });

  const activityOptions = useMemo(
    () => Array.from(new Set(recentActivities)).filter(Boolean),
    [recentActivities],
  );

  const templateFields = fields.data ?? [];

  // Each activity has its own template fields, which unmount and refetch
  // on switch. Reset the collected vars whenever the (debounced)
  // activity settles on a new value so a value typed for activity A can
  // never linger into activity B's note.
  useEffect(() => {
    setVars({});
  }, [debouncedActivity]);

  function reset() {
    setActivity("");
    setRoutine("");
    setContent("");
    setVars({});
  }

  const submit = useMutation({
    mutationFn: () => {
      // Belt and braces alongside the reset-on-switch above: submit only
      // vars whose keys belong to the CURRENT activity's template, so an
      // orphaned key can never ride into the note.
      const names = new Set(templateFields.map((f) => f.name));
      const scopedVars = Object.fromEntries(
        Object.entries(vars).filter(([name]) => names.has(name)),
      );
      return logTrackingEntry(
        slug,
        activity.trim(),
        content,
        scopedVars,
        routine.trim() || undefined,
      );
    },
    onError: (err) => toast(errorMessage(err), "attention"),
    onSuccess: () => {
      toast(`Logged ${activity.trim()} — one more on the record.`);
      reset();
      onClose();
    },
    onSettled: onLogged,
  });

  // No collapsed state of its own: the header owns whether this is open,
  // so one control decides rather than two that can disagree. It used to
  // render its own "Log entry" button at the very bottom of the page.
  return (
    <section aria-label="Log a tracking entry" className="mt-6 rounded-lg border border-line bg-bg-surface p-4">
      <SectionHeading>Log entry</SectionHeading>
      <form
        className="mt-3 space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          // Guard against a double-submit (fast second click / Enter
          // before the mutation settles).
          if (activity.trim() && !submit.isPending) submit.mutate();
        }}
      >
        <div>
          <label htmlFor="log-activity" className="block text-xs text-ink-muted">
            Activity
          </label>
          <input
            id="log-activity"
            list="log-activity-options"
            value={activity}
            onChange={(event) => setActivity(event.target.value)}
            placeholder="gym, swim, weigh-in…"
            className="mt-1 w-full rounded border border-line bg-bg-base px-2 py-1 text-sm text-ink"
          />
          <datalist id="log-activity-options">
            {activityOptions.map((option) => (
              <option key={option} value={option} />
            ))}
          </datalist>
        </div>

        <div>
          <label htmlFor="log-routine" className="block text-xs text-ink-muted">
            Routine (optional)
          </label>
          <input
            id="log-routine"
            value={routine}
            onChange={(event) => setRoutine(event.target.value)}
            placeholder="upper-body-a"
            className="mt-1 w-full rounded border border-line bg-bg-base px-2 py-1 text-sm text-ink"
          />
        </div>

        {/* Template-derived fields for the typed activity. */}
        {templateFields.map((field) => (
          <div key={field.name}>
            <label htmlFor={`log-var-${field.name}`} className="block text-xs text-ink-muted">
              {field.prompt || field.name}
            </label>
            <input
              id={`log-var-${field.name}`}
              value={vars[field.name] ?? ""}
              onChange={(event) =>
                setVars((prev) => ({ ...prev, [field.name]: event.target.value }))
              }
              className="mt-1 w-full rounded border border-line bg-bg-base px-2 py-1 text-sm text-ink"
            />
          </div>
        ))}

        <div>
          <label htmlFor="log-content" className="block text-xs text-ink-muted">
            Notes
          </label>
          <textarea
            id="log-content"
            value={content}
            onChange={(event) => setContent(event.target.value)}
            rows={3}
            className="mt-1 w-full rounded border border-line bg-bg-base p-2 text-sm text-ink"
          />
        </div>

        <div className="flex gap-2">
          <button
            type="submit"
            disabled={submit.isPending || !activity.trim()}
            className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken disabled:opacity-50"
          >
            Log it
          </button>
          <button
            type="button"
            onClick={() => {
              reset();
              onClose();
            }}
            className="rounded px-3 py-1 text-sm text-ink-muted hover:text-ink"
          >
            Cancel
          </button>
        </div>
      </form>
    </section>
  );
}
