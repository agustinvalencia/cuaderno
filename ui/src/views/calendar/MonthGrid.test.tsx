// MonthGrid (#340): the from-scratch month layout, the note-bearing-day
// marks, day selection, and the roving-tabindex keyboard navigation.
import { afterEach, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import MonthGrid, { isoDay } from "./MonthGrid";

afterEach(cleanup);

// July 2026: the 1st is a Wednesday, so with a Monday-first grid there
// are two leading blanks (Mon, Tue) before day 1.
function renderJuly(onSelectDay = vi.fn(), selectedDay: number | null = null) {
  render(
    <MonthGrid
      year={2026}
      month={7}
      noteDays={new Set([15])}
      selectedDay={selectedDay}
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

test("renders leading blank pad cells for the month's Monday-first offset", () => {
  const { container } = render(
    <MonthGrid
      year={2026}
      month={7}
      noteDays={new Set()}
      selectedDay={null}
      onSelectDay={vi.fn()}
    />,
  );
  // July 2026's 1st is a Wednesday; Monday-first that is offset 2, so
  // there are two aria-hidden pad cells before day 1. (The weekday-header
  // row is a single aria-hidden container, so scope the count to the grid
  // group to count only the pad cells.)
  const grid = screen.getByRole("group");
  const pads = grid.querySelectorAll(":scope > [aria-hidden]");
  expect(pads).toHaveLength(2);
  expect(container).toBeDefined();
});
