// The wikilink parser's contract: plain targets, |label aliases,
// adjacent links, and the unparseable-stays-literal escape hatch.
import { expect, test } from "vitest";
import { wikilinkSegments } from "./remarkWikilinks";

test("a plain [[target]] becomes a link whose label is the target", () => {
  expect(wikilinkSegments("see [[garden]] now")).toEqual([
    { kind: "text", value: "see " },
    { kind: "link", target: "garden", label: "garden" },
    { kind: "text", value: " now" },
  ]);
});

test("[[target|label]] splits the alias from the resolution target", () => {
  expect(wikilinkSegments("[[projects/garden|the garden]]")).toEqual([
    { kind: "link", target: "projects/garden", label: "the garden" },
  ]);
});

test("adjacent [[a]][[b]] parses as two links with no gap", () => {
  expect(wikilinkSegments("[[a]][[b]]")).toEqual([
    { kind: "link", target: "a", label: "a" },
    { kind: "link", target: "b", label: "b" },
  ]);
});

test("an empty or targetless wikilink stays literal text (no link)", () => {
  const segments = wikilinkSegments("[[]] and [[|x]]");
  expect(segments.every((s) => s.kind === "text")).toBe(true);
  expect(segments.map((s) => (s.kind === "text" ? s.value : "")).join("")).toBe(
    "[[]] and [[|x]]",
  );
});

test("plain text with no wikilinks is a single text segment", () => {
  expect(wikilinkSegments("nothing here")).toEqual([{ kind: "text", value: "nothing here" }]);
});
