// The Commitments view (plan §1.3, #56; reworked in #447).
//
// Commitments are the one thing in the vault with real external dates
// attached — a todo is something you decided to do, a commitment is
// something someone else is counting on. The question this view is asked
// is spatial: what is bearing down on me, and when. It answered with a
// flat list bucketed by month, which answers only if you read every line.
//
// So: a stated horizon rather than a hardcoded ninety days, a month view
// beside the timeline for the same data read spatially, near-term banding
// inside the timeline (that lives in the shared component, since the
// weekly and monthly reviews consume it too), and filter chips that carry
// their counts and can be cleared in one move.
//
// Emptiness is success — no illustration, no prompt to add.
import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import type { CommitmentEntry } from "../../api/bindings/CommitmentEntry";
import { getCommitments } from "../../api/commands";
import CommitmentsTimeline from "../../components/commitments/CommitmentsTimeline";
import MonthGrid from "../calendar/MonthGrid";
import { shortDate } from "../../lib/dates";
import type { Context } from "../../lib/contexts";
import { CONTEXTS, contextDotClass, contextLabel } from "../../lib/contexts";

/** How far ahead to look, and what to call it.
 *
 * The window used to be a hardcoded 90 days, which silently defined what
 * "all your commitments" meant: a promise 100 days out was invisible with
 * no hint that it existed. Now it is stated, and "everything" is
 * reachable. */
const HORIZONS: { label: string; days: number; empty: string }[] = [
  { label: "2 weeks", days: 14, empty: "in the next 2 weeks" },
  { label: "6 weeks", days: 42, empty: "in the next 6 weeks" },
  { label: "3 months", days: 90, empty: "in the next 3 months" },
  { label: "6 months", days: 182, empty: "in the next 6 months" },
  // Ten years stands in for "all": the backend takes a day count, and no
  // vault holds a promise past it. Its empty phrasing is its own — "in
  // the next everything" is not a sentence.
  { label: "everything", days: 3650, empty: "at any date you have recorded" },
];

/** 3 months — the window this view has always had, now stated. */
const DEFAULT_HORIZON = 2;

type ViewMode = "timeline" | "month";

export default function Commitments() {
  const [horizon, setHorizon] = useState(DEFAULT_HORIZON);
  const days = HORIZONS[horizon].days;
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["get_commitments", days],
    queryFn: () => getCommitments(days),
    // Keep the window you were reading on screen while the new one
    // loads. The horizon is part of the query key, so without this every
    // change to it drops the view to "Reading the vault…" — taking the
    // control you just used with it.
    placeholderData: (previous) => previous,
  });
  // Multiple contexts can be active at once; an empty set means "all".
  const [filter, setFilter] = useState<Set<Context>>(new Set());
  const [mode, setMode] = useState<ViewMode>("timeline");

  function toggle(context: Context) {
    setFilter((current) => {
      const next = new Set(current);
      if (!next.delete(context)) next.add(context);
      return next;
    });
  }

  if (isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (isError) {
    return (
      <div className="p-8">
        <p className="text-ink">The vault could not be read.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(error)}</p>
      </div>
    );
  }

  const entries = data.entries;
  const counts = new Map<Context, number>();
  for (const entry of entries) counts.set(entry.context, (counts.get(entry.context) ?? 0) + 1);
  // Only the contexts that actually promised something get a chip — seven
  // of which five read zero is a wall, not a filter.
  const present = CONTEXTS.filter((c) => (counts.get(c) ?? 0) > 0);

  return (
    <div className="mx-auto max-w-4xl p-8">
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <h1 className="text-xl font-semibold text-ink">Commitments</h1>
        <p className="text-xs text-ink-faint">
          What someone else is counting on — not what you decided to do.
        </p>
      </div>

      <div className="mt-4 flex flex-wrap items-center gap-2">
        <label htmlFor="horizon" className="text-xs text-ink-faint">
          Looking ahead
        </label>
        <select
          id="horizon"
          value={horizon}
          onChange={(event) => setHorizon(Number(event.target.value))}
          className="rounded border border-line bg-bg-surface px-2 py-1 text-xs text-ink"
        >
          {HORIZONS.map((h, index) => (
            <option key={h.label} value={index}>
              {h.label}
            </option>
          ))}
        </select>

        <div
          role="group"
          aria-label="View"
          className="ml-auto flex gap-0.5 rounded-md bg-bg-sunken p-0.5"
        >
          {(["timeline", "month"] as ViewMode[]).map((value) => (
            <button
              key={value}
              type="button"
              aria-pressed={mode === value}
              onClick={() => setMode(value)}
              className={`rounded px-2.5 py-1 text-xs ${
                mode === value
                  ? "bg-bg-surface font-medium text-ink shadow-sm"
                  : "text-ink-muted hover:text-ink"
              }`}
            >
              {value === "timeline" ? "Timeline" : "Month"}
            </button>
          ))}
        </div>
      </div>

      {present.length > 0 && (
        <div
          role="group"
          aria-label="Filter by context"
          className="mt-3 flex flex-wrap items-center gap-2"
        >
          {present.map((context) => {
            const active = filter.has(context);
            const count = counts.get(context) ?? 0;
            return (
              <button
                key={context}
                type="button"
                aria-pressed={active}
                // The count rides the name as well as the pixels.
                aria-label={`${contextLabel(context)}, ${count}`}
                onClick={() => toggle(context)}
                className={`flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs ${
                  active
                    ? "border-line bg-bg-sunken font-medium text-ink"
                    : "border-line text-ink-muted hover:text-ink"
                }`}
              >
                <span
                  aria-hidden
                  className={`h-2 w-2 shrink-0 rounded-full ${contextDotClass(context)}`}
                />
                <span aria-hidden>{contextLabel(context)}</span>
                <span aria-hidden className="text-ink-faint">
                  {count}
                </span>
              </button>
            );
          })}
          {/* Clearing used to mean un-toggling each chip you had set, with
              a sentence for a recovery hint rather than a control. */}
          {filter.size > 0 && (
            <button
              type="button"
              onClick={() => setFilter(new Set())}
              className="rounded text-xs text-ink-faint underline decoration-dotted underline-offset-2 hover:text-ink"
            >
              Clear
            </button>
          )}
        </div>
      )}

      {entries.length === 0 ? (
        <p className="mt-8 rounded border border-line bg-bg-surface p-6 text-ink-muted">
          Nothing promised {HORIZONS[horizon].empty}.
        </p>
      ) : mode === "month" ? (
        <MonthView entries={entries} today={data.today} filter={filter} />
      ) : (
        <CommitmentsTimeline
          entries={entries}
          today={data.today}
          filter={filter}
          monthHeading="h2"
        />
      )}
    </div>
  );
}

/** The same promises, read spatially: a month grid with one
 * context-coloured dot per commitment falling on a day, and the chosen
 * day's entries listed beneath.
 *
 * Is the next fortnight clear, or is there a wall — a list answers that
 * only if you read every line. */
function MonthView({
  entries,
  today,
  filter,
}: {
  entries: CommitmentEntry[];
  today: string;
  filter: Set<Context>;
}) {
  const [year, setYear] = useState(() => Number(today.split("-")[0]));
  const [month, setMonth] = useState(() => Number(today.split("-")[1]));
  const [selected, setSelected] = useState<string | null>(null);

  const visible = filter.size === 0 ? entries : entries.filter((e) => filter.has(e.context));

  const marks = useMemo(() => {
    const byDay = new Map<string, string[]>();
    for (const entry of visible) {
      const dots = byDay.get(entry.date) ?? [];
      dots.push(contextDotClass(entry.context));
      byDay.set(entry.date, dots);
    }
    return byDay;
  }, [visible]);

  function pageMonth(delta: number) {
    const next = month + delta;
    if (next < 1) {
      setMonth(12);
      setYear(year - 1);
    } else if (next > 12) {
      setMonth(1);
      setYear(year + 1);
    } else {
      setMonth(next);
    }
  }

  const [selYear, selMonth, selDay] = (selected ?? "").split("-").map(Number);
  const selectedDayInView = selected && selYear === year && selMonth === month ? selDay : null;
  const forSelected = selected ? visible.filter((e) => e.date === selected) : [];

  const label = new Date(year, month - 1, 1).toLocaleDateString(undefined, {
    month: "long",
    year: "numeric",
  });

  return (
    <div className="mt-6">
      <div className="rounded-lg border border-line bg-bg-surface p-4">
        <div className="mb-3 flex items-center justify-between">
          <button
            type="button"
            onClick={() => pageMonth(-1)}
            aria-label="Previous month"
            className="rounded border border-line px-2 py-1 text-sm text-ink-muted hover:text-ink"
          >
            ‹
          </button>
          <span className="text-sm font-medium text-ink">{label}</span>
          <button
            type="button"
            onClick={() => pageMonth(1)}
            aria-label="Next month"
            className="rounded border border-line px-2 py-1 text-sm text-ink-muted hover:text-ink"
          >
            ›
          </button>
        </div>
        <MonthGrid
          year={year}
          month={month}
          marks={marks}
          markLabel="has a commitment"
          selectedDay={selectedDayInView}
          today={today}
          onSelectDay={setSelected}
        />
      </div>

      {selected && (
        <section aria-label="Commitments on the chosen day" className="mt-4">
          <h2 className="text-sm font-medium text-ink">{shortDate(selected)}</h2>
          {forSelected.length === 0 ? (
            <p className="mt-2 text-sm text-ink-muted">Nothing promised on this day.</p>
          ) : (
            <ul className="mt-2 space-y-2">
              {forSelected.map((entry) => (
                <li
                  key={`${entry.source.kind}|${entry.source.slug}|${entry.title}`}
                  className="flex items-center gap-3 rounded-md border border-line bg-bg-surface px-3 py-2"
                >
                  <span
                    aria-hidden
                    className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(entry.context)}`}
                  />
                  <span className="min-w-0 flex-1 truncate text-sm text-ink">{entry.title}</span>
                  <span className="shrink-0 text-xs text-ink-faint">{entry.source.slug}</span>
                </li>
              ))}
            </ul>
          )}
        </section>
      )}
    </div>
  );
}
