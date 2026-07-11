// NoteContent (UI request 2026-07-12): a daily note blob is presented as
// a separated metadata strip, titled sections, and the `## Logs` history
// as timestamped log cards — not a raw markdown wall.
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, render, screen } from "@testing-library/react";
import NoteContent from "./NoteContent";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

afterEach(cleanup);

const DAILY = `---
type: daily
date: 2026-07-12
---

# Tuesday 12 July

## Standup

Plan the watcher work.

## Logs

- **09:05**: started the day
- **14:32**: wired the watcher
`;

test("renders frontmatter as a separated Properties strip", () => {
  render(<NoteContent markdown={DAILY} onWikilink={() => {}} />);
  expect(screen.getByText("Properties")).toBeDefined();
  expect(screen.getByText("type")).toBeDefined();
  expect(screen.getByText("daily")).toBeDefined();
});

test("renders each section under a clear title", () => {
  render(<NoteContent markdown={DAILY} onWikilink={() => {}} />);
  // The preamble h1 from the body still renders.
  expect(screen.getByRole("heading", { name: "Tuesday 12 July" })).toBeDefined();
  expect(screen.getByRole("heading", { name: "Standup" })).toBeDefined();
  expect(screen.getByText("Plan the watcher work.")).toBeDefined();
});

test("renders the Logs section as timestamped cards", () => {
  render(<NoteContent markdown={DAILY} onWikilink={() => {}} />);
  expect(screen.getByRole("heading", { name: "Logs" })).toBeDefined();
  // Each entry's time and text appear as its own card content.
  expect(screen.getByText("09:05")).toBeDefined();
  expect(screen.getByText("started the day")).toBeDefined();
  expect(screen.getByText("14:32")).toBeDefined();
  expect(screen.getByText("wired the watcher")).toBeDefined();
});

test("a Logs section that doesn't parse falls back to plain markdown", () => {
  const md = "## Logs\n\nJust prose, no timestamped bullets.\n";
  render(<NoteContent markdown={md} onWikilink={() => {}} />);
  expect(screen.getByRole("heading", { name: "Logs" })).toBeDefined();
  expect(screen.getByText("Just prose, no timestamped bullets.")).toBeDefined();
});

test("is axe-clean", async () => {
  const { container } = render(<NoteContent markdown={DAILY} onWikilink={() => {}} />);
  expect(await axe(container)).toHaveNoViolations();
});
