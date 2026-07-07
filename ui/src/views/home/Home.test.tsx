// Smoke for the frontend test stack itself (M1 review condition:
// verify Testing Library + @tauri-apps/api/mocks run under Node/jsdom
// before committing to the stack) — and for Home's three render
// states against a fixture shaped like the ts-rs bindings.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ToastProvider } from "../../shell/Toasts";
import type { OrientationView } from "../../api/bindings/OrientationView";
import Home from "./Home";

const FIXTURE: OrientationView = {
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

// A variant carrying a second, lower-energy action so the energy
// filter surfaces a *different* bullet — the reset-on-swap path. Kept
// separate from FIXTURE so the deep-only no-match test stays valid.
const FIXTURE_TWO_ACTIONS: OrientationView = {
  ...FIXTURE,
  projects: [
    {
      ...FIXTURE.projects[0],
      actions: [
        { text: "Draft methods (deep)", energy: "deep", attached: null },
        { text: "File receipts (light)", energy: "light", attached: null },
      ],
    },
  ],
};

function renderHome() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <Home />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  // Auto-cleanup needs vitest globals, which stay off — clean by hand.
  cleanup();
  clearMocks();
});

test("renders commitments, project cards, and the lapsed line", async () => {
  mockIPC((cmd) => {
    if (cmd === "get_orientation") return FIXTURE;
    throw new Error(`unexpected command ${cmd}`);
  });

  renderHome();

  expect(await screen.findByText("alpha")).toBeDefined();
  expect(screen.getByText("submit-report")).toBeDefined();
  expect(screen.getByText(/Draft methods/)).toBeDefined();
  expect(screen.getByText(/quietly lapsed/)).toBeDefined();
});

test("backend failure renders the calm error state", async () => {
  mockIPC(() => {
    throw { kind: "internal", data: "internal error" };
  });

  renderHome();

  expect(await screen.findByText(/could not be read/)).toBeDefined();
});

test("empty vault renders the warm empty state, not blanks", async () => {
  mockIPC((cmd) =>
    cmd === "get_orientation"
      ? ({ today: "2026-07-07", commitments: [], projects: [], lapsed_habits: [] } as OrientationView)
      : undefined,
  );

  renderHome();

  expect(await screen.findByText(/Nothing active/)).toBeDefined();
  expect(screen.queryByText(/quietly lapsed/)).toBeNull();
});

test("energy filter never blanks a card (no-match rule)", async () => {
  mockIPC((cmd) => (cmd === "get_orientation" ? FIXTURE : undefined));
  const { getByRole } = renderHome();

  await screen.findByText("alpha");
  // The only action is (deep); filtering to light must keep it
  // visible behind the muted smallest-step note.
  getByRole("button", { name: "light" }).click();

  expect(await screen.findByText(/no light action here/)).toBeDefined();
  expect(screen.getByText(/Draft methods/)).toBeDefined();
});

test("Start invokes start_action and flips to the started note", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_orientation") return FIXTURE;
    return undefined;
  });
  renderHome();

  (await screen.findByRole("button", { name: "Start" })).click();

  expect(await screen.findByText(/in today's log/)).toBeDefined();
  const started = calls.find((c) => c.cmd === "start_action");
  expect(started?.args).toMatchObject({ project: "alpha", action: "Draft methods (deep)" });
});

test("done optimistically removes the action and calls complete_action", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "get_orientation") return FIXTURE;
    return undefined;
  });
  renderHome();

  (await screen.findByRole("button", { name: /Mark done/ })).click();

  // The success toast confirms the mutation settled.
  expect(await screen.findByText(/one step further/)).toBeDefined();
  expect(calls).toContain("complete_action");
});

test("typing j/k in the state editor doesn't steal focus to a card", async () => {
  mockIPC((cmd) => (cmd === "get_orientation" ? FIXTURE : undefined));
  renderHome();

  // Open the inline Current State editor, then type navigation-shaped
  // letters into the textarea — the roving handler must bail on
  // editable targets rather than hijack j/k as card moves.
  (await screen.findByRole("button", { name: /Edit the current state/ })).click();
  const textarea = await screen.findByLabelText(/Current state of alpha/);
  textarea.focus();
  expect(document.activeElement).toBe(textarea);

  fireEvent.keyDown(textarea, { key: "j" });
  fireEvent.keyDown(textarea, { key: "k" });

  expect(document.activeElement).toBe(textarea);
});

test("started note clears when the filter surfaces a different action", async () => {
  mockIPC((cmd) => (cmd === "get_orientation" ? FIXTURE_TWO_ACTIONS : undefined));
  const { getByRole } = renderHome();

  await screen.findByText("alpha");
  (await screen.findByRole("button", { name: "Start" })).click();
  expect(await screen.findByText(/in today's log/)).toBeDefined();

  // Filtering to light surfaces "File receipts", a different bullet:
  // the optimistic started note no longer applies, so Start returns.
  getByRole("button", { name: "light" }).click();
  expect(await screen.findByRole("button", { name: "Start" })).toBeDefined();
});
