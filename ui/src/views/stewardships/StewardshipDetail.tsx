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
import { Link, useNavigate, useParams } from "react-router";
import type { StewardshipDetail as StewardshipDetailData } from "../../api/bindings/StewardshipDetail";
import {
  errorMessage,
  getStewardshipDetail,
  getTrackingTemplateFields,
  logTrackingEntry,
  openInEditor,
  resolveWikilink,
} from "../../api/commands";
import AmbiguityPicker from "../../components/ambiguity/AmbiguityPicker";
import { useAmbiguityResolver } from "../../components/ambiguity/useAmbiguityResolver";
import {
  markForSeries,
  SERIES_COLORS,
  TrendChart,
  usePrefersReducedMotion,
} from "../../components/charts/TrendChart";
import Markdown from "../../components/markdown/Markdown";
import { contextDotClass } from "../../lib/contexts";
import { useReader } from "../../shell/reader";
import { useToast } from "../../shell/Toasts";

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

/** `8 Jul` / `Jul 8` per locale, at local midnight (no timezone slip). */
function shortDate(date: string): string {
  return new Date(`${date}T00:00:00`).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
  });
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
  const key = ["get_stewardship_detail", slug];

  const showCharts = data.variant === "expanded" && data.series.length > 0;
  const canLog = data.variant === "expanded";

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
        <button
          type="button"
          onClick={() => void openInEditor(editorPath(slug, data.variant))}
          className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
        >
          Open in editor
        </button>
      </header>

      {/* Dashboard — the body as written. */}
      <section aria-label="Dashboard" className="mt-8">
        <Markdown body={data.body_markdown} onWikilink={onWikilink} />
      </section>

      {/* Trend charts — expanded-only, and only when there's numeric
          tracking to draw. Status visualisations, not goal trackers. */}
      {showCharts && (
        <section aria-label="Trends" className="mt-10 border-t border-line pt-6">
          <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">Trends</h2>
          <div className="mt-3 space-y-6">
            {data.series.map((series, index) => (
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
        </section>
      )}

      {/* Recent tracking — last few entries, opening the note reader. */}
      {data.recent.length > 0 && (
        <section aria-label="Recent tracking" className="mt-10 border-t border-line pt-6">
          <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">
            Recent tracking
          </h2>
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
                  {entry.body_excerpt && (
                    <span className="min-w-0 flex-1 truncate text-xs text-ink-muted">
                      {entry.body_excerpt}
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      {/* Log entry — expanded stewardships only (flat ones have no
          tracking/ subdir to file into). */}
      {canLog && (
        <LogEntry
          slug={slug}
          recentActivities={data.recent.map((e) => e.activity)}
          onLogged={() => void client.invalidateQueries({ queryKey: key })}
        />
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
}: {
  slug: string;
  recentActivities: string[];
  onLogged: () => void;
}) {
  const { toast } = useToast();
  const ambiguity = useAmbiguityResolver();
  const [open, setOpen] = useState(false);
  const [activity, setActivity] = useState("");
  const [routine, setRoutine] = useState("");
  const [content, setContent] = useState("");
  const [vars, setVars] = useState<Record<string, string>>({});

  const debouncedActivity = useDebounced(activity.trim(), 300);
  const fields = useQuery({
    queryKey: ["get_tracking_template_fields", debouncedActivity],
    queryFn: () => getTrackingTemplateFields(debouncedActivity),
    enabled: open && debouncedActivity.length > 0,
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
    onError: (err) => {
      // logTrackingEntry's only ambiguity is an ambiguous *slug* (a
      // stewardship existing both flat and expanded), which carries no
      // candidates — the resolver lets that fall through to the toast.
      // The wiring is here for consistency with the other write sites and
      // so any future substring-matched selector on this form gets the
      // picker for free.
      if (ambiguity.handle(err, () => submit.mutateAsync(), "entry")) return;
      toast(errorMessage(err), "attention");
    },
    onSuccess: () => {
      toast(`Logged ${activity.trim()} — one more on the record.`);
      reset();
      setOpen(false);
    },
    onSettled: onLogged,
  });

  if (!open) {
    return (
      <div className="mt-10 border-t border-line pt-6">
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
        >
          Log entry
        </button>
      </div>
    );
  }

  return (
    <section aria-label="Log a tracking entry" className="mt-10 border-t border-line pt-6">
      <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">Log entry</h2>
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
              setOpen(false);
            }}
            className="rounded px-3 py-1 text-sm text-ink-muted hover:text-ink"
          >
            Cancel
          </button>
        </div>
      </form>
      <AmbiguityPicker
        state={ambiguity.state}
        resolving={ambiguity.resolving}
        choose={ambiguity.choose}
        close={ambiguity.close}
      />
    </section>
  );
}
