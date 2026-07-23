// Neutral staleness tiers (M8 law, carried into M9): freshness is
// rendered as INK EMPHASIS, never a hue — colour is identity, never
// urgency, and no semantic green/amber/red token exists. A fresh thing
// sits at full ink; an ageing one fades to ink-muted; a long-dormant
// one recedes to ink-faint. Extracted here so the Portfolio browser
// (M8) and the Strategic health table (M9) read from ONE tier ladder
// and can't drift.
//
// `staleness_days` rides the wire as a `number` — Tauri's IPC goes through
// JSON, so the Rust `i64` arrives as a JS number, and the binding declares
// it as such — or `null` when there's nothing dated to measure against.

/** Below this many days a thing reads as fresh (full ink). */
export const AGEING_AFTER_DAYS = 30;
/** Past this many days it reads as long-dormant (ink-faint). */
export const STALE_AFTER_DAYS = 90;

/** The neutral ink tier for a staleness-in-days. `null` (nothing dated
 * yet) recedes to ink-faint — the same tone as long-dormant, since both
 * are "nothing recent here", never an alarm. */
export function stalenessTone(stalenessDays: number | null): string {
  if (stalenessDays === null) return "text-ink-faint";
  if (stalenessDays <= AGEING_AFTER_DAYS) return "text-ink";
  if (stalenessDays <= STALE_AFTER_DAYS) return "text-ink-muted";
  return "text-ink-faint";
}

/** A muted "how long ago" label — `today` for same-day (or future,
 * which a typo can produce), else `N d ago`. */
export function stalenessAgo(stalenessDays: number): string {
  return stalenessDays <= 0 ? "today" : `${stalenessDays}d ago`;
}
