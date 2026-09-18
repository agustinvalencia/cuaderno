// `candidatesFor` — what the Wins step offers as a starting point.
//
// The interesting case is the overlap: a completion writes a
// `completed_actions` entry AND a log line, so the two sources listed
// raw offered every win twice (#586 made that the common case, because
// an inline bullet is the default form of an action).
import { describe, expect, it } from "vitest";
import type { WeeklyBundle } from "../../api/bindings/WeeklyBundle";
import { candidatesFor } from "./WinsStep";

/** A bundle carrying only what `candidatesFor` reads. */
function bundle(
  completed: WeeklyBundle["completed_actions"],
  logs: WeeklyBundle["logs"],
): WeeklyBundle {
  return { completed_actions: completed, logs } as WeeklyBundle;
}

const LOG = (text: string) => ({ date: "2026-07-07", time: "09:00:00", text });

describe("candidatesFor", () => {
  it("offers a bullet completion once, not once per trace", () => {
    const got = candidatesFor(
      bundle(
        [{ slug: null, project: "alpha", title: "Rerun the ablation", completed: "2026-07-07" }],
        [LOG("action done on [[alpha]] — Rerun the ablation (deep)")],
      ),
    );

    expect(got).toEqual(["Completed: Rerun the ablation (alpha)"]);
  });

  it("offers a note-backed completion once too", () => {
    const got = candidatesFor(
      bundle(
        [
          {
            slug: "wire-reader",
            project: "alpha",
            title: "Wire the reader",
            completed: "2026-07-08",
          },
        ],
        [LOG("action done on [[alpha]] — [[actions/wire-reader]] (deep)")],
      ),
    );

    expect(got).toEqual(["Completed: Wire the reader (alpha)"]);
  });

  it("keeps ordinary log lines", () => {
    const got = candidatesFor(
      bundle([], [LOG("paired on the parser"), LOG("read the Hoffman paper")]),
    );

    expect(got).toEqual(["paired on the parser", "read the Hoffman paper"]);
  });

  it("keeps a dropped action — a different prefix, and never a completion", () => {
    // `action dropped on …` never reaches `completed_actions`, so
    // filtering it here would lose it from the week's record entirely.
    const got = candidatesFor(bundle([], [LOG("action dropped on [[alpha]] — Abandoned (light)")]));

    expect(got).toEqual(["action dropped on [[alpha]] — Abandoned (light)"]);
  });

  it("puts completions first, so a candidate's index still finds its date", () => {
    // The renderer reads `completed_actions[index]` for the date
    // subtitle, which only holds while completions lead the list.
    const completed = [
      { slug: null, project: "alpha", title: "First", completed: "2026-07-07" },
      { slug: null, project: "beta", title: "Second", completed: "2026-07-08" },
    ];
    const got = candidatesFor(
      bundle(completed, [
        LOG("action done on [[alpha]] — First (deep)"),
        LOG("paired on the parser"),
      ]),
    );

    expect(got).toEqual([
      "Completed: First (alpha)",
      "Completed: Second (beta)",
      "paired on the parser",
    ]);
    got.slice(0, completed.length).forEach((_, index) => {
      expect(completed[index]).toBeDefined();
    });
  });
});
