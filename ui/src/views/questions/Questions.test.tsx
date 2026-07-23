// Questions (#443): grouped by domain, phrased as questions, showing what
// links to them, and writable so an answered question stops asking itself.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import type { QuestionStrategicRow } from "../../api/bindings/QuestionStrategicRow";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
import Questions from "./Questions";

const NO_LINKS = { projects: [], portfolios: [], evidence: [], other: [] };

const ROWS: QuestionStrategicRow[] = [
  {
    summary: {
      slug: "sparse",
      domain: "research",
      status: "active",
      question_text: "Does the sparse variant hold up?",
      updated: "2026-07-01",
    },
    backlinks: {
      projects: ["projects/alpha.md"],
      portfolios: ["portfolios/sparse-evidence/_index.md"],
      evidence: [],
      other: [],
    },
  },
  {
    summary: {
      slug: "encoder",
      domain: "research",
      status: "answered",
      question_text: "Is the new encoder worth its cost?",
      updated: "2026-06-02",
    },
    backlinks: NO_LINKS,
  },
  {
    summary: {
      slug: "rest",
      domain: "life",
      status: "active",
      question_text: "What does a sustainable week look like?",
      updated: "2026-07-10",
    },
    backlinks: NO_LINKS,
  },
];

function renderView(calls: Array<{ cmd: string; args: unknown }> = [], rows = ROWS) {
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_questions") return rows;
    return undefined;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter>
          <ReaderProvider>
            <Questions />
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

test("questions read as questions, not as slugs", async () => {
  renderView();

  expect(await screen.findByText("Does the sparse variant hold up?")).toBeDefined();
  expect(screen.queryByText("sparse")).toBeNull();
});

test("they are grouped by domain", async () => {
  renderView();

  const research = within(await screen.findByLabelText("Research"));
  expect(research.getByText("Does the sparse variant hold up?")).toBeDefined();

  const life = within(screen.getByLabelText("Life"));
  expect(life.getByText("What does a sustainable week look like?")).toBeDefined();
});

test("a settled question stays reachable rather than vanishing", async () => {
  // Answering a question should not delete it from the record — the point
  // of a monthly re-read is partly to see what got settled.
  renderView();

  await screen.findByText("Does the sparse variant hold up?");
  expect(screen.queryByText("Is the new encoder worth its cost?")).toBeNull();

  fireEvent.click(
    screen.getByRole("button", { name: "Show all 1 settled research questions" }),
  );

  expect(screen.getByText("Is the new encoder worth its cost?")).toBeDefined();
});

test("what points at a question is shown, and routes", async () => {
  renderView();

  await screen.findByText("Does the sparse variant hold up?");
  expect(screen.getByRole("link", { name: "alpha" }).getAttribute("href")).toBe(
    "/projects/alpha",
  );
  expect(screen.getByRole("link", { name: "sparse evidence" }).getAttribute("href")).toBe(
    "/portfolios/sparse-evidence",
  );
});

test("a question nothing links to says so plainly", async () => {
  // Not an error state: it is the signal a monthly read-through wants.
  renderView();

  const life = within(await screen.findByLabelText("Life"));
  expect(life.getByText(/Nothing links here yet/)).toBeDefined();
});

test("a question's status can be changed", async () => {
  // Without a write this would be another read-only dashboard, and an
  // answered question would go on asking itself.
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderView(calls);

  const select = await screen.findByLabelText("Status of sparse");
  fireEvent.change(select, { target: { value: "answered" } });

  await waitFor(() => {
    expect(calls.find((c) => c.cmd === "set_question_status")?.args).toMatchObject({
      slug: "sparse",
      status: "answered",
    });
  });
});

test("an empty vault gets a calm empty state", async () => {
  renderView([], []);

  expect(await screen.findByText(/No questions yet/)).toBeDefined();
});
