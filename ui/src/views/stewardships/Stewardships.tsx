// Stewardship list (M7, plan §1.7; reworked in #448) — the calm index
// behind `/stewardships`.
//
// Stewardships are RLM's perpetual responsibilities, deliberately kept
// out of the five project slots because they never complete. The method's
// claim about them is that they should cost almost nothing to keep and be
// visible enough that a lapse is noticed without being nagged about.
//
// This list worked against the second half. It was alphabetical only, so
// the one number that matters — how long since anything was tracked —
// sat wherever the alphabet happened to put it, and every row painted
// flat `text-ink-faint`, so the neutral freshness ladder the rest of the
// app reads by was absent exactly where freshness is the whole signal.
//
// Status, never progress: nothing here has a bar, a percentage, or a
// target. A lapse recedes into ink; it never turns a colour.
import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import type { StewardshipSummary } from "../../api/bindings/StewardshipSummary";
import { listStewardships } from "../../api/commands";
import { contextDotClass } from "../../lib/contexts";
import { STALE_AFTER_DAYS, stalenessAgo, stalenessTone } from "../../lib/staleness";

/** How long since this one was last tracked, for sorting. A flat
 * stewardship has no tracking by design and sorts as fresh; an expanded
 * one that has never been tracked sorts as the stalest thing there is. */
function stalenessOf(s: StewardshipSummary): number {
  if (s.variant === "flat") return -1;
  return s.staleness_days ?? Number.MAX_SAFE_INTEGER;
}

/** Has this one gone quiet? Only expanded stewardships can — a flat one
 * is a dashboard with nothing to track, not a neglected habit.
 *
 * "Lapsed" is the shared ladder's long-dormant tier, so the word and the
 * ink agree rather than each having their own threshold. */
export function isLapsed(s: StewardshipSummary): boolean {
  if (s.variant !== "expanded") return false;
  return s.staleness_days === null || s.staleness_days > STALE_AFTER_DAYS;
}

/** The muted status line: tracking volume and how fresh it is, or a
 * gentle "no tracking yet" for an expanded stewardship with none /
 * "dashboard only" for a flat one. */
function statusLine(s: StewardshipSummary): string {
  if (s.last_tracking_date === null) {
    return s.variant === "expanded" ? "no tracking yet" : "dashboard only";
  }
  const days = s.staleness_days;
  const freshness = days === null ? "" : ` \u00b7 last tracked ${stalenessAgo(days)}`;
  return `${s.tracking_count} tracked${freshness}`;
}

type Sort = "quiet" | "name";

export default function Stewardships() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["list_stewardships"],
    queryFn: listStewardships,
  });
  const [sort, setSort] = useState<Sort>("quiet");
  const [lapsedOnly, setLapsedOnly] = useState(false);

  const rows = useMemo(() => {
    const all = data ?? [];
    const shown = lapsedOnly ? all.filter(isLapsed) : all;
    return [...shown].sort((a, b) =>
      sort === "name"
        ? (a.name || a.slug).localeCompare(b.name || b.slug)
        : stalenessOf(b) - stalenessOf(a),
    );
  }, [data, sort, lapsedOnly]);

  if (isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault\u2026</p>;
  }
  if (isError) {
    return (
      <div className="p-8">
        <p className="text-ink">Stewardships could not be read.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(error)}</p>
      </div>
    );
  }

  const lapsed = data.filter(isLapsed).length;

  return (
    <div className="mx-auto max-w-3xl p-8">
      <div>
        <h1 className="text-xl font-semibold text-ink">Stewardships</h1>
        <p className="mt-1 text-sm text-ink-muted">
          The perpetual responsibilities you\u2019re keeping up. They do not complete, so
          they show status, never progress.
        </p>
      </div>

      {data.length === 0 ? (
        <p className="mt-6 rounded border border-line bg-bg-surface p-6 text-ink-muted">
          Nothing to steward yet.
        </p>
      ) : (
        <>
          <div className="mt-4 flex flex-wrap items-center gap-3">
            {/* The count, and how many have gone quiet \u2014 stated once,
                plainly, rather than left to be counted down the page.
                Not behind `useMetrics()`: it is the length of a list,
                not a reading of how you are doing. */}
            <p className="text-xs text-ink-faint">
              {data.length} stewardship{data.length === 1 ? "" : "s"}
              {lapsed > 0 && ` \u00b7 ${lapsed} gone quiet`}
            </p>

            {lapsed > 0 && (
              <button
                type="button"
                aria-pressed={lapsedOnly}
                onClick={() => setLapsedOnly((on) => !on)}
                className={`rounded-full border border-line px-2.5 py-1 text-xs ${
                  lapsedOnly ? "bg-bg-sunken font-medium text-ink" : "text-ink-muted hover:text-ink"
                }`}
              >
                Only the quiet ones
              </button>
            )}

            <label htmlFor="stewardship-sort" className="ml-auto text-xs text-ink-faint">
              Sort
            </label>
            <select
              id="stewardship-sort"
              value={sort}
              onChange={(event) => setSort(event.target.value as Sort)}
              className="rounded border border-line bg-bg-surface px-2 py-1 text-xs text-ink"
            >
              <option value="quiet">Quietest first</option>
              <option value="name">By name</option>
            </select>
          </div>

          {rows.length === 0 ? (
            <p className="mt-6 rounded border border-line bg-bg-surface p-6 text-ink-muted">
              Nothing has gone quiet. That is the whole point of them.
            </p>
          ) : (
            <ul className="mt-4 space-y-2">
              {rows.map((s) => (
                <li
                  key={s.slug}
                  className="flex items-center gap-3 rounded-md border border-line bg-bg-surface px-3 py-2"
                >
                  <span
                    aria-hidden
                    className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(s.context)}`}
                  />
                  <Link
                    to={`/stewardships/${s.slug}`}
                    className="min-w-0 flex-[3] truncate text-sm text-ink hover:underline"
                  >
                    {s.name || s.slug}
                  </Link>
                  <span className="shrink-0 rounded bg-bg-sunken px-1.5 py-0.5 text-xs text-ink-faint">
                    {s.variant}
                  </span>
                  {/* The status line truncates before the name does. It
                      used to be the other way round, so narrowing the
                      window cut the thing you identify a row by and kept
                      "12 tracked \u00b7 last tracked 3d ago" whole.
                      Its ink is the shared freshness ladder \u2014 this view
                      used to paint every row flat faint, which is the one
                      place the ladder actually carries meaning. */}
                  <span
                    className={`min-w-0 flex-1 truncate text-right text-xs ${stalenessTone(
                      s.variant === "flat" ? 0 : s.staleness_days,
                    )}`}
                  >
                    {statusLine(s)}
                  </span>
                  {/* Logging is the whole point of an expanded
                      stewardship, and it used to be a navigation plus a
                      scroll to the bottom of a page of charts. */}
                  {s.variant === "expanded" && (
                    <Link
                      to={`/stewardships/${s.slug}?log=1`}
                      aria-label={`Log an entry for ${s.name || s.slug}`}
                      className="shrink-0 rounded px-2 py-0.5 text-xs text-ink-muted hover:text-ink"
                    >
                      log
                    </Link>
                  )}
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  );
}
