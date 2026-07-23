// Calendar view (#340): the embedded panel renders the selected day's
// note, a note-less day/week/month shows the calm empty state, and the
// quick-nav fires the right backend read with the backend-stamped date.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
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
        <ToastProvider>
          <ReaderProvider>
            <Calendar />
          </ReaderProvider>
        </ToastProvider>
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

  // Clicking Today returns to today's note (the heading flips back from the
  // neighbour's empty state — the load-bearing proof the jump landed).
  fireEvent.click(todayBtn);
  expect(await screen.findByRole("heading", { name: "Wednesday" })).toBeDefined();
  // Focus is handed to the picker toggle, not dropped to the body, since
  // Today disabled itself on landing.
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "The month" }));
});

test("Today is enabled on today's weekly view and switches back to the day", async () => {
  installMock([]);
  renderView();

  await screen.findByRole("heading", { name: "Wednesday" });
  // Switch to today's WEEKLY note — still today's date, but not the day view.
  fireEvent.click(screen.getByRole("button", { name: "Week" }));
  await screen.findByRole("heading", { name: "Week 29" });

  // Today stays enabled here (mode !== "daily"); clicking it returns to the
  // day view of today.
  const todayBtn = screen.getByRole("button", { name: "Today" }) as HTMLButtonElement;
  expect(todayBtn.disabled).toBe(false);
  fireEvent.click(todayBtn);
  expect(await screen.findByRole("heading", { name: "Wednesday" })).toBeDefined();
});

test("today's daily panel has an add-log input that appends via log_quick", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView();

  await screen.findByRole("heading", { name: "Wednesday" });
  const input = screen.getByLabelText("Log entry");
  fireEvent.change(input, { target: { value: "shipped v0.29.2" } });
  fireEvent.click(screen.getByRole("button", { name: "Add log" }));

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "log_quick");
    expect(call?.args).toMatchObject({ text: "shipped v0.29.2" });
  });
});

test("the add-log input is hidden off-today even when that day has a note", async () => {
  // The neighbour (16th) HAS a note, so this proves the composer is gated on
  // `today` — not merely on note-presence (an empty day would hide it anyway).
  mockIPC((cmd, args) => {
    if (cmd === "get_today") return "2026-07-15";
    if (cmd === "list_daily_dates") return ["2026-07-15", "2026-07-16"];
    if (cmd === "read_daily") {
      const date = (args as { date: string }).date;
      const day: Record<string, { title: string; next: string; prev: string }> = {
        "2026-07-15": { title: "Wednesday", next: "2026-07-16", prev: "2026-07-14" },
        "2026-07-16": { title: "Thursday", next: "2026-07-17", prev: "2026-07-15" },
      };
      const d = day[date] ?? { title: "Day", next: date, prev: date };
      return {
        date,
        exists: true,
        markdown: `# ${d.title}\n\nContent.`,
        path: `journal/2026/daily/${date}.md`,
        prev_date: d.prev,
        next_date: d.next,
        week_of: "2026-07-13",
        month: "2026-07",
      };
    }
    return undefined;
  });
  renderView();

  // Today (15th): the composer is present.
  await screen.findByRole("heading", { name: "Wednesday" });
  expect(screen.getByLabelText("Log entry")).toBeDefined();

  // Step to the 16th — its note renders, but the composer is gone.
  fireEvent.click(screen.getByRole("button", { name: "Next day ›" }));
  await screen.findByRole("heading", { name: "Thursday" });
  expect(screen.queryByLabelText("Log entry")).toBeNull();
});

test("the month grid is a hideable picker, collapsing once a day is chosen", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView();

  // The note leads: the grid isn't shown until summoned (day 15 is the
  // one note-bearing cell in the fixture, so its marker stands in for the
  // grid's presence).
  await screen.findByRole("heading", { name: "Wednesday" });
  const pick = screen.getByRole("button", { name: "The month" });
  expect(screen.queryByRole("button", { name: /has a note/ })).toBeNull();

  // "The month" reveals the grid.
  fireEvent.click(pick);
  const day = await screen.findByRole("button", { name: /15,.*has a note/ });

  // Choosing a day reads it and re-collapses the picker.
  fireEvent.click(day);
  expect(screen.getByRole("button", { name: "The month" })).toBeDefined();
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


/** Pretend the window is wide enough for two columns. jsdom has no
 * layout, so the breakpoint is read through `matchMedia` rather than a
 * Tailwind class — which is also the only way it can be asserted. */
function setWide(wide: boolean) {
  window.matchMedia = ((query: string) =>
    ({
      matches: wide,
      media: query,
      addEventListener() {},
      removeEventListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia;
}

test("on a wide window the month is pinned beside the note, not summoned over it", async () => {
  // The overview is the reason a calendar view exists. It used to be
  // collapsed by default and to hide itself again the moment you used it.
  setWide(true);
  try {
    installMock([]);
    renderView();

    // Visible with nothing clicked, and no toggle offered — there is
    // nothing to toggle.
    expect(await screen.findByRole("button", { name: /15,.*has a note/ })).toBeDefined();
    expect(screen.queryByRole("button", { name: /The month/ })).toBeNull();
  } finally {
    setWide(false);
  }
});

test("a pinned month does not vanish when you choose a day", async () => {
  setWide(true);
  try {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    installMock(calls);
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /15,.*has a note/ }));

    expect(screen.getByRole("button", { name: /15,.*has a note/ })).toBeDefined();
    const reads = calls
      .filter((c) => c.cmd === "read_daily")
      .map((c) => (c.args as { date: string }).date);
    expect(reads).toContain("2026-07-15");
  } finally {
    setWide(false);
  }
});

test("today is marked in the grid, from the date the backend stamped", async () => {
  setWide(true);
  try {
    installMock([]);
    renderView();
    expect(await screen.findByRole("button", { name: /15, today/ })).toBeDefined();
  } finally {
    setWide(false);
  }
});
