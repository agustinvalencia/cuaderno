// Portfolio selector: renders each summary with its evidence count and
// a neutral-tier staleness line (with a "last updated" title), and links
// to the detail route.
import { afterEach, expect, test } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { PortfolioSummary } from "../../api/bindings/PortfolioSummary";
import Portfolios from "./Portfolios";

const ROWS: PortfolioSummary[] = [
  {
    slug: "surrogate",
    question: "How does the surrogate behave?",
    evidence_count: 4,
    last_updated: "2026-07-05",
    staleness_days: 3,
  },
  {
    slug: "empty",
    question: "An untouched question?",
    evidence_count: 0,
    last_updated: null,
    staleness_days: null,
  },
];

function renderList(rows: PortfolioSummary[]) {
  mockIPC((cmd) => {
    if (cmd === "list_portfolios") return rows;
    return undefined;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={["/portfolios"]}>
        <Portfolios />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("renders each portfolio with its evidence count and staleness line", async () => {
  renderList(ROWS);
  expect(await screen.findByText("How does the surrogate behave?")).toBeDefined();
  // Count + freshness, with the age spelled out in the hover title.
  // Visible line and hover title share the same "3d ago" spacing.
  const line = screen.getByText("4 notes · last filed 3d ago");
  expect(line.getAttribute("title")).toBe("last updated 3d ago");

  // A portfolio with no evidence reads as calm "no evidence yet", never
  // an alarm.
  expect(screen.getByText("An untouched question?")).toBeDefined();
  const empty = screen.getByText("no evidence yet");
  expect(empty.getAttribute("title")).toBe("no evidence filed yet");
});

test("rows link to the detail route", async () => {
  renderList(ROWS);
  const link = (await screen.findByText("How does the surrogate behave?")).closest("a");
  expect(link?.getAttribute("href")).toBe("/portfolios/surrogate");
});
