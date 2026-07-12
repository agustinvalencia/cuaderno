// Note-content parsing (UI request 2026-07-12): splitting a raw note blob
// into frontmatter + `##` sections, and a `## Logs` section into
// timestamped entries.
import { expect, test } from "vitest";
import { isLogsSection, parseLogEntries, parseNote } from "./noteContent";

const DAILY = `---
type: daily
date: 2026-07-12
context: work
tags:
  - a
  - b
---

# Tuesday 12 July

## Standup

Plan the watcher work.

## Logs

- **09:05**: started the day
- **14:32**: wired the watcher
  was: idle
  now: building
- **17:10**: wrapped up

## Notes

A stray thought.
`;

test("parseNote strips frontmatter into a flat scalar record", () => {
  const { frontmatter } = parseNote(DAILY);
  expect(frontmatter).toEqual({
    type: "daily",
    date: "2026-07-12",
    context: "work",
  });
  // The `tags:` list is a container — dropped, not shown as an empty scalar.
  expect(frontmatter.tags).toBeUndefined();
});

test("parseNote splits the body on level-2 headings, preamble first", () => {
  const { sections } = parseNote(DAILY);
  expect(sections.map((s) => s.heading)).toEqual([
    null,
    "Standup",
    "Logs",
    "Notes",
  ]);
  expect(sections[0].body).toContain("# Tuesday 12 July");
  expect(sections[1].body).toBe("Plan the watcher work.");
});

test("parseNote drops empty sections (unfilled scaffold headings)", () => {
  const { sections } = parseNote(
    "# Title\n\n## Empty\n\n## Full\n\nhas content\n",
  );
  expect(sections.map((s) => s.heading)).toEqual([null, "Full"]);
});

test("parseNote treats a note with no frontmatter as all body", () => {
  const { frontmatter, sections } = parseNote(
    "# Just a title\n\n## One\n\nx\n",
  );
  expect(frontmatter).toEqual({});
  expect(sections.map((s) => s.heading)).toEqual([null, "One"]);
});

test("parseNote keeps a `###` subheading inside its section body", () => {
  const { sections } = parseNote("## Top\n\ntext\n\n### Sub\n\nmore\n");
  expect(sections).toHaveLength(1);
  expect(sections[0].heading).toBe("Top");
  expect(sections[0].body).toContain("### Sub");
});

test("parseNote strips surrounding quotes from a frontmatter value", () => {
  const { frontmatter } = parseNote('---\ntitle: "Quoted value"\n---\nbody\n');
  expect(frontmatter.title).toBe("Quoted value");
});

test("parseNote drops an inline-flow container frontmatter value", () => {
  const { frontmatter } = parseNote(
    "---\ntype: daily\ntags: [a, b]\n---\nbody\n",
  );
  expect(frontmatter).toEqual({ type: "daily" });
});

test("parseNote reads CRLF frontmatter", () => {
  const { frontmatter } = parseNote("---\r\ntype: daily\r\n---\r\nbody\r\n");
  expect(frontmatter).toEqual({ type: "daily" });
});

test("splitBodySections ignores a `## ` line inside a fenced code block", () => {
  const md = "## Notes\n\nExample:\n\n```md\n## Section A\nbody\n```\n\ndone\n";
  const { sections } = parseNote(md);
  // The fenced `## Section A` must not open a new section — everything
  // stays under Notes, fence intact.
  expect(sections.map((s) => s.heading)).toEqual(["Notes"]);
  expect(sections[0].body).toContain("## Section A");
  expect(sections[0].body).toContain("done");
});

test("parseLogEntries reads timestamped entries and folds continuations", () => {
  const { sections } = parseNote(DAILY);
  const logs = sections.find((s) => isLogsSection(s.heading));
  expect(logs).toBeDefined();
  const entries = parseLogEntries(logs!.body);
  expect(entries).toEqual([
    { time: "09:05", text: "started the day" },
    { time: "14:32", text: "wired the watcher; was: idle; now: building" },
    { time: "17:10", text: "wrapped up" },
  ]);
});

test("parseLogEntries returns [] for a section with no log lines", () => {
  expect(parseLogEntries("just prose, no bullets")).toEqual([]);
});

test("parseLogEntries ignores an indented line before any entry", () => {
  // A stray continuation with no entry to attach to is dropped, not crashed.
  expect(
    parseLogEntries("  orphan continuation\n- **09:00**: real entry"),
  ).toEqual([{ time: "09:00", text: "real entry" }]);
});

test("isLogsSection matches case-insensitively and rejects null", () => {
  expect(isLogsSection("Logs")).toBe(true);
  expect(isLogsSection("logs")).toBe(true);
  expect(isLogsSection("Notes")).toBe(false);
  expect(isLogsSection(null)).toBe(false);
});
