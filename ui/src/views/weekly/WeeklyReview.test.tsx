// The Weekly Review stepper against a fixture shaped like the ts-rs
// bindings: the seeded Wins composer, non-linear dot navigation, the
// exact save_weekly_section wire string, the muted stuck-project line,
// the focus quick-pick, and the stop-anywhere reassurance.
import { afterEach, expect, test } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ToastProvider } from "../../shell/Toasts";
import type { WeeklyBundle } from "../../api/bindings/WeeklyBundle";
import WeeklyReview from "./WeeklyReview";

const FIXTURE: WeeklyBundle = {
  week_of: "2026-07-06",
  today: "2026-07-08",
  weekly: {
    exists: false,
    wins: null,
    challenges: null,
    one_improvement: null,
    this_weeks_goal: null,
  },
  completed_actions: [
    { slug: "wire-reader", project: "alpha", title: "Wire the reader", completed: "2026-07-08" },
  ],
  logs: [{ date: "2026-07-07", time: "09:00:00", text: "paired on the parser" }],
  projects: [
    { slug: "alpha", status: "active", context: "work", state_snippet: "Underway.", top_action: null },
  ],
  stuck: [{ slug: "alpha", days_unchanged: 12n }],
  commitments: [],
  stewardships: [],
};

/** mockIPC that serves the bundle and records every call so the tests
 * can assert the exact command + args the UI sends. */
function mockWithCapture(): Array<{ cmd: string; args: unknown }> {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_weekly_bundle") return FIXTURE;
    return undefined;
  });
  return calls;
}

function renderView() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <ToastProvider>
          <WeeklyReview />
        </ToastProvider>
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("opens on step 1 with wins seeded from completed actions", async () => {
  mockWithCapture();
  renderView();

  const wins = (await screen.findByLabelText("This week's wins")) as HTMLTextAreaElement;
  // Seed composed from the week's completed action.
  expect(wins.value).toContain("- Completed: Wire the reader (alpha)");
});

test("dots navigate non-linearly — jump straight from Wins to Focus", async () => {
  mockWithCapture();
  renderView();

  await screen.findByLabelText("This week's wins");
  // Skip past steps 2-4 directly to Focus (step 5) via its dot.
  screen.getByRole("button", { name: "Focus" }).click();
  expect(await screen.findByLabelText("Next week's focus")).toBeDefined();

  // And back to Wins.
  screen.getByRole("button", { name: "Wins" }).click();
  expect(await screen.findByLabelText("This week's wins")).toBeDefined();
});

test("saving wins invokes save_weekly_section with section 'wins'", async () => {
  const calls = mockWithCapture();
  renderView();

  await screen.findByLabelText("This week's wins");
  screen.getByRole("button", { name: "Save wins" }).click();

  await screen.findByText("Wins saved.");
  const save = calls.find((c) => c.cmd === "save_weekly_section");
  expect(save?.args).toMatchObject({ section: "wins" });
});

test("a stuck project shows the muted staleness line", async () => {
  mockWithCapture();
  renderView();

  await screen.findByLabelText("This week's wins");
  screen.getByRole("button", { name: "Projects" }).click();

  expect(await screen.findByText("state untouched for 12 days")).toBeDefined();
});

test("focus quick-pick fills the input with the project slug", async () => {
  mockWithCapture();
  renderView();

  await screen.findByLabelText("This week's wins");
  screen.getByRole("button", { name: "Focus" }).click();

  const input = (await screen.findByLabelText("Next week's focus")) as HTMLInputElement;
  expect(input.value).toBe("");
  screen.getByRole("button", { name: "alpha" }).click();
  await waitFor(() => expect(input.value).toBe("alpha"));
});

test("the stop-anywhere line appears only after a save", async () => {
  mockWithCapture();
  renderView();

  await screen.findByLabelText("This week's wins");
  expect(screen.queryByText(/you can stop here/)).toBeNull();

  screen.getByRole("button", { name: "Save wins" }).click();
  expect(await screen.findByText(/you can stop here — it's already saved/)).toBeDefined();
});
