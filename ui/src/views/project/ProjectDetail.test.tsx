// Project Detail: renders a composed fixture (actions, milestones,
// backlinks, log mentions), fires complete_action on done, and renders
// a parked project read-only (no add row, no done buttons).
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { ProjectDetail as ProjectDetailData } from "../../api/bindings/ProjectDetail";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
import ProjectDetail from "./ProjectDetail";

const ACTIVE: ProjectDetailData = {
  slug: "alpha",
  status: "active",
  context: "work",
  created: "2026-01-01",
  core_question: null,
  body_markdown: "## Current State\nGoing well.\n\n## Notes\nSome prose.",
  actions: [{ text: "Draft methods", energy: "deep", attached: null }],
  open_milestones: [{ name: "v1 ship", date: "2026-08-01", is_hard: true }],
  backlinks: {
    portfolios: ["portfolios/x/_index.md"],
    questions: [],
    evidence: [],
    actions: [],
    other: [],
  },
  log_mentions: [{ date: "2026-07-01", time: "09:00:00", text: "worked on alpha" }],
};

const PARKED: ProjectDetailData = {
  ...ACTIVE,
  status: "parked",
  // list_actions refuses parked projects, so the bundle carries none.
  actions: [],
};

function renderDetail(fixture: ProjectDetailData, onCall?: (cmd: string, args: unknown) => void) {
  mockIPC((cmd, args) => {
    onCall?.(cmd, args);
    if (cmd === "get_project") return fixture;
    return undefined;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={["/projects/alpha"]}>
          <ReaderProvider>
            <Routes>
              <Route path="/projects/:slug" element={<ProjectDetail />} />
            </Routes>
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

test("renders actions, milestones, backlinks, and log mentions", async () => {
  renderDetail(ACTIVE);
  expect(await screen.findByText("Draft methods")).toBeDefined();
  expect(screen.getByText("v1 ship")).toBeDefined();
  expect(screen.getByText("hard:")).toBeDefined();
  expect(screen.getByText("portfolios/x/_index.md")).toBeDefined();
  expect(screen.getByText(/worked on alpha/)).toBeDefined();
});

test("done fires complete_action for the bullet", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderDetail(ACTIVE, (cmd, args) => calls.push({ cmd, args }));
  fireEvent.click(await screen.findByRole("button", { name: /Mark done: Draft methods/ }));
  // Await the success toast so the (microtask-deferred) mutation has run.
  expect(await screen.findByText(/one step further/)).toBeDefined();
  const done = calls.find((c) => c.cmd === "complete_action");
  expect(done?.args).toMatchObject({ project: "alpha", action: "Draft methods" });
});

test("an ambiguous complete opens the picker; choosing re-invokes with the exact text", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  let completeCalls = 0;
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_project") return ACTIVE;
    if (cmd === "complete_action") {
      completeCalls += 1;
      // First attempt (the full bullet text is a substring of two
      // milestones' texts) comes back ambiguous; the retry succeeds.
      if (completeCalls === 1) {
        throw {
          kind: "ambiguous",
          data: {
            query: "Draft methods",
            candidates: ["Draft methods for section 1", "Draft methods for section 2"],
          },
        };
      }
      return undefined;
    }
    return undefined;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={["/projects/alpha"]}>
          <ReaderProvider>
            <Routes>
              <Route path="/projects/:slug" element={<ProjectDetail />} />
            </Routes>
          </ReaderProvider>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );

  fireEvent.click(await screen.findByRole("button", { name: /Mark done: Draft methods/ }));

  // The picker opens with the candidates — not a dead-end toast.
  expect(await screen.findByRole("dialog")).toBeDefined();
  const chosen = screen.getByRole("button", { name: "Draft methods for section 2" });
  fireEvent.click(chosen);

  // The command re-fires with the exact chosen string.
  await waitFor(() => {
    const completes = calls.filter((c) => c.cmd === "complete_action");
    expect(completes).toHaveLength(2);
    expect(completes[1].args).toMatchObject({
      project: "alpha",
      action: "Draft methods for section 2",
    });
  });
});

test("a parked project renders read-only — no add row, no done", async () => {
  renderDetail(PARKED);
  // The header still loads and shows the parked status.
  expect(await screen.findByText("parked")).toBeDefined();
  // No next-action add row and no waiting-on write affordances.
  expect(screen.queryByLabelText("New next action")).toBeNull();
  expect(screen.queryByLabelText("New waiting-on blocker")).toBeNull();
  // No done button on the (empty) action list.
  expect(screen.queryByRole("button", { name: /Mark done/ })).toBeNull();
});
