import { expect, test } from "vitest";

import { noteLabel } from "./noteLabel";

test("a frontmatter title wins", () => {
  expect(noteLabel("portfolios/x/2026-07-13-thing.md", "Sparse variants hold up")).toBe(
    "Sparse variants hold up",
  );
});

test("a blank title is treated as absent", () => {
  expect(noteLabel("portfolios/x/2026-07-13-index-shape.md", "   ")).toBe("index shape");
});

test("the filename reads as words, without its date stamp", () => {
  expect(noteLabel("portfolios/x/2026-07-13-index-shape.md", null)).toBe("index shape");
});

test("an _index names its folder, not itself", () => {
  // "index" would describe every portfolio equally, which is no label.
  expect(noteLabel("portfolios/how-should-the-pipeline-be-staged/_index.md", null)).toBe(
    "how should the pipeline be staged",
  );
});

test("a path with no folder still yields something", () => {
  expect(noteLabel("scratch.md", null)).toBe("scratch");
});

test("a date-only filename falls back to the path rather than an empty label", () => {
  expect(noteLabel("journal/2026/daily/2026-07-13.md", null)).toBe("2026-07-13");
});

test("a weekly note keeps its ISO week form", () => {
  // `2026-W30` is how cuaderno names weeklies; "2026 W30" is not friendlier.
  expect(noteLabel("journal/2026/weekly/2026-W30.md", null)).toBe("2026-W30");
});

test("a monthly note keeps its date form", () => {
  expect(noteLabel("journal/2026/monthly/2026-07.md", null)).toBe("2026-07");
});
