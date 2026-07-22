// Stewardship list: renders each summary with its variant chip and a
// muted staleness line, and links to the detail route.
import { afterEach, expect, test } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { StewardshipSummary } from "../../api/bindings/StewardshipSummary";
import Stewardships from "./Stewardships";

const ROWS: StewardshipSummary[] = [
  {
    slug: "health",
    name: "Health",
    context: "personal",
    variant: "expanded",
    tracking_count: 12,
    last_tracking_date: "2026-07-05",
    staleness_days: 3,
  },
  {
    slug: "finances",
    name: "Finances",
    context: "household",
    variant: "flat",
    tracking_count: 0,
    last_tracking_date: null,
    staleness_days: null,
  },
];

function renderList(rows: StewardshipSummary[]) {
  mockIPC((cmd) => {
    if (cmd === "list_stewardships") return rows;
    return undefined;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={["/stewardships"]}>
        <Stewardships />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("renders each stewardship with its variant chip and staleness line", async () => {
  renderList(ROWS);
  expect(await screen.findByText("Health")).toBeDefined();
  expect(screen.getByText("expanded")).toBeDefined();
  expect(screen.getByText("12 tracked · last tracked 3d ago")).toBeDefined();

  // The flat stewardship reads as "dashboard only", never an alarm.
  expect(screen.getByText("Finances")).toBeDefined();
  expect(screen.getByText("flat")).toBeDefined();
  expect(screen.getByText("dashboard only")).toBeDefined();
});

test("rows link to the detail route", async () => {
  renderList(ROWS);
  const link = (await screen.findByText("Health")).closest("a");
  expect(link?.getAttribute("href")).toBe("/stewardships/health");
});
