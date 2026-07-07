// The wikilink parser's contract: plain targets, |label aliases,
// adjacent links, and the unparseable-stays-literal escape hatch.
import { expect, test } from "vitest";
import { remarkWikilinks, wikilinkSegments } from "./remarkWikilinks";

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

// The highest-value AST-safety guarantee: `[[target]]` written inside
// code — inline `inlineCode` or a fenced `code` block — must stay
// literal, never become a navigable link. mdast code nodes carry a
// `value` and no `children`, so the plugin's tree walk never descends
// into them; this pins that behaviour against a regression that started
// treating code as prose.

/** Minimal mdast shape for asserting the plugin's tree walk directly. */
interface TestNode {
  type: string;
  value?: string;
  lang?: string;
  children?: TestNode[];
  data?: Record<string, unknown>;
}

test("[[target]] inside inline code and fenced code stays literal", () => {
  // A paragraph mixing prose, inline code, and a real wikilink, plus a
  // fenced code block below it.
  const tree: TestNode = {
    type: "root",
    children: [
      {
        type: "paragraph",
        children: [
          { type: "text", value: "run " },
          { type: "inlineCode", value: "[[target]]" },
          { type: "text", value: " then see [[real]]" },
        ],
      },
      { type: "code", lang: "ts", value: "const link = '[[target]]';" },
    ],
  };

  remarkWikilinks()(tree);

  const paragraph = tree.children![0];
  const fenced = tree.children![1];

  // The inline-code node is untouched: same value, still no children.
  const inlineCode = paragraph.children!.find((child) => child.type === "inlineCode")!;
  expect(inlineCode.value).toBe("[[target]]");
  expect(inlineCode.children).toBeUndefined();

  // The fenced code block is untouched too.
  expect(fenced.type).toBe("code");
  expect(fenced.value).toBe("const link = '[[target]]';");
  expect(fenced.children).toBeUndefined();

  // The real wikilink in prose still became a link node — proof the
  // plugin ran and only spared the code.
  const link = paragraph.children!.find((child) => child.type === "link")!;
  expect(link).toBeDefined();
  const props = link.data!.hProperties as Record<string, unknown>;
  expect(props["data-wikilink"]).toBe("real");
});
