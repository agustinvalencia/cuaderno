// The wins list as data (#449).
//
// Wins used to be one markdown blob in a fixed-height editor: seeded by
// string concatenation, saved verbatim, with no per-win object at all —
// so an individual win could not be edited, removed or reordered. Above
// it sat a grid of "Completed this week" cards that were purely
// decorative, rendering the same completions the editor already held as
// prose. The same information twice, once pretty and inert, once raw and
// editable.
//
// This is the seam that makes them one thing. Markdown stays the source
// of truth — a hand-edited `## Wins` section still parses, and what is
// written back is ordinary bullets a human would have typed.

/** One win, as the card list holds it. */
export interface Win {
  text: string;
  /** A `- [x]` bullet reads as done. Plain `-` bullets stay plain, so a
   * hand-written list is not silently converted into a checklist. */
  done: boolean;
  /** Whether the source bullet carried a checkbox at all. A win parsed
   * from a plain bullet is written back as one. */
  checkbox: boolean;
}

const BULLET = /^\s*[-*]\s+(?:\[( |x|X)\]\s+)?(.*)$/;

/** Parse a `## Wins` body into cards.
 *
 * Anything that is not a bullet — a paragraph someone wrote, a stray
 * heading — is kept as its own plain entry rather than dropped, because
 * losing text on a round trip is the one thing a parser here must not
 * do. */
export function parseWins(markdown: string): Win[] {
  const wins: Win[] = [];
  for (const line of markdown.split("\n")) {
    if (line.trim() === "") continue;
    const match = BULLET.exec(line);
    if (match) {
      wins.push({
        text: match[2].trim(),
        done: match[1] !== undefined && match[1].toLowerCase() === "x",
        checkbox: match[1] !== undefined,
      });
    } else {
      wins.push({ text: line.trim(), done: false, checkbox: false });
    }
  }
  return wins;
}

/** Serialise cards back to the markdown that goes into the note. */
export function serialiseWins(wins: Win[]): string {
  return wins
    .map((win) => {
      if (!win.checkbox) return `- ${win.text}`;
      return `- [${win.done ? "x" : " "}] ${win.text}`;
    })
    .join("\n");
}

/** Move the item at `from` to `to`, returning a new array. Out-of-range
 * moves are no-ops, so the callers do not each re-check the ends. */
export function reorder<T>(items: T[], from: number, to: number): T[] {
  if (to < 0 || to >= items.length || from === to) return items;
  const next = [...items];
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
}
