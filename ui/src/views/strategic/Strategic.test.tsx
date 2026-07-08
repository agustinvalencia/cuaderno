// Strategic / Monthly view (M9, #57): questions group by domain; the
// allocator draws filled + dashed-empty slots from the configured cap;
// park fires the command; an over-cap activate opens the gentle modal
// listing actives; portfolio rows read as neutral tiers; the sparkline
// renders for a tracked stewardship; the six-week timeline is read-only.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { StrategicBundle } from "../../api/bindings/StrategicBundle";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
import Strategic from "./Strategic";

const BUNDLE: StrategicBundle = {
  today: "2026-07-08",
  questions: [
    {
      slug: "surrogate-fidelity",
      domain: "research",
      status: "active",
      question_text: "How faithful is the surrogate?",
      updated: "2026-06-15",
    },
    {
      slug: "balance",
      domain: "life",
      status: "active",
      question_text: "What does a sustainable week look like?",
      updated: "2026-06-10",
    },
  ],
  portfolios: [
    {
      slug: "surrogate",
      question: "How does the surrogate behave?",
      evidence_count: 3,
      last_updated: "2026-07-01",
      staleness_days: 7n,
    },
  ],
  active: [
    { slug: "alpha", context: "work" },
    { slug: "gamma", context: "university" },
  ],
  parked: [{ slug: "beta", context: "personal" }],
  max_active: 5,
  stewardships: [
    {
      summary: {
        slug: "health",
        name: "Health",
        context: "personal",
        variant: "expanded",
        tracking_count: 12,
        last_tracking_date: "2026-07-07",
        staleness_days: 1n,
      },
      sparkline: [0, 0, 1, 2, 1, 0, 3, 2, 1, 0, 1, 2],
    },
    {
      summary: {
        slug: "finances",
        name: "Finances",
        context: "household",
        variant: "flat",
        tracking_count: 0,
        last_tracking_date: null,
        staleness_days: null,
      },
      sparkline: [],
    },
  ],
  commitments: [
    {
      date: "2026-07-20",
      title: "Submit the grant report",
      source: { kind: "standalone_commitment", slug: "grant-report" },
      is_overdue: false,
      context: "work",
    },
  ],
};

type Handler = (cmd: string, args: unknown) => unknown;

function renderStrategic(bundle: StrategicBundle, handler?: Handler) {
  mockIPC((cmd, args) => {
    const overridden = handler?.(cmd, args);
    if (overridden !== undefined) return overridden;
    if (cmd === "get_strategic_bundle") return bundle;
    // park / activate default to success (undefined = resolved void).
    return undefined;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <ReaderProvider>
          <MemoryRouter initialEntries={["/strategic"]}>
            <Strategic />
          </MemoryRouter>
        </ReaderProvider>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("questions are grouped by domain", async () => {
  renderStrategic(BUNDLE);
  // Both domain headings render, each over its question card.
  expect(await screen.findByText("research")).toBeDefined();
  expect(screen.getByText("life")).toBeDefined();
  expect(screen.getByText("How faithful is the surrogate?")).toBeDefined();
  expect(screen.getByText("What does a sustainable week look like?")).toBeDefined();
});

test("the allocator draws filled slots and dashed open slots from the cap", async () => {
  renderStrategic(BUNDLE);
  // Two active projects fill two slots; the cap of five leaves three
  // soft "open slot" placeholders — breathing room, not vacancy.
  await screen.findByText("alpha");
  expect(screen.getByText("gamma")).toBeDefined();
  expect(screen.getAllByText("open slot")).toHaveLength(3);
});

test("park fires park_project for the slot", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderStrategic(BUNDLE, (cmd, args) => {
    calls.push({ cmd, args });
    return undefined;
  });
  await screen.findByText("alpha");
  // The park button on the first active slot.
  const parkButtons = screen.getAllByRole("button", { name: "park" });
  fireEvent.click(parkButtons[0]);
  await waitFor(() => expect(calls.some((c) => c.cmd === "park_project")).toBe(true));
  const parked = calls.find((c) => c.cmd === "park_project");
  expect(parked?.args).toMatchObject({ slug: "alpha" });
});

test("an over-cap activate opens the gentle modal listing the active projects", async () => {
  renderStrategic(BUNDLE, (cmd) => {
    if (cmd === "activate_project") {
      // The structured CmdError the allocator modal keys on.
      throw {
        kind: "project_cap_reached",
        data: { current: 5, max: 5, active: ["alpha", "gamma"] },
      };
    }
    return undefined;
  });
  fireEvent.click(await screen.findByRole("button", { name: "activate" }));

  // The gentle copy — no red, no scolding — with the actives listed for
  // inline parking.
  expect(await screen.findByText("Room for five. Park one to make space.")).toBeDefined();
  const dialog = screen.getByRole("dialog");
  // Both active projects appear in the modal with their own park buttons.
  const parkButtons = within(dialog).getAllByRole("button", { name: "park" });
  expect(parkButtons).toHaveLength(2);
});

test("portfolio rows show neutral staleness tiers, never a hue", async () => {
  renderStrategic(BUNDLE);
  expect(await screen.findByText("How does the surrogate behave?")).toBeDefined();
  const cell = screen.getByText("7d ago");
  // A fresh dossier (7 days) sits at full ink — a neutral tier, not a
  // semantic colour.
  expect(cell.className).toContain("text-ink");
});

test("a tracked stewardship renders a sparkline; a flat one does not", async () => {
  renderStrategic(BUNDLE);
  await screen.findByText("Health");
  // The expanded, tracked stewardship draws its 12-week spark.
  expect(screen.getByRole("img", { name: /Health: 12-week/ })).toBeDefined();
  // The flat stewardship's empty series renders nothing at all.
  expect(screen.queryByRole("img", { name: /Finances/ })).toBeNull();
});

test("the six-week timeline is read-only", async () => {
  renderStrategic(BUNDLE);
  expect(await screen.findByText("Submit the grant report")).toBeDefined();
  // Read-only: no completion control on any commitment row.
  expect(screen.queryByRole("button", { name: /Mark done/ })).toBeNull();
});
