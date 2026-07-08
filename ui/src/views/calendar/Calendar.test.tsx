// Calendar view (#340): the embedded panel renders the selected day's
// note, a note-less day/week/month shows the calm empty state, and the
// quick-nav fires the right backend read with the backend-stamped date.
import { afterEach, expect, test } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ReaderProvider } from "../../shell/reader";
import Calendar from "./Calendar";

// today = 15 Jul 2026 (a Wednesday). Its backend-stamped neighbours:
// prev 14th, next 16th, week Monday the 13th, month 2026-07.
const TODAY_DAILY = {
  date: "2026-07-15",
  exists: true,
  markdown: "# Wednesday\n\nShipped the calendar grid.",
  path: "journal/2026/daily/2026-07-15.md",
  prev_date: "2026-07-14",
  next_date: "2026-07-16",
  week_of: "2026-07-13",
  month: "2026-07",
};

/** A mockIPC handler recording every call, returning calendar fixtures.
 * `dailyExists` toggles the daily read between a populated note and the
 * empty state. */
function installMock(calls: Array<{ cmd: string; args: unknown }>, dailyExists = true) {
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    switch (cmd) {
      case "get_today":
        return "2026-07-15";
      case "list_daily_dates":
        return ["2026-07-15"];
      case "read_daily": {
        const date = (args as { date: string }).date;
        if (date === "2026-07-15") {
          return { ...TODAY_DAILY, exists: dailyExists, markdown: dailyExists ? TODAY_DAILY.markdown : "" };
        }
        // A neighbour day with no note — the empty state.
        return {
          date,
          exists: false,
          markdown: "",
          path: `journal/2026/daily/${date}.md`,
          prev_date: "2026-07-15",
          next_date: "2026-07-17",
          week_of: "2026-07-13",
          month: "2026-07",
        };
      }
      case "read_weekly":
        return {
          week_of: (args as { weekOf: string }).weekOf,
          exists: true,
          markdown: "# Week 29\n\nThe week in review.",
          path: "journal/2026/weekly/2026-W29.md",
        };
      case "read_monthly":
        return {
          month: (args as { month: string }).month,
          exists: false,
          markdown: "",
          path: "journal/2026/monthly/2026-07.md",
        };
      default:
        return undefined;
    }
  });
}

function renderView() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <ReaderProvider>
          <Calendar />
        </ReaderProvider>
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("opens on today's note in the embedded panel", async () => {
  installMock([]);
  renderView();

  // The panel renders the daily markdown (an h1 from the note body).
  expect(await screen.findByRole("heading", { name: "Wednesday" })).toBeDefined();
  // The panel title is the full date — the weekday + a comma (locale
  // orders day/month differently, so match the stable parts only).
  expect(screen.getByRole("heading", { name: /Wednesday,.*2026/ })).toBeDefined();
});

test("a note-less day shows the calm empty state, not an error", async () => {
  installMock([], false);
  renderView();

  expect(await screen.findByText("No note for this day yet.")).toBeDefined();
  // The open-in-editor path is offered.
  expect(screen.getByRole("button", { name: "journal/2026/daily/2026-07-15.md" })).toBeDefined();
});

test("the week jump reads the weekly note at the backend-stamped week_of", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView();

  await screen.findByRole("heading", { name: "Wednesday" });
  (await screen.findByRole("button", { name: "Week" })).click();

  // The weekly note loads in the same panel, read at week_of = the
  // Monday the daily read stamped (2026-07-13), never computed client-side.
  expect(await screen.findByRole("heading", { name: "Week 29" })).toBeDefined();
  const weekly = calls.find((c) => c.cmd === "read_weekly");
  expect(weekly?.args).toMatchObject({ weekOf: "2026-07-13" });
});

test("next-day steps to the neighbour the backend stamped", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView();

  await screen.findByRole("heading", { name: "Wednesday" });
  (await screen.findByRole("button", { name: "Next day ›" })).click();

  // The panel reads the next day (2026-07-16), which the fixture returns
  // as a note-less empty state.
  expect(await screen.findByText("No note for this day yet.")).toBeDefined();
  const reads = calls.filter((c) => c.cmd === "read_daily").map((c) => (c.args as { date: string }).date);
  expect(reads).toContain("2026-07-16");
});
