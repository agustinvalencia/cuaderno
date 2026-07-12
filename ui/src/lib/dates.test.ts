// shortDate: the shared day/short-month formatter. The load-bearing
// invariant is that a `YYYY-MM-DD` is parsed at LOCAL midnight, so the
// day never slips across a timezone — the exact bug `new Date("YYYY-MM-DD")`
// (UTC-parsed) would reintroduce.
import { expect, test } from "vitest";
import { shortDate } from "./dates";

test("formats an ISO date as day + short month", () => {
  // Locale-agnostic assertion: the output contains the day number and is
  // non-empty; the shape is `2 Jul` / `Jul 2` depending on locale.
  const out = shortDate("2026-07-02");
  expect(out).toMatch(/2/);
  expect(out.length).toBeGreaterThan(0);
});

test("keeps the calendar day — no timezone slip to the day before", () => {
  // Parsed at local midnight, `2026-07-01` must render as the 1st, never
  // the 30th (which a UTC parse could produce in a negative-offset zone).
  const parts = new Intl.DateTimeFormat(undefined, {
    day: "numeric",
    month: "short",
  }).format(new Date("2026-07-01T00:00:00"));
  expect(shortDate("2026-07-01")).toBe(parts);
});
