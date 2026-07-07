// The Commitments Timeline view against a fixture shaped like the
// ts-rs bindings: month grouping, the collapsed slipped-past group,
// past-due wording without hoisting, the context filter, and the
// completion buttons (present only where the source is completable).
import { afterEach, expect, test } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ToastProvider } from "../../shell/Toasts";
import type { CommitmentsView } from "../../api/bindings/CommitmentsView";
import Commitments from "./Commitments";

// today = 15 Jul; one slipped-past entry, two in July, two in August.
// Kept in the backend's chronological (date-sorted) order.
const FIXTURE: CommitmentsView = {
  today: "2026-07-15",
  entries: [
    {
      date: "2026-07-10",
      title: "Slipped milestone",
      source: { kind: "project_milestone", slug: "alpha" },
      is_overdue: true,
      context: "work",
    },
    {
      date: "2026-07-20",
      title: "Ship v1",
      source: { kind: "project_milestone", slug: "alpha" },
      is_overdue: false,
      context: "work",
    },
    {
      date: "2026-07-25",
      title: "Renew passport",
      source: { kind: "standalone_commitment", slug: "renew-passport" },
      is_overdue: false,
      context: "personal",
    },
    {
      date: "2026-08-05",
      title: "Water bill",
      source: { kind: "stewardship", slug: "finances" },
      is_overdue: false,
      context: "household",
    },
    {
      date: "2026-08-10",
      title: "Draft report",
      source: { kind: "action_note", slug: "beta" },
      is_overdue: false,
      context: "work",
    },
  ],
};

function renderView() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <ToastProvider>
          <Commitments />
        </ToastProvider>
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("groups upcoming by month and collapses slipped-past by default", async () => {
  mockIPC((cmd) => (cmd === "get_commitments" ? FIXTURE : undefined));
  renderView();

  await screen.findByText("Ship v1");
  // Two month headers for the upcoming entries.
  expect(screen.getByRole("heading", { name: "July" })).toBeDefined();
  expect(screen.getByRole("heading", { name: "August" })).toBeDefined();

  // The slipped-past group carries its count and is collapsed on load.
  const summary = screen.getByText("a few slipped past");
  expect(summary.textContent).toContain("1");
  const details = summary.closest("details");
  expect(details?.hasAttribute("open")).toBe(false);
});

test("past-due reads 'planned for' and is not hoisted above upcoming", async () => {
  mockIPC((cmd) => (cmd === "get_commitments" ? FIXTURE : undefined));
  renderView();

  await screen.findByText("Ship v1");
  expect(screen.getByText(/planned for/)).toBeDefined();
  // "overdue" and red never appear — the wording law.
  expect(screen.queryByText(/overdue/i)).toBeNull();

  // Order: the slipped-past row sits in its group at the top, then the
  // upcoming rows in chronological order — never re-sorted.
  // Per row the spans are [context dot, title, …]; the title is [1].
  const titles = screen
    .getAllByRole("listitem")
    .map((li) => li.querySelectorAll("span")[1]?.textContent);
  expect(titles).toEqual([
    "Slipped milestone",
    "Ship v1",
    "Renew passport",
    "Water bill",
    "Draft report",
  ]);
});

test("context chips filter the timeline client-side", async () => {
  mockIPC((cmd) => (cmd === "get_commitments" ? FIXTURE : undefined));
  renderView();

  await screen.findByText("Ship v1");
  screen.getByRole("button", { name: "personal" }).click();

  // Only the personal entry survives; the work/household ones drop.
  expect(await screen.findByText("Renew passport")).toBeDefined();
  expect(screen.queryByText("Ship v1")).toBeNull();
  expect(screen.queryByText("Water bill")).toBeNull();
});

test("done on a standalone commitment invokes complete_commitment with its slug", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_commitments") return FIXTURE;
    return undefined;
  });
  renderView();

  (await screen.findByRole("button", { name: "Mark done: Renew passport" })).click();

  expect(await screen.findByText("Done: Renew passport.")).toBeDefined();
  const done = calls.find((c) => c.cmd === "complete_commitment");
  expect(done?.args).toMatchObject({ slug: "renew-passport" });
});

test("periodic and action-note rows carry no done button", async () => {
  mockIPC((cmd) => (cmd === "get_commitments" ? FIXTURE : undefined));
  renderView();

  await screen.findByText("Water bill");
  // Stewardship (periodic) and action-note sources are not completable
  // from here; the milestone and standalone ones are.
  expect(screen.queryByRole("button", { name: "Mark done: Water bill" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Mark done: Draft report" })).toBeNull();
  expect(screen.getByRole("button", { name: "Mark done: Ship v1" })).toBeDefined();
});
