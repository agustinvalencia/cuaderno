// MonthGrid (#340): the from-scratch month layout, the note-bearing-day
// marks, day selection, and the roving-tabindex keyboard navigation.
import { afterEach, expect, test, vi } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import MonthGrid, { isoDay } from "./MonthGrid";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

afterEach(cleanup);

// July 2026: the 1st is a Wednesday, so with a Monday-first grid there
// are two leading blanks (Mon, Tue) before day 1.
// A day no other test names, so adding ", today" to its label cannot
// collide with the existing selection and keyboard assertions.
const TODAY = "2026-07-22";

function renderJuly(
  onSelectDay = vi.fn(),
  selectedDay: number | null = null,
  today = TODAY,
) {
  render(
    <MonthGrid
      year={2026}
      month={7}
      marks={new Map([["2026-07-15", ["bg-accent-interactive"]]])}
      selectedDay={selectedDay}
      today={today}
      onSelectDay={onSelectDay}
    />,
  );
  return onSelectDay;
}

test("isoDay formats a zero-padded YYYY-MM-DD", () => {
  expect(isoDay(2026, 7, 5)).toBe("2026-07-05");
  expect(isoDay(2026, 12, 31)).toBe("2026-12-31");
});

test("renders every day of the month with weekday headers", () => {
  renderJuly();
  // 31 day cells for July.
  expect(screen.getAllByRole("button")).toHaveLength(31);
  expect(screen.getByText("Mon")).toBeDefined();
  expect(screen.getByText("Sun")).toBeDefined();
});

test("marks a note-bearing day in its accessible name", () => {
  renderJuly();
  // Day 15 has a note; day 16 does not.
  expect(screen.getByRole("button", { name: /July 2026 15, has a note/ })).toBeDefined();
  expect(screen.queryByRole("button", { name: /July 2026 16, has a note/ })).toBeNull();
});

test("clicking a day reports its ISO date", () => {
  const onSelectDay = renderJuly();
  screen.getByRole("button", { name: /July 2026 10$/ }).click();
  expect(onSelectDay).toHaveBeenCalledWith("2026-07-10");
});

test("arrow keys move the roving focus by day and week", () => {
  renderJuly();
  const grid = screen.getByRole("group");
  const day1 = screen.getByRole("button", { name: /July 2026 1$/ });
  // Day 1 starts in the tab order (no selection, so it seeds the marker).
  expect(day1.getAttribute("tabindex")).toBe("0");

  // Right moves to day 2, down a further week to day 9.
  fireEvent.keyDown(grid, { key: "ArrowRight" });
  expect(screen.getByRole("button", { name: /July 2026 2$/ }).getAttribute("tabindex")).toBe("0");
  fireEvent.keyDown(grid, { key: "ArrowDown" });
  expect(screen.getByRole("button", { name: /July 2026 9$/ }).getAttribute("tabindex")).toBe("0");
});

test("arrowing off the start clamps at the first day", () => {
  renderJuly();
  const grid = screen.getByRole("group");
  fireEvent.keyDown(grid, { key: "ArrowLeft" });
  // Held at day 1 rather than crossing into the previous month.
  expect(screen.getByRole("button", { name: /July 2026 1$/ }).getAttribute("tabindex")).toBe("0");
});

test("ArrowUp on the first day clamps (a -7 delta holds at day 1)", () => {
  renderJuly();
  const grid = screen.getByRole("group");
  // Day 1 seeds the marker; a week up would land on day -6, so it clamps.
  fireEvent.keyDown(grid, { key: "ArrowUp" });
  expect(screen.getByRole("button", { name: /July 2026 1$/ }).getAttribute("tabindex")).toBe("0");
});

test("ArrowDown on the last day clamps (a +7 delta holds at day 31)", () => {
  // Seed the marker on the last day (July has 31, sitting in a partial
  // final row), then arrow down a week — day 38 doesn't exist, so it
  // holds at 31 rather than spilling into August.
  renderJuly(vi.fn(), 31);
  const grid = screen.getByRole("group");
  expect(screen.getByRole("button", { name: /July 2026 31$/ }).getAttribute("tabindex")).toBe("0");
  fireEvent.keyDown(grid, { key: "ArrowDown" });
  expect(screen.getByRole("button", { name: /July 2026 31$/ }).getAttribute("tabindex")).toBe("0");
});

test("pads to a fixed six-row grid, leading and trailing", () => {
  // July 2026's 1st is a Wednesday; Monday-first that is offset 2. Every
  // month renders 42 cells, so the card's bottom edge does not move as
  // you page. (The weekday-header row is a single aria-hidden container,
  // so scope the count to the grid group.)
  renderJuly();
  const grid = screen.getByRole("group");
  const pads = grid.querySelectorAll(":scope > [aria-hidden]");
  expect(pads).toHaveLength(42 - 31);
  expect(grid.children).toHaveLength(42);
});

test("a shorter month pads to the same six rows", () => {
  // February 2026: 28 days starting on a Sunday — a different offset and
  // a different length, the same grid.
  render(
    <MonthGrid
      year={2026}
      month={2}
      marks={new Map()}
      selectedDay={null}
      today={TODAY}
      onSelectDay={vi.fn()}
    />,
  );
  expect(screen.getByRole("group").children).toHaveLength(42);
});

test("today is marked, and stays marked when another day is selected", () => {
  // The grid is the log through time; paging it without an anchor means
  // losing your place on every turn. Today used to look marked only
  // because it happened to be the initial selection.
  renderJuly(vi.fn(), 20);
  expect(screen.getByRole("button", { name: "July 2026 22, today" })).toBeDefined();
  expect(
    screen.getByRole("button", { name: "July 2026 22, today" }).getAttribute("aria-pressed"),
  ).toBe("false");
  expect(screen.getByRole("button", { name: "July 2026 20" }).getAttribute("aria-pressed")).toBe(
    "true",
  );
});

test("the day that is both today and selected says both", () => {
  renderJuly(vi.fn(), 22);
  const cell = screen.getByRole("button", { name: "July 2026 22, today" });
  expect(cell.getAttribute("aria-pressed")).toBe("true");
});

test("a month without today marks nothing", () => {
  // Paging away must not leave a stray anchor on the same day number.
  render(
    <MonthGrid
      year={2026}
      month={8}
      marks={new Map()}
      selectedDay={null}
      today={TODAY}
      onSelectDay={vi.fn()}
    />,
  );
  expect(screen.queryByRole("button", { name: /today/ })).toBeNull();
});

test("has no axe violations", async () => {
  const { container } = render(
    <MonthGrid
      year={2026}
      month={7}
      marks={new Map([["2026-07-15", ["bg-accent-interactive"]]])}
      selectedDay={22}
      today={TODAY}
      onSelectDay={vi.fn()}
    />,
  );
  expect(
    await axe(container, { rules: { "color-contrast": { enabled: false } } }),
  ).toHaveNoViolations();
});

/** The dot classes actually painted under a day cell. The accessible
 * name branches on the mark count, so it matches whether or not a pixel
 * is drawn — only the DOM answers "is there a dot, and what colour". */
function dotsOn(name: RegExp | string): string[] {
  const cell = screen.getByRole("button", { name });
  return [...cell.querySelectorAll("span.rounded-full")].map((s) => s.className);
}

test("a marked day draws a dot in the colour its caller asked for", () => {
  // The marks are `aria-hidden`, so every existing "marks a day"
  // assertion reads the label — which is computed from the mark count and
  // would pass with nothing painted at all.
  renderJuly();
  expect(dotsOn(/July 2026 15/).join(" ")).toContain("bg-accent-interactive");
  expect(dotsOn(/July 2026 16/)).toHaveLength(0);
});

test("several marks on one day draw several dots, capped at three", () => {
  // The reason `marks` is a map of class lists rather than a set of days:
  // Commitments draws one context-coloured dot per promise falling on it.
  // Past three the cell is a smudge, and a count is not what a grid is for.
  render(
    <MonthGrid
      year={2026}
      month={7}
      marks={
        new Map([
          ["2026-07-06", ["bg-ctx-work", "bg-ctx-personal"]],
          ["2026-07-07", ["bg-ctx-work", "bg-ctx-work", "bg-ctx-work", "bg-ctx-work"]],
        ])
      }
      selectedDay={null}
      today={TODAY}
      onSelectDay={vi.fn()}
    />,
  );
  const two = dotsOn(/July 2026 6/);
  expect(two).toHaveLength(2);
  expect(two[1]).toContain("bg-ctx-personal");
  expect(dotsOn(/July 2026 7/)).toHaveLength(3);
});
