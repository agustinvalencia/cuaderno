// The shared Commitments Timeline (plan §1.3). A strictly
// chronological vertical list: past-due entries collapsed into a
// calm "a few slipped past" group at the top (never expanded on
// load), upcoming entries below grouped under month headers. Colour
// signals context, never urgency — past-due earns only a secondary
// desaturated-amber accent (a thin left border and a "planned for …"
// date label), never red, never the word "overdue".
//
// M6's 14-day lookahead and M9's 6-week view reuse this component, so
// it takes its data and filter as props and owns no fetching.
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router";
import type { CommitmentEntry } from "../../api/bindings/CommitmentEntry";
import type { CommitmentSource } from "../../api/bindings/CommitmentSource";
import type { CommitmentsView } from "../../api/bindings/CommitmentsView";
import type { Context } from "../../lib/contexts";
import { completeCommitment, completeMilestone, errorMessage } from "../../api/commands";
import { contextDotClass } from "../../lib/contexts";
import { useReader } from "../../shell/reader";
import { useToast } from "../../shell/Toasts";

/** `8 Jul` / `Jul 8` per locale. Parsed at local midnight so the day
 * never slips a timezone. Mirrors Home's helper. */
function shortDate(date: string): string {
  return new Date(`${date}T00:00:00`).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
  });
}

/** A stable identity for an entry, for optimistic removal and React
 * keys — the tuple has no id, so compose one from its fields. */
function entryKey(entry: CommitmentEntry): string {
  return `${entry.date}|${entry.source.kind}|${entry.source.slug}|${entry.title}`;
}

/** Month bucket header, e.g. "July" — or "January 2027" when the
 * window crosses into another year, so a Dec→Jan span stays legible. */
function monthLabel(date: string, thisYear: number): string {
  const d = new Date(`${date}T00:00:00`);
  const opts: Intl.DateTimeFormatOptions =
    d.getFullYear() === thisYear ? { month: "long" } : { month: "long", year: "numeric" };
  return d.toLocaleDateString(undefined, opts);
}

/** The origin chip: names the source and links to where it lives.
 * Project-backed sources go to the project detail; stewardship to the
 * stewardships list; a standalone commitment opens its note in the
 * shell reader (the reader landed in M5). */
function OriginChip({ source }: { source: CommitmentSource }) {
  const { openReader } = useReader();
  const cls = "text-xs text-ink-faint hover:text-ink-muted";
  switch (source.kind) {
    case "project_milestone":
    case "action_note":
      return (
        <Link to={`/projects/${source.slug}`} className={cls}>
          {source.slug}
        </Link>
      );
    case "stewardship":
      return (
        <Link to="/stewardships" className={cls}>
          {source.slug}
        </Link>
      );
    case "standalone_commitment":
      return (
        <button
          type="button"
          onClick={() => openReader(`commitments/${source.slug}.md`)}
          className={cls}
        >
          {source.slug}
        </button>
      );
  }
}

/** The done button, present only for sources completable from here:
 * standalone commitments and project milestones. Periodic (stewardship)
 * commitments show their cadence, not a checkbox; action notes complete
 * from Home. Returns the invoke for a completable source, else null. */
function completionFor(entry: CommitmentEntry): (() => Promise<void>) | null {
  switch (entry.source.kind) {
    case "standalone_commitment":
      return () => completeCommitment(entry.source.slug);
    case "project_milestone":
      return () => completeMilestone(entry.source.slug, entry.title);
    default:
      return null;
  }
}

function removeEntry(view: CommitmentsView | undefined, key: string): CommitmentsView | undefined {
  return view ? { ...view, entries: view.entries.filter((e) => entryKey(e) !== key) } : view;
}

function TimelineRow({ entry, readOnly }: { entry: CommitmentEntry; readOnly: boolean }) {
  const client = useQueryClient();
  const { toast } = useToast();
  const key = entryKey(entry);
  // A read-only host (the Weekly Review's lookahead — "nothing to add
  // here") suppresses completion entirely; origin chips stay.
  const invoke = readOnly ? null : completionFor(entry);

  // Optimistic removal across every cached lookahead window (the key
  // carries the days arg, so filter by prefix). Rolls back on error.
  const complete = useMutation({
    mutationFn: () => invoke!(),
    onMutate: async () => {
      await client.cancelQueries({ queryKey: ["get_commitments"] });
      const snapshots = client.getQueriesData<CommitmentsView>({ queryKey: ["get_commitments"] });
      client.setQueriesData<CommitmentsView>({ queryKey: ["get_commitments"] }, (view) =>
        removeEntry(view, key),
      );
      return { snapshots };
    },
    onError: (error, _vars, context) => {
      context?.snapshots.forEach(([qk, data]) => client.setQueryData(qk, data));
      toast(errorMessage(error), "attention");
    },
    onSuccess: () => toast(`Done: ${entry.title}.`),
    onSettled: () => client.invalidateQueries({ queryKey: ["get_commitments"] }),
  });

  return (
    <li
      className={`flex items-center gap-3 rounded-md border border-line bg-bg-surface px-3 py-2 ${
        // Past-due: a thin desaturated-amber left border, the only
        // accent it earns. The context hue stays on the dot.
        entry.is_overdue ? "border-l-2 border-l-attention" : ""
      }`}
    >
      <span
        aria-hidden
        className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(entry.context)}`}
      />
      <span className="min-w-0 flex-1 truncate text-sm text-ink">{entry.title}</span>
      <OriginChip source={entry.source} />
      <span className="shrink-0 text-xs text-ink-muted">
        {entry.is_overdue ? `planned for ${shortDate(entry.date)}` : shortDate(entry.date)}
      </span>
      {invoke && (
        <button
          type="button"
          onClick={() => complete.mutate()}
          disabled={complete.isPending}
          aria-label={`Mark done: ${entry.title}`}
          className="shrink-0 rounded px-2 py-0.5 text-xs text-ink-muted hover:text-ink"
        >
          done
        </button>
      )}
    </li>
  );
}

export default function CommitmentsTimeline({
  entries,
  today,
  filter,
  readOnly = false,
  monthHeading: MonthHeading = "h3",
}: {
  entries: CommitmentEntry[];
  today: string;
  filter: Set<Context>;
  /** Suppress the done buttons (origin chips remain) — for hosts that
   * present the timeline as a pure look, like the review's lookahead. */
  readOnly?: boolean;
  /** Month-header element. Heading levels must not skip (axe
   * heading-order): hosts nesting the timeline under an h2 section
   * keep the h3 default; the Commitments view sits right under its h1
   * and passes "h2". */
  monthHeading?: "h2" | "h3";
}) {
  // Empty active set means "all contexts" — a filter narrows, never
  // blanks by default.
  const visible = filter.size === 0 ? entries : entries.filter((e) => filter.has(e.context));

  // Past-due entries (date < today) collapse into the top affordance;
  // upcoming entries flow below, grouped by month. Both arms preserve
  // the incoming chronological order (the backend sorts by date).
  const past = visible.filter((e) => e.is_overdue);
  const upcoming = visible.filter((e) => !e.is_overdue);

  const thisYear = new Date(`${today}T00:00:00`).getFullYear();
  const months: { label: string; entries: CommitmentEntry[] }[] = [];
  for (const entry of upcoming) {
    const label = monthLabel(entry.date, thisYear);
    const bucket = months.at(-1);
    if (bucket && bucket.label === label) {
      bucket.entries.push(entry);
    } else {
      months.push({ label, entries: [entry] });
    }
  }

  return (
    <div className="mt-6 space-y-6">
      {past.length > 0 && (
        // Collapsed by default (no `open` attribute) — the sanctioned
        // deviation from strict inline position (plan §1.3): grouped if
        // wanted, never a guilt-list on load.
        <details className="rounded-md border border-line bg-bg-sunken px-3 py-2">
          <summary className="cursor-pointer text-sm text-ink-muted">
            a few slipped past
            <span className="ml-2 rounded bg-bg-surface px-1.5 py-0.5 text-xs text-ink-faint">
              {past.length}
            </span>
          </summary>
          <ul className="mt-3 space-y-2">
            {past.map((entry) => (
              <TimelineRow key={entryKey(entry)} entry={entry} readOnly={readOnly} />
            ))}
          </ul>
        </details>
      )}

      {visible.length === 0 && entries.length > 0 && (
        <p className="text-sm text-ink-muted">
          Nothing in these contexts. Clear the filter to see everything.
        </p>
      )}

      {months.map((month) => (
        <section key={month.label} aria-label={month.label}>
          <MonthHeading className="text-xs font-medium uppercase tracking-wider text-ink-faint">
            {month.label}
          </MonthHeading>
          <ul className="mt-2 space-y-2">
            {month.entries.map((entry) => (
              <TimelineRow key={entryKey(entry)} entry={entry} readOnly={readOnly} />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
