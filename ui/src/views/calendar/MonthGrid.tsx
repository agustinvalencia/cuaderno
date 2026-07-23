// A from-scratch month calendar grid (#340). The app ships no date
// library by design — domain dates stay Rust-side — so this computes
// only the *layout* of a month (which weekday the 1st lands on, how many
// days it holds) with the platform `Date`, never note identities: a
// clicked cell hands its `YYYY-MM-DD` string up, and every onward jump
// (prev/next day, week, month) uses the dates the backend stamped.
//
// Seven columns, Monday-first (matching the vault's Monday-keyed weeks).
// Days that already have a daily note carry a subtle dot — calm, never
// red, honouring the design laws. Keyboard-navigable with a roving
// tabindex (exactly one cell in the tab order), arrows moving by day and
// by week, clamped within the month so navigation never silently crosses
// a boundary the grid isn't showing.
import { useEffect, useRef, useState } from "react";

/** Zero-pad a 1- or 2-digit number to two digits (no date library). */
function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}

/** The `YYYY-MM-DD` identity of a day in the shown month — the string the
 * backend's `read_daily` expects. Pure formatting, not date arithmetic. */
export function isoDay(year: number, month: number, day: number): string {
  return `${year}-${pad2(month)}-${pad2(day)}`;
}

/** Monday-first weekday index (0 = Monday .. 6 = Sunday) for the 1st of
 * the shown month. `Date.getDay()` is Sunday-first (0 = Sunday), so shift
 * it. Constructed at local midnight, so the weekday never slips a zone. */
function mondayFirstOffset(year: number, month: number): number {
  const sundayFirst = new Date(year, month - 1, 1).getDay();
  return (sundayFirst + 6) % 7;
}

/** Days in the shown month. Day 0 of the *next* month is the last day of
 * this one — sidesteps per-month length and leap years. */
function daysInMonth(year: number, month: number): number {
  return new Date(year, month, 0).getDate();
}

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/** Six rows of seven. The most a month can need is 6 (31 days starting on
 * a Sunday, Monday-first), so a fixed grid is uniform without ever
 * clipping. */
const CELLS = 42;

export default function MonthGrid({
  year,
  month,
  noteDays,
  selectedDay,
  today,
  onSelectDay,
}: {
  /** Calendar year of the shown month. */
  year: number;
  /** Calendar month, 1..12. */
  month: number;
  /** `YYYY-MM-DD` of every day that has a daily note.
   *
   * Full dates, not day-of-month numbers: collapsing to integers was
   * correct for the shown month and meant the grid could never mark
   * anything outside it, which is a floor this component should not have
   * (#446). */
  noteDays: Set<string>;
  /** The selected day-of-month, or null when nothing is selected (e.g.
   * the shown month differs from the selection's month). */
  selectedDay: number | null;
  /** `YYYY-MM-DD` of the real today, stamped in Rust (`get_today`).
   *
   * The grid is "the log through time", and paging it without an anchor
   * means losing your place on every turn. Today used to look marked only
   * in the current month, and only because it happened to be the initial
   * selection. */
  today: string;
  /** Called with a day's `YYYY-MM-DD` string when it is chosen. */
  onSelectDay: (iso: string) => void;
}) {
  const offset = mondayFirstOffset(year, month);
  const total = daysInMonth(year, month);
  const cellRefs = useRef<(HTMLButtonElement | null)[]>([]);
  // The day the roving tabindex currently marks (1-based). Seeded from
  // the selection when it is in this month, else the first day.
  const [focusedDay, setFocusedDay] = useState<number>(selectedDay ?? 1);

  // When the shown month pages, or the selection lands in it, re-seat the
  // marker so it never points past the new month's end (a shorter month)
  // or away from a just-made selection.
  useEffect(() => {
    setFocusedDay((current) => {
      if (selectedDay && selectedDay <= total) return selectedDay;
      return Math.min(current, total);
    });
  }, [year, month, total, selectedDay]);

  function moveFocus(day: number) {
    // Clamp within the month: arrowing off either end holds at the edge
    // rather than jumping to a month the grid isn't rendering.
    const clamped = Math.max(1, Math.min(total, day));
    setFocusedDay(clamped);
    cellRefs.current[clamped]?.focus();
  }

  function onKeyDown(event: React.KeyboardEvent) {
    const deltas: Record<string, number> = {
      ArrowRight: 1,
      ArrowLeft: -1,
      ArrowDown: 7,
      ArrowUp: -7,
    };
    const delta = deltas[event.key];
    if (delta === undefined) return;
    event.preventDefault();
    moveFocus(focusedDay + delta);
  }

  const label = new Date(year, month - 1, 1).toLocaleDateString(undefined, {
    month: "long",
    year: "numeric",
  });

  return (
    <div>
      <div
        aria-hidden
        className="mb-2 grid grid-cols-7 gap-1 text-center text-xs font-medium uppercase tracking-wider text-ink-faint"
      >
        {WEEKDAYS.map((wd) => (
          <div key={wd}>{wd}</div>
        ))}
      </div>
      <div
        role="group"
        aria-label={`${label} calendar`}
        onKeyDown={onKeyDown}
        className="grid grid-cols-7 gap-1"
      >
        {/* Leading blanks so the 1st lands under its weekday column. */}
        {Array.from({ length: offset }, (_unused, i) => (
          <div key={`pad-${i}`} aria-hidden className="aspect-square" />
        ))}
        {Array.from({ length: total }, (_unused, i) => {
          const day = i + 1;
          const iso = isoDay(year, month, day);
          const hasNote = noteDays.has(iso);
          const isSelected = selectedDay === day;
          const isToday = iso === today;
          return (
            <button
              key={day}
              type="button"
              ref={(el) => {
                cellRefs.current[day] = el;
              }}
              aria-pressed={isSelected}
              // Today rides the name, not only the ring: an anchor a
              // screen reader cannot find is not an anchor.
              aria-label={`${label} ${day}${isToday ? ", today" : ""}${hasNote ? ", has a note" : ""}`}
              tabIndex={day === focusedDay ? 0 : -1}
              onClick={() => onSelectDay(isoDay(year, month, day))}
              // Two independent states, two independent marks: the
              // selection fills, today rings, and the day that is both
              // wears both. One style doing double duty is what made
              // today vanish the moment you selected another day.
              className={`flex aspect-square flex-col items-center justify-center rounded border text-sm ${
                isSelected
                  ? "border-line bg-bg-sunken font-medium text-ink"
                  : "border-transparent text-ink-muted hover:border-line hover:text-ink"
              } ${isToday ? "ring-1 ring-inset ring-accent-interactive" : ""}`}
            >
              <span>{day}</span>
              {/* A calm dot marks a note-bearing day — never red, and
                  hidden from the a11y tree since the label already says
                  "has a note". */}
              <span
                aria-hidden
                className={`mt-0.5 h-1 w-1 rounded-full ${
                  hasNote ? "bg-accent-interactive" : "bg-transparent"
                }`}
              />
            </button>
          );
        })}
        {/* Trailing blanks to a fixed six-row grid. Without them the last
            row is short and the card's bottom edge lands somewhere new
            each month — the shape you page through should not move. */}
        {/* `aspect-square` like a day cell: an empty div has no height,
            so pads without it collapse their row and the fixed grid this
            exists to create is fixed only in cell count. */}
        {Array.from({ length: CELLS - offset - total }, (_unused, i) => (
          <div key={`tail-${i}`} aria-hidden className="aspect-square" />
        ))}
      </div>
    </div>
  );
}
