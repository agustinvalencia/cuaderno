// The Weekly Review stepper against a fixture shaped like the ts-rs
// bindings: the seeded Wins composer, non-linear dot navigation (with
// drafts surviving the jumps), the exact save_weekly_section wire
// string, the focus save targeting NEXT week, the muted stuck-project
// line, the read-only lookahead, and the two-tier stop-anywhere
// reassurance.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
import type { WeeklyBundle } from "../../api/bindings/WeeklyBundle";
import WeeklyReview from "./WeeklyReview";

const FIXTURE: WeeklyBundle = {
  week_of: "2026-07-06",
  next_week_of: "2026-07-13",
  today: "2026-07-08",
  weekly: {
    exists: false,
    wins: null,
    challenges: null,
    one_improvement: null,
    this_weeks_goal: null,
  },
  next_week_goal: null,
  completed_actions: [
    { slug: "wire-reader", project: "alpha", title: "Wire the reader", completed: "2026-07-08" },
  ],
  logs: [{ date: "2026-07-07", time: "09:00:00", text: "paired on the parser" }],
  projects: [
    { slug: "alpha", status: "active", context: "work", state_snippet: "Underway.", top_action: null },
  ],
  stuck: [{ slug: "alpha", days_unchanged: 12n }],
  // A completable source (standalone commitment) — outside the review
  // its row would carry a done button, so the read-only lookahead test
  // has something real to suppress.
  commitments: [
    {
      date: "2026-07-10",
      title: "Renew passport",
      source: { kind: "standalone_commitment", slug: "renew-passport" },
      is_overdue: false,
      context: "personal",
    },
  ],
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
          {/* The lookahead's origin chips call useReader, so the review
              needs the same reader host the shell provides. */}
          <ReaderProvider>
            <WeeklyReview />
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

test("the focus save targets NEXT week's note, not the reviewed week", async () => {
  const calls = mockWithCapture();
  renderView();

  await screen.findByLabelText("This week's wins");
  screen.getByRole("button", { name: "Focus" }).click();

  const input = (await screen.findByLabelText("Next week's focus")) as HTMLInputElement;
  fireEvent.change(input, { target: { value: "Start M7" } });
  screen.getByRole("button", { name: "Set focus" }).click();

  await screen.findByText("Focus set.");
  const save = calls.find((c) => c.cmd === "save_weekly_section");
  // weekOf is the bundle's next_week_of — the week the goal anchors —
  // never the reviewed week's Monday (whose own goal must survive).
  expect(save?.args).toMatchObject({
    section: "this-weeks-goal",
    weekOf: "2026-07-13",
    content: "Start M7",
  });
});

test("a wins draft survives a non-linear jump away and back", async () => {
  // All five steps stay mounted (visibility-toggled), so an unsaved
  // draft in the uncontrolled Wins textarea must not be discarded by
  // hopping to Focus and back.
  mockWithCapture();
  renderView();

  const wins = (await screen.findByLabelText("This week's wins")) as HTMLTextAreaElement;
  fireEvent.change(wins, { target: { value: "- Fixed the flaky test at last" } });

  screen.getByRole("button", { name: "Focus" }).click();
  await screen.findByLabelText("Next week's focus");
  screen.getByRole("button", { name: "Wins" }).click();

  const winsAgain = (await screen.findByLabelText("This week's wins")) as HTMLTextAreaElement;
  expect(winsAgain.value).toBe("- Fixed the flaky test at last");
});

test("the lookahead is read-only — origin chips stay, done buttons don't", async () => {
  mockWithCapture();
  renderView();

  await screen.findByLabelText("This week's wins");
  screen.getByRole("button", { name: "Lookahead" }).click();

  // The completable standalone commitment renders with its origin chip...
  expect(await screen.findByText("Renew passport")).toBeDefined();
  expect(screen.getByRole("button", { name: "renew-passport" })).toBeDefined();
  // ...but no completion affordance: the step's copy says "nothing to
  // add here", and the timeline honours it via readOnly.
  expect(screen.queryByRole("button", { name: /Mark done/ })).toBeNull();
});

test("an actual write earns the 'already saved' stop line", async () => {
  mockWithCapture();
  renderView();

  await screen.findByLabelText("This week's wins");
  expect(screen.queryByText(/you can stop/)).toBeNull();

  screen.getByRole("button", { name: "Save wins" }).click();
  expect(await screen.findByText(/you can stop here — it's already saved/)).toBeDefined();
});

test("a read-only looked-at mark earns only the softer stop line", async () => {
  // Honesty tier two: nothing was written yet, so the flow must not
  // claim "it's already saved" — only that stopping is fine.
  mockWithCapture();
  renderView();

  await screen.findByLabelText("This week's wins");
  screen.getByRole("button", { name: "Lookahead" }).click();
  (await screen.findByRole("button", { name: "Looked at these" })).click();

  expect(
    await screen.findByText(/you can stop anytime — nothing here demands finishing/),
  ).toBeDefined();
  expect(screen.queryByText(/it's already saved/)).toBeNull();
});
