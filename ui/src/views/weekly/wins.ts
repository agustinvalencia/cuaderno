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
  /** Whether the *source* bullet carried a checkbox. Immutable across
   * edits: a plain hand-written bullet stays plain, and only a tick (or
   * a source checkbox) earns the `- [ ]` form on save — so ticking then
   * unticking a plain win leaves it plain rather than converting a
   * hand-written line into a checklist item. */
  checkbox: boolean;
}

/** A win as the editing UI holds it: a `Win` plus a stable identity, so a
 * row's DOM node (and its focus) follows the win across a reorder rather
 * than staying with the position a positional key would pin it to. */
export interface WinCard extends Win {
  id: number;
}

const BULLET = /^\s*[-*]\s+(?:\[( |x|X)\]\s+)?(.*)$/;

/** A text that, left alone in a bullet, would read back as a checkbox. */
const CHECKBOX_HEAD = /^\[( |x|X)\]\s/;

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
      // A leading backslash is the escape serialiseWins writes so a
      // plain win beginning with "[x] " does not masquerade as a
      // checkbox; strip it back off here.
      const text = match[2].replace(/^\\(?=\[( |x|X)\]\s)/, "").trim();
      wins.push({
        text,
        done: match[1] !== undefined && match[1].toLowerCase() === "x",
        checkbox: match[1] !== undefined,
      });
    } else {
      wins.push({ text: line.trim(), done: false, checkbox: false });
    }
  }
  return wins;
}

/** Serialise cards back to the markdown that goes into the note.
 *
 * A plain win whose text happens to begin with `[x] ` (someone typed it,
 * or edited a card to start that way) would re-parse as a checkbox on the
 * next read — a phantom tick, and the bracket eaten. So a plain bullet
 * whose text would be mistaken for a checkbox is written with the marker
 * held off by a backslash, which `parseWins` unescapes. A round trip is
 * lossless either way. */
export function serialiseWins(wins: Win[]): string {
  return wins
    .map((win) => {
      // A checkbox is written when the source had one or the win is
      // ticked — a tick needs somewhere to live. A plain, unticked win
      // stays a plain bullet, its marker-like text escaped so it does
      // not read back as a checkbox.
      if (win.checkbox || win.done) return `- [${win.done ? "x" : " "}] ${win.text}`;
      return `- ${CHECKBOX_HEAD.test(win.text) ? "\\" : ""}${win.text}`;
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
