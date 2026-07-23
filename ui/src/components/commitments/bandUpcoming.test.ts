// `bandUpcoming` (#447): the week arithmetic behind This week / Next
// week, tested directly.
//
// It needs its own suite because every view fixture in the repo stamps a
// Wednesday `today`, and on a Wednesday two independent mistakes cancel:
// an off-by-one in the days-left calculation and a Sunday-first weekday
// index produce the same answer. Both survived the whole 377-test suite
// when applied one at a time.
import { expect, test } from "vitest";
import type { CommitmentEntry } from "../../api/bindings/CommitmentEntry";
import { bandUpcoming } from "./CommitmentsTimeline";

function on(date: string): CommitmentEntry {
  return {
    date,
    title: date,
    source: { kind: "standalone_commitment", slug: date },
    is_overdue: false,
    context: "work",
  };
}

/** The dates in each band, for a `today` and a set of entry dates. */
function bands(today: string, dates: string[]) {
  const out = bandUpcoming(dates.map(on), today);
  return {
    thisWeek: out.thisWeek.map((e) => e.date),
    nextWeek: out.nextWeek.map((e) => e.date),
    later: out.later.map((e) => e.date),
  };
}

// 2026-07-13 is a Monday, so that week runs Mon 13 to Sun 19 and the next
// runs Mon 20 to Sun 26.
test("on a Monday, this week reaches Sunday and stops", () => {
  const out = bands("2026-07-13", ["2026-07-13", "2026-07-19", "2026-07-20", "2026-07-26", "2026-07-27"]);
  expect(out.thisWeek).toEqual(["2026-07-13", "2026-07-19"]);
  expect(out.nextWeek).toEqual(["2026-07-20", "2026-07-26"]);
  expect(out.later).toEqual(["2026-07-27"]);
});

test("on a Sunday, this week is only today", () => {
  // The band shrinks as the week goes on, which is the honest reading of
  // how much room is left in it. A Sunday-first weekday index would make
  // this the *start* of a full week — the opposite claim.
  const out = bands("2026-07-19", ["2026-07-19", "2026-07-20", "2026-07-26", "2026-07-27"]);
  expect(out.thisWeek).toEqual(["2026-07-19"]);
  expect(out.nextWeek).toEqual(["2026-07-20", "2026-07-26"]);
  expect(out.later).toEqual(["2026-07-27"]);
});

test("on a Wednesday, the boundaries land where the calendar does", () => {
  // The case every view fixture happens to use, kept so the common path
  // is pinned too.
  const out = bands("2026-07-15", ["2026-07-15", "2026-07-19", "2026-07-20", "2026-07-26"]);
  expect(out.thisWeek).toEqual(["2026-07-15", "2026-07-19"]);
  expect(out.nextWeek).toEqual(["2026-07-20", "2026-07-26"]);
  expect(out.later).toEqual([]);
});

test("something due today is in this week, not behind it", () => {
  expect(bands("2026-07-15", ["2026-07-15"]).thisWeek).toEqual(["2026-07-15"]);
});

test("the bands hold across a month boundary", () => {
  // Wed 29 Jul: the week runs to Sun 2 Aug, the next to Sun 9 Aug.
  const out = bands("2026-07-29", ["2026-08-02", "2026-08-03", "2026-08-09", "2026-08-10"]);
  expect(out.thisWeek).toEqual(["2026-08-02"]);
  expect(out.nextWeek).toEqual(["2026-08-03", "2026-08-09"]);
  expect(out.later).toEqual(["2026-08-10"]);
});
