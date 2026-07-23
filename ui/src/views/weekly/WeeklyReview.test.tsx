// The Weekly Review stepper against a fixture shaped like the ts-rs
// bindings: the seeded Wins composer, non-linear dot navigation (with
// drafts surviving the jumps), the exact save_weekly_section wire
// string, the focus save targeting NEXT week, the muted stuck-project
// line, the read-only lookahead, and the two-tier stop-anywhere
// reassurance.
import { afterEach, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
import type { WeeklyBundle } from "../../api/bindings/WeeklyBundle";
import WeeklyReview from "./WeeklyReview";

// CodeMirror needs layout APIs jsdom lacks; stub the Wins editor with a
// textarea that mirrors its seed-once + onChange contract and forwards the
// accessible label, so the seeded-composer assertions still resolve it.
vi.mock("../../components/markdown/MarkdownEditor", () => ({
  default: ({
    initialDoc,
    ariaLabel,
    onChange,
  }: {
    initialDoc: string;
    ariaLabel?: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label={ariaLabel}
      defaultValue={initialDoc}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

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
  stuck: [{ slug: "alpha", days_unchanged: 12 }],
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

test("opens on Wins, offering the week rather than pasting it", async () => {
  mockWithCapture();
  renderView();

  // The completions are candidates you add, not prose already written
  // for you — and not a decorative card above a box holding the same
  // text again.
  const from = within(await screen.findByRole("region", { name: "From your week" }));
  expect(from.getByText("Completed: Wire the reader (alpha)")).toBeDefined();
  expect(from.getByRole("button", { name: /^Add: Completed: Wire the reader/ })).toBeDefined();
});

test("dots navigate non-linearly — jump straight from Wins to Focus", async () => {
  mockWithCapture();
  renderView();

  await screen.findByRole("region", { name: "Your wins" });
  // Skip past steps 2-4 directly to Focus (step 5) via its dot.
  screen.getByRole("button", { name: "Focus" }).click();
  expect(await screen.findByLabelText("Next week's focus")).toBeDefined();

  // And back to Wins.
  screen.getByRole("button", { name: "Wins" }).click();
  expect(await screen.findByRole("region", { name: "Your wins" })).toBeDefined();
});

test("saving wins invokes save_weekly_section with section 'wins'", async () => {
  const calls = mockWithCapture();
  renderView();

  await screen.findByRole("region", { name: "From your week" });
  fireEvent.click(screen.getByRole("button", { name: /^Add: Completed: Wire the reader/ }));
  screen.getByRole("button", { name: "Save wins" }).click();

  await screen.findByText("Wins saved.");
  const save = calls.find((c) => c.cmd === "save_weekly_section");
  expect(save?.args).toMatchObject({ section: "wins" });
});

test("a win can be edited in place, and that is what gets saved", async () => {
  // The old blob could only be edited as a whole; fixing one word meant
  // a text-selection exercise in a sixteen-line box.
  const calls = mockWithCapture();
  renderView();

  await screen.findByRole("region", { name: "From your week" });
  fireEvent.click(screen.getByRole("button", { name: /^Add: Completed: Wire the reader/ }));
  fireEvent.change(screen.getByLabelText(/^Win: Completed: Wire the reader/), {
    target: { value: "Shipped the reader" },
  });
  screen.getByRole("button", { name: "Save wins" }).click();

  await screen.findByText("Wins saved.");
  const save = calls.find((c) => c.cmd === "save_weekly_section");
  expect(save?.args).toMatchObject({ section: "wins", content: "- [x] Shipped the reader" });
});

test("a stuck project shows the muted staleness line", async () => {
  mockWithCapture();
  renderView();

  await screen.findByRole("region", { name: "Your wins" });
  screen.getByRole("button", { name: "Projects" }).click();

  expect(await screen.findByText("state untouched for 12 days")).toBeDefined();
});

test("focus quick-pick fills the input with the project slug", async () => {
  mockWithCapture();
  renderView();

  await screen.findByRole("region", { name: "Your wins" });
  screen.getByRole("button", { name: "Focus" }).click();

  const input = (await screen.findByLabelText("Next week's focus")) as HTMLInputElement;
  expect(input.value).toBe("");
  screen.getByRole("button", { name: "alpha" }).click();
  await waitFor(() => expect(input.value).toBe("alpha"));
});

test("the focus save targets NEXT week's note, not the reviewed week", async () => {
  const calls = mockWithCapture();
  renderView();

  await screen.findByRole("region", { name: "Your wins" });
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

test("an unsaved wins list survives a non-linear jump away and back", async () => {
  // All five steps stay mounted (visibility-toggled), so unsaved wins
  // must not be discarded by hopping to Focus and back.
  mockWithCapture();
  renderView();

  await screen.findByRole("region", { name: "From your week" });
  fireEvent.click(screen.getByRole("button", { name: /^Add: Completed: Wire the reader/ }));
  fireEvent.change(screen.getByLabelText(/^Win: Completed: Wire the reader/), {
    target: { value: "Fixed the flaky test at last" },
  });

  screen.getByRole("button", { name: "Focus" }).click();
  await screen.findByLabelText("Next week's focus");
  screen.getByRole("button", { name: "Wins" }).click();

  const back = (await screen.findByLabelText(
    "Win: Fixed the flaky test at last",
  )) as HTMLInputElement;
  expect(back.value).toBe("Fixed the flaky test at last");
});

test("the lookahead is read-only — origin chips stay, done buttons don't", async () => {
  mockWithCapture();
  renderView();

  await screen.findByRole("region", { name: "Your wins" });
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

  await screen.findByRole("region", { name: "From your week" });
  expect(screen.queryByText(/you can stop/)).toBeNull();

  // Nothing to save until there is a win: an empty list is not an
  // artefact, so the button does not pretend it would write one.
  expect((screen.getByRole("button", { name: "Save wins" }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  fireEvent.click(screen.getByRole("button", { name: /^Add: Completed: Wire the reader/ }));
  screen.getByRole("button", { name: "Save wins" }).click();
  expect(await screen.findByText(/you can stop here — it's already saved/)).toBeDefined();
});

test("a read-only looked-at mark earns only the softer stop line", async () => {
  // Honesty tier two: nothing was written yet, so the flow must not
  // claim "it's already saved" — only that stopping is fine.
  mockWithCapture();
  renderView();

  await screen.findByRole("region", { name: "Your wins" });
  screen.getByRole("button", { name: "Lookahead" }).click();
  (await screen.findByRole("button", { name: "Looked at these" })).click();

  expect(
    await screen.findByText(/you can stop anytime — nothing here demands finishing/),
  ).toBeDefined();
  expect(screen.queryByText(/it's already saved/)).toBeNull();
});

test("the steps are named, not five anonymous dots", async () => {
  // The names used to live only in `aria-label`, so a sighted user saw
  // five dots and had to click each one to learn what it was — the shape
  // of the review hidden until you had walked it.
  mockWithCapture();
  renderView();
  await screen.findByRole("region", { name: "Your wins" });

  const rail = within(screen.getByRole("navigation", { name: "Review steps" }));
  for (const label of ["Wins", "Projects", "Stewardships", "Lookahead", "Focus"]) {
    expect(rail.getByRole("button", { name: label }).textContent).toContain(label);
  }
  expect(rail.getByRole("button", { name: "Wins" }).getAttribute("aria-current")).toBe("step");
});

test("Back and Next walk the ritual without hunting for a dot", async () => {
  mockWithCapture();
  renderView();
  await screen.findByRole("region", { name: "Your wins" });

  expect((screen.getByRole("button", { name: "Back" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));

  const rail = within(screen.getByRole("navigation", { name: "Review steps" }));
  expect(rail.getByRole("button", { name: "Projects" }).getAttribute("aria-current")).toBe("step");
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  expect(rail.getByRole("button", { name: "Wins" }).getAttribute("aria-current")).toBe("step");
});

test("the stop-anywhere line sits above the step, not under a tall one", async () => {
  // It is needed exactly when the step is long enough to have pushed it
  // below the fold.
  mockWithCapture();
  renderView();
  await screen.findByRole("region", { name: "From your week" });
  fireEvent.click(screen.getByRole("button", { name: /^Add: Completed: Wire the reader/ }));
  screen.getByRole("button", { name: "Save wins" }).click();

  const line = await screen.findByText(/already saved/);
  const steps = screen.getByRole("region", { name: "Your wins" });
  expect(line.compareDocumentPosition(steps) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

test("a win can be removed, and the remaining ones are what save", async () => {
  const calls = mockWithCapture();
  renderView();
  await screen.findByRole("region", { name: "From your week" });

  fireEvent.click(screen.getByRole("button", { name: /^Add: Completed: Wire the reader/ }));
  fireEvent.click(screen.getByRole("button", { name: "Add your own" }));
  fireEvent.change(screen.getByLabelText("Win: new win"), { target: { value: "rested" } });
  fireEvent.click(screen.getByRole("button", { name: /^Remove: Completed: Wire the reader/ }));
  screen.getByRole("button", { name: "Save wins" }).click();

  await screen.findByText("Wins saved.");
  const save = calls.find((c) => c.cmd === "save_weekly_section");
  expect(save?.args).toMatchObject({ content: "- [x] rested" });
});

test("wins reorder, and the order is what is written", async () => {
  const calls = mockWithCapture();
  renderView();
  await screen.findByRole("region", { name: "From your week" });

  fireEvent.click(screen.getByRole("button", { name: /^Add: Completed: Wire the reader/ }));
  fireEvent.click(screen.getByRole("button", { name: "Add your own" }));
  fireEvent.change(screen.getByLabelText("Win: new win"), { target: { value: "rested" } });
  fireEvent.click(screen.getByRole("button", { name: "Move up: rested" }));
  screen.getByRole("button", { name: "Save wins" }).click();

  await screen.findByText("Wins saved.");
  const save = calls.find((c) => c.cmd === "save_weekly_section");
  expect((save?.args as { content: string }).content.split("\n")[0]).toBe("- [x] rested");
});

test("the project scan links to the project and names its next action", async () => {
  // You could read the scan and have no way to reach what it was about,
  // and the action the bundle already carried was never rendered.
  mockWithCapture();
  renderView();
  await screen.findByRole("region", { name: "Your wins" });
  screen.getByRole("button", { name: "Projects" }).click();

  const link = await screen.findByRole("link", { name: "alpha" });
  expect(link.getAttribute("href")).toBe("/projects/alpha");
});
