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
  collapsedItems,
  limit = 5,
  children,
  label,
}: {
  items: ReactNode[];
  /** What to show while collapsed, when that is not simply the first
   * `limit` of `items`.
   *
   * Which items to show and which order to show them in are different
   * questions. A log list ordered oldest-first still wants its *recent*
   * entries in the collapsed summary, and taking a prefix would show the
   * oldest — so the caller picks the subset and this renders it. */
  collapsedItems?: ReactNode[];
  /** How many to show collapsed, when `collapsedItems` is not given. */
  limit?: number;
  /** Optional trailing content, rendered inside the list container. */
  children?: ReactNode;
  /** What the items are, for the toggle's accessible name ("show all 23
   * backlinks"). Keeps the control meaningful when several capped lists
   * sit on one page. */
  label: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const collapsed = collapsedItems ?? items.slice(0, limit);
  const overflows = items.length > collapsed.length;
  const shown = expanded || !overflows ? items : collapsed;

  return (
    <div>
      {expanded && overflows ? (
        // Focusable so a keyboard user can arrow-scroll it (axe
        // scrollable-region-focusable). The items here are often plain
        // prose — a log card carries no button or link — so without this
        // the content below the fold is simply unreachable without a mouse.
        <div
          tabIndex={0}
          role="group"
          aria-label={label}
          className="max-h-80 overflow-y-auto pr-1"
        >
          {shown}
          {children}
        </div>
      ) : (
        // Short or collapsed: no inner scroller, so no trapped wheel
        // gesture and no extra tab stop for a list that does not need one.
        <div>
          {shown}
          {children}
        </div>
      )}
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
