// Calendar view (#340): the embedded panel renders the selected day's
// note, a note-less day/week/month shows the calm empty state, and the
// quick-nav fires the right backend read with the backend-stamped date.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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
 * empty state; `weeklyExists` does the same for the weekly read (so the
 * week empty-state branch can be exercised). */
function installMock(
  calls: Array<{ cmd: string; args: unknown }>,
  { dailyExists = true, weeklyExists = true }: { dailyExists?: boolean; weeklyExists?: boolean } = {},
) {
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
          exists: weeklyExists,
          markdown: weeklyExists ? "# Week 29\n\nThe week in review." : "",
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

test("the Today button jumps back to today, and is disabled while already on it", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView();

  await screen.findByRole("heading", { name: "Wednesday" });
  // On today's day, the jump is a no-op — the button is disabled.
  expect((screen.getByRole("button", { name: "Today" }) as HTMLButtonElement).disabled).toBe(true);

  // Step to the next day (a note-less neighbour); now Today is live.
  fireEvent.click(screen.getByRole("button", { name: "Next day ›" }));
  await screen.findByText("No note for this day yet.");
  const todayBtn = screen.getByRole("button", { name: "Today" }) as HTMLButtonElement;
  expect(todayBtn.disabled).toBe(false);

  // Clicking Today reads today's note again and shows it.
  fireEvent.click(todayBtn);
  expect(await screen.findByRole("heading", { name: "Wednesday" })).toBeDefined();
  const reads = calls
    .filter((c) => c.cmd === "read_daily")
    .map((c) => (c.args as { date: string }).date);
  expect(reads).toContain("2026-07-15");
});

test("the month grid is a hideable picker, collapsing once a day is chosen", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView();

  // The note leads: the grid isn't shown until summoned (day 15 is the
  // one note-bearing cell in the fixture, so its marker stands in for the
  // grid's presence).
  await screen.findByRole("heading", { name: "Wednesday" });
  const pick = screen.getByRole("button", { name: "Pick a date" });
  expect(screen.queryByRole("button", { name: /has a note/ })).toBeNull();

  // "Pick a date" reveals the month grid.
  fireEvent.click(pick);
  const day = await screen.findByRole("button", { name: /15, has a note/ });

  // Choosing a day reads it and re-collapses the picker.
  fireEvent.click(day);
  expect(screen.getByRole("button", { name: "Pick a date" })).toBeDefined();
  expect(screen.queryByRole("button", { name: /has a note/ })).toBeNull();
  const reads = calls.filter((c) => c.cmd === "read_daily").map((c) => (c.args as { date: string }).date);
  expect(reads).toContain("2026-07-15");
});

test("a note-less day shows the calm empty state, not an error", async () => {
  installMock([], { dailyExists: false });
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

test("the week jump shows the calm empty state when the week has no note", async () => {
  installMock([], { weeklyExists: false });
  renderView();

  await screen.findByRole("heading", { name: "Wednesday" });
  (await screen.findByRole("button", { name: "Week" })).click();

  expect(await screen.findByText("No note for this week yet.")).toBeDefined();
});

test("the month jump reads read_monthly and shows the month empty state", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView();

  await screen.findByRole("heading", { name: "Wednesday" });
  (await screen.findByRole("button", { name: "Month" })).click();

  // The monthly fixture is note-less, so the month empty-state copy
  // renders — and read_monthly is invoked at the backend-stamped month.
  expect(await screen.findByText("No note for this month yet.")).toBeDefined();
  const monthly = calls.find((c) => c.cmd === "read_monthly");
  expect(monthly?.args).toMatchObject({ month: "2026-07" });
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

test("prev-day steps to the neighbour the backend stamped", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView();

  await screen.findByRole("heading", { name: "Wednesday" });
  (await screen.findByRole("button", { name: "‹ Prev day" })).click();

  // The panel reads the previous day (2026-07-14), a note-less day.
  expect(await screen.findByText("No note for this day yet.")).toBeDefined();
  const reads = calls.filter((c) => c.cmd === "read_daily").map((c) => (c.args as { date: string }).date);
  expect(reads).toContain("2026-07-14");
});
