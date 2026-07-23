// Project Detail: renders a composed fixture (actions, milestones,
// backlinks, log mentions), fires complete_action on done, and renders
// a parked project read-only (no add row, no done buttons).
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Link, MemoryRouter, Route, Routes } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { ProjectDetail as ProjectDetailData } from "../../api/bindings/ProjectDetail";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
import ProjectDetail, { recentLogMentions } from "./ProjectDetail";

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
    portfolios: [{ path: "portfolios/x/_index.md", title: null }],
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

function renderDetail(
  fixture: ProjectDetailData = ACTIVE,
  onCall?: (cmd: string, args: unknown) => void,
) {
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
  // A backlink reads as its note, not its path; an `_index.md` names its
  // folder, since "index" would describe every portfolio equally.
  expect(screen.getByText("x")).toBeDefined();
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

test("the current state is rendered, not hidden behind an edit affordance", async () => {
  // "Where am I" is the one question the map exists to answer, so the state
  // is on the page. Previously the section held only an "Edit current
  // state" link and the prose appeared solely in the verbatim map, several
  // screens down.
  renderDetail();

  // Twice, deliberately: once as the section, once inside the verbatim map
  // at the foot of the page, which still shows everything as written.
  const shown = await screen.findAllByText("Going well.");
  expect(shown.length).toBe(2);
});

test("the edit affordance sits beside the heading, not around the prose", async () => {
  // Wrapping the rendered state in a button nests its wikilinks inside a
  // control: invalid, and a click on one bubbles into opening the editor.
  renderDetail();

  fireEvent.click(await screen.findByRole("button", { name: "Edit" }));

  const editor = screen.getByLabelText("Current state of alpha") as HTMLTextAreaElement;
  expect(editor.value).toBe("Going well.");
});

test("a wikilink in the state resolves without opening the editor", async () => {
  const linked = {
    ...ACTIVE,
    body_markdown: "## Current State\nBlocked on [[projects/beta]].\n",
  };
  const calls: string[] = [];
  renderDetail(linked, (cmd) => calls.push(cmd));

  // Twice on the page: once in the state section, once in the verbatim map.
  const links = await screen.findAllByText("projects/beta");
  fireEvent.click(links[0]);

  expect(calls).toContain("resolve_wikilink");
  expect(screen.queryByLabelText("Current state of alpha")).toBeNull();
});

test("a long backlink group collapses to a few, with the true total offered", async () => {
  // Backlinks accumulate for as long as the project runs; past a handful
  // they push the state and the next actions off the screen.
  const many = Array.from({ length: 9 }, (_, i) => ({
    path: `portfolios/x/2026-07-0${i + 1}-finding-${i + 1}.md`,
    title: null,
  }));
  renderDetail({ ...ACTIVE, backlinks: { ...ACTIVE.backlinks, portfolios: many } });

  expect(await screen.findByText("finding 1")).toBeDefined();
  expect(screen.queryByText("finding 9")).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Show all 9 portfolios" }));

  expect(screen.getByText("finding 9")).toBeDefined();
});

test("log mentions collapse the same way", async () => {
  const many = Array.from({ length: 8 }, (_, i) => ({
    date: `2026-07-0${i + 1}`,
    time: "09:00:00",
    text: `entry ${i + 1}`,
  }));
  renderDetail({ ...ACTIVE, log_mentions: many });

  // Five of eight, whichever end the app-wide order puts first.
  const toggle = await screen.findByRole("button", { name: "Show all 8 log mentions" });
  const visible = many.filter((m) => screen.queryByText(m.text) !== null);
  expect(visible.length).toBe(5);

  fireEvent.click(toggle);

  for (const mention of many) {
    expect(screen.getByText(mention.text)).toBeDefined();
  }
});

test("the collapsed log summary is the recent mentions, not the first five", () => {
  // The cap and the sort answer different questions. Capping the display
  // order meant an oldest-first reader saw the five OLDEST entries under a
  // heading that says "Recently in your logs".
  const mentions = Array.from({ length: 8 }, (_, i) => ({
    date: `2026-07-0${i + 1}`,
    time: "09:00:00",
    text: `entry ${i + 1}`,
  }));

  const oldestFirst = recentLogMentions(mentions, "oldest", 3);
  expect(oldestFirst.map((m) => m.text)).toEqual(["entry 6", "entry 7", "entry 8"]);

  const newestFirst = recentLogMentions(mentions, "newest", 3);
  expect(newestFirst.map((m) => m.text)).toEqual(["entry 8", "entry 7", "entry 6"]);
});

test("an open editor does not survive a move to another project", async () => {
  // The data-loss path: react-query serves a cached project synchronously,
  // so nothing unmounts, the body is RECONCILED with new props, and an
  // uncontrolled textarea keeps the FIRST project's text — Save would then
  // write it onto the second. The sidebar lists every active project on
  // every view, so this navigation is always one click away.
  //
  // Navigation happens in place, through the router, because that is what
  // reconciles. Unmounting and rendering afresh would pass with or without
  // the fix and prove nothing.
  const beta: ProjectDetailData = {
    ...ACTIVE,
    slug: "beta",
    body_markdown: "## Current State\nQuite different.\n",
  };
  mockIPC((cmd, args) => {
    if (cmd === "get_project") {
      return (args as { project?: string; slug?: string }).slug === "beta" ||
        (args as { project?: string }).project === "beta"
        ? beta
        : ACTIVE;
    }
    return undefined;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  // Beta must already be cached — that is the whole precondition. With a
  // cold cache the `isPending` branch unmounts the body and clears its
  // state, so the bug hides; a project visited in the last few minutes is
  // served synchronously and the body is reconciled instead.
  client.setQueryData(["get_project", "beta"], beta);
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={["/projects/alpha"]}>
          <ReaderProvider>
            {/* Stands in for the sidebar's always-present project links. */}
            <Link to="/projects/beta">go to beta</Link>
            <Routes>
              <Route path="/projects/:slug" element={<ProjectDetail />} />
            </Routes>
          </ReaderProvider>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );

  fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
  const editor = screen.getByLabelText("Current state of alpha") as HTMLTextAreaElement;
  fireEvent.change(editor, { target: { value: "Waiting on the ethics board" } });

  fireEvent.click(screen.getByRole("link", { name: "go to beta" }));

  await screen.findAllByText("Quite different.");
  expect(screen.queryByLabelText("Current state of beta")).toBeNull();
  expect(screen.queryByDisplayValue("Waiting on the ethics board")).toBeNull();
});
