// The wins parse/serialise round trip (#449).
//
// Markdown stays the source of truth: the cards are a lens over a `##
// Wins` section that a human may have hand-written, and what is saved
// has to read as ordinary bullets. Losing text on a round trip is the
// one thing this must not do.
import { expect, test } from "vitest";
import { parseWins, reorder, serialiseWins } from "./wins";

test("a hand-written plain list stays plain", () => {
  // Someone who typed `- shipped it` in their editor should not find it
  // silently converted into a checklist.
  const wins = parseWins("- shipped it\n- rested");
  expect(wins).toEqual([
    { text: "shipped it", done: false, checkbox: false },
    { text: "rested", done: false, checkbox: false },
  ]);
  expect(serialiseWins(wins)).toBe("- shipped it\n- rested");
});

test("checkboxes round trip, ticked and unticked", () => {
  const source = "- [x] shipped it\n- [ ] rested";
  expect(serialiseWins(parseWins(source))).toBe(source);
});

test("an uppercase tick is still a tick", () => {
  expect(parseWins("- [X] shipped it")[0].done).toBe(true);
});

test("a `*` bullet parses, and normalises to `-` on the way back", () => {
  expect(parseWins("* shipped it")[0].text).toBe("shipped it");
  expect(serialiseWins(parseWins("* shipped it"))).toBe("- shipped it");
});

test("prose that is not a bullet is kept, not dropped", () => {
  // The alternative is silently eating a line someone wrote.
  const wins = parseWins("It was a hard week.\n- but I shipped it");
  expect(wins.map((w) => w.text)).toEqual(["It was a hard week.", "but I shipped it"]);
});

test("blank lines do not become empty wins", () => {
  expect(parseWins("- one\n\n\n- two")).toHaveLength(2);
});

test("ticking a plain win gives it a checkbox", () => {
  // Because the card offers the tick, the bullet has to be able to carry
  // it — otherwise the state would have nowhere to live on disk.
  const wins = parseWins("- shipped it");
  const ticked = [{ ...wins[0], done: true, checkbox: true }];
  expect(serialiseWins(ticked)).toBe("- [x] shipped it");
});

test("reorder moves an item, and refuses to move off either end", () => {
  const items = ["a", "b", "c"];
  expect(reorder(items, 2, 0)).toEqual(["c", "a", "b"]);
  expect(reorder(items, 0, -1)).toBe(items);
  expect(reorder(items, 2, 3)).toBe(items);
  expect(reorder(items, 1, 1)).toBe(items);
});

test("a plain line that looks like a checkbox survives the round trip", () => {
  // A hand-written `[x] shipped it` with no dash is prose, kept plain —
  // and must not come back as a phantom-ticked checkbox with the bracket
  // eaten.
  const wins = parseWins("[x] shipped it");
  expect(wins).toEqual([{ text: "[x] shipped it", done: false, checkbox: false }]);
  expect(parseWins(serialiseWins(wins))).toEqual(wins);
});

test("editing a plain win to start with a bracket token does not fake a checkbox", () => {
  const edited = [{ text: "[ ] not really a checkbox", done: false, checkbox: false }];
  const roundTripped = parseWins(serialiseWins(edited));
  expect(roundTripped[0]).toEqual({
    text: "[ ] not really a checkbox",
    done: false,
    checkbox: false,
  });
});

test("a real checkbox is untouched by the escape", () => {
  // The escape only applies to plain bullets; a genuine `- [x]` still
  // round-trips as itself, no backslash.
  expect(serialiseWins(parseWins("- [x] shipped it"))).toBe("- [x] shipped it");
});
