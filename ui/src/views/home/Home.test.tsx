// Today (#442): the Now band, the pick-one shortlist, and the daily note
// as the page. Fixtures are shaped like the ts-rs bindings, so a Rust type
// change breaks these at compile time.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import type { DailyView } from "../../api/bindings/DailyView";
import type { NowView } from "../../api/bindings/NowView";
import type { OrientationView } from "../../api/bindings/OrientationView";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
import Home from "./Home";

const ORIENTATION: OrientationView = {
  today: "2026-07-07",
  commitments: [
    {
      date: "2026-07-08",
      title: "submit-report",
      source: { kind: "project_milestone", slug: "alpha" },
      is_overdue: false,
      context: "work",
    },
  ],
  projects: [
    {
      slug: "alpha",
      status: "active",
      state_snippet: "Core loop underway.",
      top_action: { text: "Draft methods", energy: "deep" },
      context: "work",
      actions: [{ text: "Draft methods (deep)", energy: "deep", attached: null }],
    },
  ],
  lapsed_habits: [{ stewardship: "health", detail: "Swimming 1x/week — lapsed since March" }],
};

const TWO_ACTIONS: OrientationView = {
  ...ORIENTATION,
  projects: [
    {
      ...ORIENTATION.projects[0],
      actions: [
        { text: "Draft methods (deep)", energy: "deep", attached: null },
        { text: "File receipts (light)", energy: "light", attached: null },
      ],
    },
  ],
};

const DAILY: DailyView = {
  date: "2026-07-07",
  exists: true,
  markdown:
    "---\ndate: 2026-07-07\ntype: daily\n---\n\n# Tuesday\n\n## Intention\nFinish the draft.\n\n## Logs\n- **09:30**: started something\n",
  path: "journal/2026/daily/2026-07-07.md",
  prev_date: "2026-07-06",
  next_date: "2026-07-08",
  week_of: "2026-07-06",
  month: "2026-07",
};

const NOW: NowView = {
  project: "alpha",
  context: "work",
  action: "Draft methods (deep)",
  started: "09:30",
};

/** Install an IPC mock; `now` and `daily` default to the happy shapes. */
function installMock(
  calls: Array<{ cmd: string; args: unknown }>,
  overrides: { orientation?: OrientationView; now?: NowView | null; daily?: DailyView } = {},
) {
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_orientation") return overrides.orientation ?? ORIENTATION;
    if (cmd === "get_now") return overrides.now === undefined ? NOW : overrides.now;
    if (cmd === "read_daily") return overrides.daily ?? DAILY;
    return undefined;
  });
}

function renderHome() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter>
          <ReaderProvider>
            <Home />
          </ReaderProvider>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("the day's own note is the page", async () => {
  // It used to live one view away, behind the Calendar, while this page
  // showed cards restating the sidebar.
  installMock([]);
  renderHome();

  expect(await screen.findByText("Finish the draft.")).toBeDefined();
  expect(screen.getByText("Intention")).toBeDefined();
});

test("the Now band says what is open, and since when", async () => {
  installMock([]);
  renderHome();

  const band = await screen.findByLabelText("What you are working on");
  expect(band.textContent).toContain("Draft methods");
  expect(band.textContent).toContain("09:30");
});

test("with nothing started, the band points at the shortlist", async () => {
  // A blank morning gets the honest answer, not an empty frame.
  installMock([], { now: null });
  renderHome();

  expect(await screen.findByText(/Nothing started yet/)).toBeDefined();
});

test("Done on the band completes the open action", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderHome();

  fireEvent.click(await screen.findByRole("button", { name: "Done" }));

  await waitFor(() => {
    expect(calls.find((c) => c.cmd === "complete_action")?.args).toMatchObject({
      project: "alpha",
      action: "Draft methods (deep)",
    });
  });
});

test("Start logs the action so the band can read it back", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls, { now: null });
  renderHome();

  fireEvent.click(await screen.findByRole("button", { name: "Start" }));

  await waitFor(() => {
    expect(calls.find((c) => c.cmd === "start_action")?.args).toMatchObject({
      project: "alpha",
      action: "Draft methods (deep)",
    });
  });
});

test("the energy filter never blanks a row (no-match rule)", async () => {
  // A low-energy moment must not be met by an empty page: the project
  // still offers its best-available action, with a muted note.
  installMock([]);
  renderHome();

  fireEvent.click(await screen.findByRole("button", { name: "light" }));

  // Scoped to the shortlist: the Now band names the same action, since it
  // is what is currently open.
  const shortlist = within(screen.getByLabelText("Pick one thing"));
  expect(shortlist.getByText(/Draft methods/)).toBeDefined();
  expect(shortlist.getByText(/no light action here/)).toBeDefined();
});

test("the energy filter surfaces a matching action when there is one", async () => {
  installMock([], { orientation: TWO_ACTIONS });
  renderHome();

  fireEvent.click(await screen.findByRole("button", { name: "light" }));

  expect(screen.getByText(/File receipts/)).toBeDefined();
});

test("the quick-log input appends to today's log", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderHome();

  const input = await screen.findByLabelText("Log entry");
  fireEvent.change(input, { target: { value: "picked up the draft" } });
  fireEvent.click(screen.getByRole("button", { name: "Add log" }));

  await waitFor(() => {
    expect(calls.find((c) => c.cmd === "log_quick")?.args).toMatchObject({
      text: "picked up the draft",
    });
  });
});

test("commitments and the lapsed line still show", async () => {
  installMock([]);
  renderHome();

  expect(await screen.findByText("submit-report")).toBeDefined();
  expect(screen.getByText(/quietly lapsed/)).toBeDefined();
});

test("a day with no note yet says so rather than rendering nothing", async () => {
  installMock([], { daily: { ...DAILY, exists: false, markdown: "" } });
  renderHome();

  expect(await screen.findByText(/No note for today yet/)).toBeDefined();
});

test("an empty vault gets the warm empty state", async () => {
  installMock([], {
    orientation: { ...ORIENTATION, projects: [], commitments: [], lapsed_habits: [] },
    now: null,
  });
  renderHome();

  expect(await screen.findByText(/Nothing active/)).toBeDefined();
});
