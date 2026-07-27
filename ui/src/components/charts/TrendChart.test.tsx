// Picking a series' mark. A DECLARED mark wins - the vault said what the
// metric is, which beats any signal read off its values. Absent one, the
// heuristic stands: an all-integer series reads as a count/volume (column),
// any fractional value keeps the line.
import { expect, test } from "vitest";
import type { PlotKind } from "../../api/bindings/PlotKind";
import type { TrackingSeries } from "../../api/bindings/TrackingSeries";
import { captionFor, markForSeries } from "./TrendChart";

function series(...values: number[]): TrackingSeries {
  return {
    name: "test",
    points: values.map((value, index) => ({ date: `2026-07-0${index + 1}`, value })),
    unit: null,
    label: null,
    mark: null,
  };
}

function declared(mark: PlotKind, ...values: number[]): TrackingSeries {
  return { ...series(...values), mark };
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

test("a declared mark beats the heuristic", () => {
  // All-integer values would read as a column; the declaration says line.
  expect(markForSeries(declared("line", 6, 4, 9))).toBe("line");
  // And fractional values would read as a line; the declaration says column.
  expect(markForSeries(declared("column", 6, 4.5))).toBe("column");
});

test("a mark this chart cannot draw resolves to its closest", () => {
  // Two marks are drawable. A scatter reads as discrete points, an area as
  // a filled line - honour the intent rather than ignoring the declaration.
  expect(markForSeries(declared("scatter", 6, 4.5))).toBe("column");
  expect(markForSeries(declared("area", 6, 4, 9))).toBe("line");
});

test("`none` falls through to the heuristic rather than picking a mark", () => {
  // `none` says "not drawn", not "draw it flat". Gating on it is #500; until
  // then a series that reaches the chart is drawn by the heuristic.
  expect(markForSeries(declared("none", 6, 4, 9))).toBe("column");
  expect(markForSeries(declared("none", 6, 4.5))).toBe("line");
});

test("the caption prefers a declared label and appends a declared unit", () => {
  // A metric key is written for the data (`resting_hr`), not for a reader.
  const base = series(60, 58);
  expect(captionFor(base)).toBe("test");
  expect(captionFor({ ...base, unit: "bpm" })).toBe("test (bpm)");
  expect(captionFor({ ...base, label: "Resting heart rate" })).toBe("Resting heart rate");
  expect(captionFor({ ...base, label: "Resting heart rate", unit: "bpm" })).toBe(
    "Resting heart rate (bpm)",
  );
});
