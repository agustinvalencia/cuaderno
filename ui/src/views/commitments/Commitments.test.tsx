// The Commitments Timeline view against a fixture shaped like the
// ts-rs bindings: month grouping, the collapsed slipped-past group,
// past-due wording without hoisting, the context filter, and the
// completion buttons (present only where the source is completable).
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ReaderProvider } from "../../shell/reader";
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
          <ReaderProvider>
            <Commitments />
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

test("bands the near term, keeps months beyond it, and collapses slipped-past", async () => {
  mockIPC((cmd) => (cmd === "get_commitments" ? FIXTURE : undefined));
  renderView();

  await screen.findByText("Ship v1");
  // today is Wednesday 15 Jul, so the week runs out on the 19th: the 20th
  // and 25th are next week, and August is far enough to stay a month.
  // Inside one month bucket, something due tomorrow and something due in
  // 28 days used to sit together with no divider.
  expect(screen.getByRole("heading", { name: "Next week" })).toBeDefined();
  expect(screen.getByRole("heading", { name: "August" })).toBeDefined();
  expect(screen.queryByRole("heading", { name: "July" })).toBeNull();

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
  screen.getByRole("button", { name: "personal, 1" }).click();

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

test("an ambiguous milestone completion opens the picker and re-invokes with the exact name", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  let milestoneCalls = 0;
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_commitments") return FIXTURE;
    if (cmd === "complete_milestone") {
      milestoneCalls += 1;
      // The row's title "Ship v1" is a substring of two milestones; the
      // first attempt comes back ambiguous, the picked one succeeds.
      if (milestoneCalls === 1) {
        throw {
          kind: "ambiguous",
          data: { query: "Ship v1", candidates: ["Ship v1 (backend)", "Ship v1 (desktop)"] },
        };
      }
      return undefined;
    }
    return undefined;
  });
  renderView();

  fireEvent.click(await screen.findByRole("button", { name: "Mark done: Ship v1" }));

  // The picker opens with the candidates — not a dead-end toast.
  expect(await screen.findByRole("dialog")).toBeDefined();
  fireEvent.click(screen.getByRole("button", { name: "Ship v1 (desktop)" }));

  // The milestone completion re-fires against the project slug with the
  // exact chosen name (the bespoke override plumbing on this site).
  await waitFor(() => {
    const completes = calls.filter((c) => c.cmd === "complete_milestone");
    expect(completes).toHaveLength(2);
    expect(completes[1].args).toMatchObject({ project: "alpha", milestone: "Ship v1 (desktop)" });
  });
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

test("the horizon is stated, and changing it re-queries", async () => {
  // 90 days used to be hardcoded into both the call and the query key, so
  // it silently defined what "all your commitments" meant — a promise 100
  // days out was invisible with no hint that it existed.
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return cmd === "get_commitments" ? FIXTURE : undefined;
  });
  renderView();
  await screen.findByText("Ship v1");
  expect(calls.find((c) => c.cmd === "get_commitments")?.args).toMatchObject({ lookaheadDays: 90 });

  const horizon = screen.getByLabelText("Looking ahead") as HTMLSelectElement;
  expect(horizon.selectedOptions[0].textContent).toBe("3 months");
  fireEvent.change(horizon, { target: { value: "0" } });

  await waitFor(() =>
    expect(
      calls
        .filter((c) => c.cmd === "get_commitments")
        .map((c) => (c.args as { lookaheadDays: number }).lookaheadDays),
    ).toContain(14),
  );
  expect(horizon.selectedOptions[0].textContent).toBe("2 weeks");
});

test("chips carry counts and clear in one move", async () => {
  mockIPC((cmd) => (cmd === "get_commitments" ? FIXTURE : undefined));
  renderView();
  await screen.findByText("Ship v1");

  expect(screen.getByRole("button", { name: "personal, 1" })).toBeDefined();
  // A context nobody promised anything in gets no chip at all.
  expect(screen.queryByRole("button", { name: /^legal/ })).toBeNull();
  expect(screen.queryByRole("button", { name: "Clear" })).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "personal, 1" }));
  fireEvent.click(screen.getByRole("button", { name: "Clear" }));

  expect(screen.getByText("Ship v1")).toBeDefined();
  expect(screen.getByRole("button", { name: "personal, 1" }).getAttribute("aria-pressed")).toBe(
    "false",
  );
});

test("the month view places each promise on its day, and lists the day you choose", async () => {
  // The same data read spatially: is the next fortnight clear, or is there
  // a wall. A list answers that only if you read every line.
  mockIPC((cmd) => (cmd === "get_commitments" ? FIXTURE : undefined));
  renderView();
  await screen.findByText("Ship v1");

  fireEvent.click(screen.getByRole("button", { name: "Month" }));

  // 20 July carries a commitment; 21 July does not.
  const marked = await screen.findByRole("button", { name: /July 2026 20, has a commitment/ });
  expect(screen.queryByRole("button", { name: /July 2026 21, has a commitment/ })).toBeNull();

  fireEvent.click(marked);
  const day = within(screen.getByRole("region", { name: "Commitments on the chosen day" }));
  expect(day.getByText("Ship v1")).toBeDefined();
  expect(day.queryByText("Renew passport")).toBeNull();
});

test("the month view honours the context filter", async () => {
  mockIPC((cmd) => (cmd === "get_commitments" ? FIXTURE : undefined));
  renderView();
  await screen.findByText("Ship v1");
  fireEvent.click(screen.getByRole("button", { name: "Month" }));
  await screen.findByRole("button", { name: /July 2026 20, has a commitment/ });

  fireEvent.click(screen.getByRole("button", { name: "personal, 1" }));

  // Only the personal promise (25 Jul) keeps its mark; the work one on
  // the 20th loses it.
  expect(screen.getByRole("button", { name: /July 2026 25, has a commitment/ })).toBeDefined();
  expect(screen.queryByRole("button", { name: /July 2026 20, has a commitment/ })).toBeNull();
});
