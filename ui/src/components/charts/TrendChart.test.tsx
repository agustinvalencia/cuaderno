// The mark heuristic: an all-integer series reads as a count/volume
// (column); any fractional value keeps the line. It carries no domain
// knowledge, so these are the only cases that matter.
import { expect, test } from "vitest";
import type { TrackingSeries } from "../../api/bindings/TrackingSeries";
import { markForSeries } from "./TrendChart";

function series(...values: number[]): TrackingSeries {
  return {
    name: "test",
    points: values.map((value, index) => ({ date: `2026-07-0${index + 1}`, value })),
  };
}

test("an all-integer series is a column", () => {
  expect(markForSeries(series(6, 4, 9))).toBe("column");
});

test("negative and zero integers are still a column", () => {
  expect(markForSeries(series(0, -3, 2))).toBe("column");
});

test("any fractional value keeps the line", () => {
  expect(markForSeries(series(6, 4.5, 9))).toBe("line");
});

test("a single fractional point keeps the line", () => {
  expect(markForSeries(series(77.9))).toBe("line");
});

test("an empty series falls back to the line", () => {
  expect(markForSeries(series())).toBe("line");
});
