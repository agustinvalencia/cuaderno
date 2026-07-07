// The Commitments Timeline view (plan §1.3, #56): the full promises
// view. Fetches the 90-day window, offers the 7-context filter chips,
// and hands the entries to the shared timeline. Emptiness is success —
// no illustration, no prompt to add.
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { getCommitments } from "../../api/commands";
import CommitmentsTimeline from "../../components/commitments/CommitmentsTimeline";
import type { Context } from "../../lib/contexts";
import { CONTEXTS, contextDotClass, contextLabel } from "../../lib/contexts";

const LOOKAHEAD_DAYS = 90;

export default function Commitments() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["get_commitments", LOOKAHEAD_DAYS],
    queryFn: () => getCommitments(LOOKAHEAD_DAYS),
  });
  // Multiple contexts can be active at once; an empty set means "all".
  const [filter, setFilter] = useState<Set<Context>>(new Set());

  function toggle(context: Context) {
    setFilter((current) => {
      const next = new Set(current);
      if (next.has(context)) {
        next.delete(context);
      } else {
        next.add(context);
      }
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

  return (
    <div className="mx-auto max-w-3xl p-8">
      <h1 className="text-xl font-semibold text-ink">Commitments</h1>

      <div role="group" aria-label="Filter by context" className="mt-4 flex flex-wrap gap-2">
        {CONTEXTS.map((context) => {
          const active = filter.has(context);
          return (
            <button
              key={context}
              type="button"
              aria-pressed={active}
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
              {contextLabel(context)}
            </button>
          );
        })}
      </div>

      {data.entries.length === 0 ? (
        <p className="mt-8 rounded border border-line bg-bg-surface p-6 text-ink-muted">
          Nothing promised in this window.
        </p>
      ) : (
        <CommitmentsTimeline entries={data.entries} today={data.today} filter={filter} />
      )}
    </div>
  );
}
