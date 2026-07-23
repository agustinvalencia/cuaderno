// Stewardship list: renders each summary with its variant chip and a
// muted staleness line, and links to the detail route.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { StewardshipSummary } from "../../api/bindings/StewardshipSummary";
import Stewardships from "./Stewardships";

/** A stewardship that has gone quiet — past the shared ladder's
 * long-dormant tier. */
const QUIET: StewardshipSummary = {
  slug: "admin",
  name: "Admin",
  context: "legal",
  variant: "expanded",
  tracking_count: 4,
  last_tracking_date: "2026-01-02",
  staleness_days: 200,
};

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


test("the quietest sits first, not whatever the alphabet gave", async () => {
  // Staleness is the one number that matters here, and it used to sit
  // wherever the slug ordering put it.
  renderList([...ROWS, QUIET]);
  await screen.findByText("Health");

  const names = screen.getAllByRole("link", { name: /^(Health|Finances|Admin)$/ }).map((l) => l.textContent);
  expect(names[0]).toBe("Admin");

  fireEvent.change(screen.getByLabelText("Sort"), { target: { value: "name" } });
  const byName = screen.getAllByRole("link", { name: /^(Health|Finances|Admin)$/ }).map((l) => l.textContent);
  expect(byName).toEqual(["Admin", "Finances", "Health"]);
});

test("the count says how many have gone quiet, and you can see only those", async () => {
  renderList([...ROWS, QUIET]);
  await screen.findByText("Health");
  expect(screen.getByText(/3 stewardships · 1 gone quiet/)).toBeDefined();

  fireEvent.click(screen.getByRole("button", { name: "Only the quiet ones" }));

  expect(screen.getByText("Admin")).toBeDefined();
  expect(screen.queryByText("Health")).toBeNull();
});

test("a flat stewardship is never counted as lapsed", async () => {
  // It has no tracking by design — a dashboard, not a neglected habit.
  renderList(ROWS);
  await screen.findByText("Health");
  expect(screen.queryByText(/gone quiet/)).toBeNull();
  expect(screen.queryByRole("button", { name: "Only the quiet ones" })).toBeNull();
});

test("freshness reads in the shared ink ladder, not flat faint", async () => {
  // This view hand-rolled its own status vocabulary and painted every row
  // the same tone — in the one place freshness is the whole signal.
  renderList([...ROWS, QUIET]);
  await screen.findByText("Health");

  const fresh = screen.getByText(/12 tracked/);
  const quiet = screen.getByText(/4 tracked/);
  expect(fresh.className).toContain("text-ink");
  expect(fresh.className).not.toContain("text-ink-faint");
  expect(quiet.className).toContain("text-ink-faint");
});

test("an expanded stewardship can be logged from the list", async () => {
  // Logging used to mean navigating in and scrolling past a page of
  // charts to reach the form.
  renderList(ROWS);
  await screen.findByText("Health");

  const log = screen.getByRole("link", { name: "Log an entry for Health" });
  expect(log.getAttribute("href")).toBe("/stewardships/health?log=1");
  // A flat one has nothing to log into.
  expect(screen.queryByRole("link", { name: /Log an entry for Finances/ })).toBeNull();
});

test("the name outlives the status line when space runs out", async () => {
  // It used to be the other way round: the name truncated while
  // "12 tracked · last tracked 3d ago" kept its full width.
  renderList(ROWS);
  const name = await screen.findByRole("link", { name: "Health" });
  const status = screen.getByText(/12 tracked/);
  expect(name.className).toContain("flex-[3]");
  expect(status.className).toContain("flex-1");
});
