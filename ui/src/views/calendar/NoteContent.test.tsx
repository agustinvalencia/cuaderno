// NoteContent (UI request 2026-07-12): a daily note blob is presented as
// a separated metadata strip, titled sections, and the `## Logs` history
// as timestamped log cards — not a raw markdown wall.
import { afterEach, beforeAll, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import NoteContent from "./NoteContent";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

// The log-order toggle persists via localStorage; jsdom's here doesn't work.
beforeAll(() => {
  const store = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    value: {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, String(v)),
      removeItem: (k: string) => void store.delete(k),
      clear: () => store.clear(),
      key: (i: number) => [...store.keys()][i] ?? null,
      get length() {
        return store.size;
      },
    },
    configurable: true,
  });
});

afterEach(() => {
  cleanup();
  localStorage.clear();
});

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
  expect(
    screen.getByRole("heading", { name: "Tuesday 12 July" }),
  ).toBeDefined();
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

test("caps the Logs stack in a focusable scroll region (calendar context)", () => {
  render(<NoteContent markdown={DAILY} onWikilink={() => {}} />);
  // The calendar page is wide and page-scrolled, so Logs get a capped,
  // keyboard-focusable inner scroll.
  const region = screen.getByLabelText("Logs entries");
  expect(region.getAttribute("tabindex")).toBe("0");
  expect(region.className).toContain("overflow-y-auto");
});

test("a wikilink inside a log entry stays a clickable link, not raw text", () => {
  const md =
    "## Logs\n\n- **14:32**: shipped [[projects/cuaderno]] milestone\n";
  const calls: string[] = [];
  render(<NoteContent markdown={md} onWikilink={(t) => calls.push(t)} />);
  // Rendered as a wikilink anchor (not literal `[[…]]` text).
  const link = screen.getByRole("link", { name: "projects/cuaderno" });
  expect(screen.queryByText(/\[\[projects\/cuaderno\]\]/)).toBeNull();
  link.click();
  expect(calls).toEqual(["projects/cuaderno"]);
});

test("the Logs sort toggle flips the entry order", () => {
  render(<NoteContent markdown={DAILY} onWikilink={() => {}} />);
  // Default is oldest-first: 09:05 before 14:32.
  const times = () =>
    screen.getAllByText(/^\d{2}:\d{2}$/).map((el) => el.textContent);
  expect(times()).toEqual(["09:05", "14:32"]);
  fireEvent.click(screen.getByRole("button", { name: /reverse log order/ }));
  expect(times()).toEqual(["14:32", "09:05"]);
});

test("a Logs section that doesn't parse falls back to plain markdown", () => {
  const md = "## Logs\n\nJust prose, no timestamped bullets.\n";
  render(<NoteContent markdown={md} onWikilink={() => {}} />);
  expect(screen.getByRole("heading", { name: "Logs" })).toBeDefined();
  expect(screen.getByText("Just prose, no timestamped bullets.")).toBeDefined();
});

test("is axe-clean", async () => {
  const { container } = render(
    <NoteContent markdown={DAILY} onWikilink={() => {}} />,
  );
  expect(await axe(container)).toHaveNoViolations();
});
