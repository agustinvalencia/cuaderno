// Shows the first few items and keeps the rest one click away (#441).
//
// Some sections grow for as long as the thing they describe lives: a
// project's backlinks and its log mentions both accumulate indefinitely,
// and once they run to dozens they push the sections a reader came for —
// the state, the next actions, the blockers — off the screen entirely.
//
// Expansion happens in place rather than sending the reader elsewhere, and
// the collapsed state names the true total, so nothing is hidden, only
// deferred. Once expanded the list gets its own bounded scroll: a hundred
// items should not restore the problem the cap was solving.
import { useState, type ReactNode } from "react";

export function CappedList({
  items,
  limit = 5,
  children,
  label,
}: {
  items: ReactNode[];
  /** How many to show collapsed. */
  limit?: number;
  /** Optional trailing content, rendered inside the list container. */
  children?: ReactNode;
  /** What the items are, for the toggle's accessible name ("show all 23
   * backlinks"). Keeps the control meaningful when several capped lists
   * sit on one page. */
  label: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const overflows = items.length > limit;
  const shown = expanded || !overflows ? items : items.slice(0, limit);

  return (
    <div>
      <div
        className={
          // Only the expanded, genuinely-long case gets an inner scroller,
          // so short lists never trap a wheel gesture or add a tab stop.
          expanded && overflows ? "max-h-80 overflow-y-auto pr-1" : undefined
        }
      >
        {shown}
        {children}
      </div>
      {overflows && (
        <button
          type="button"
          onClick={() => setExpanded((value) => !value)}
          aria-expanded={expanded}
          className="mt-1.5 rounded text-xs text-ink-faint hover:text-ink"
        >
          {expanded ? `Show fewer ${label}` : `Show all ${items.length} ${label}`}
        </button>
      )}
    </div>
  );
}
