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
import MonthlyReview from "./MonthlyReview";

const BUNDLE: StrategicBundle = {
  today: "2026-07-08",
  questions: [
    {
      summary: {
        slug: "surrogate-fidelity",
        domain: "research",
        status: "active",
        question_text: "How faithful is the surrogate?",
        updated: "2026-06-15",
      },
      // A project references this question in its body → a routed chip.
      // Use a slug that doesn't collide with the allocator's alpha/beta
      // slots, so `getByText` in unrelated tests stays unambiguous. A
      // portfolio body-links it with a DIFFERENT slug
      // (link_portfolio_to_question) → a routed portfolio chip via the
      // parent-dir slug. A daily note in `other` must NOT be chipped (noise).
      backlinks: {
        projects: ["projects/delta.md"],
        portfolios: ["portfolios/other-dossier/_index.md"],
        evidence: [],
        other: ["journal/2026/daily/2026-07-01.md"],
      },
    },
    {
      summary: {
        slug: "balance",
        domain: "life",
        status: "active",
        question_text: "What does a sustainable week look like?",
        updated: "2026-06-10",
      },
      backlinks: { projects: [], portfolios: [], evidence: [], other: [] },
    },
  ],
  portfolios: [
    {
      slug: "surrogate",
      question: "How does the surrogate behave?",
      evidence_count: 3,
      last_updated: "2026-07-01",
      staleness_days: 7,
    },
    {
      // Shares the "surrogate-fidelity" research question's slug, so it
      // correlates to that question and surfaces a chip on its card.
      slug: "surrogate-fidelity",
      question: "Fidelity evidence dossier",
      evidence_count: 2,
      last_updated: "2026-07-05",
      staleness_days: 3,
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
        staleness_days: 1,
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

function renderMonthly(bundle: StrategicBundle, handler?: Handler) {
  mockIPC((cmd, args) => {
    const overridden = handler?.(cmd, args);
    if (overridden !== undefined) return overridden;
    if (cmd === "get_strategic_bundle") return bundle;
    // park / activate / save_monthly_section default to success.
    return undefined;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={["/monthly"]}>
          <ReaderProvider>
            <MonthlyReview />
          </ReaderProvider>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

/** The review is stepped now, so every panel but Questions starts
 * hidden. Click its rail entry to bring one into view. */
async function goToStep(label: string) {
  const rail = within(await screen.findByRole("navigation", { name: "Review steps" }));
  fireEvent.click(rail.getByRole("button", { name: label }));
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("questions are grouped by domain", async () => {
  renderMonthly(BUNDLE);
  // Both domain headings render, each over its question card.
  expect(await screen.findByText("research")).toBeDefined();
  expect(screen.getByText("life")).toBeDefined();
  expect(screen.getByText("How faithful is the surrogate?")).toBeDefined();
  expect(screen.getByText("What does a sustainable week look like?")).toBeDefined();
});

test("a question with a matching portfolio shows a chip that navigates to it", async () => {
  renderMonthly(BUNDLE);
  // The "surrogate-fidelity" question shares its slug with a portfolio,
  // so its card carries a chip linking to that portfolio's detail route.
  const chip = await screen.findByRole("link", { name: "surrogate-fidelity" });
  expect(chip.getAttribute("href")).toBe("/portfolios/surrogate-fidelity");
});

test("a question without a matching portfolio shows no chip", async () => {
  renderMonthly(BUNDLE);
  // The "balance" life question has no same-slug portfolio, so no chip
  // links out from it.
  await screen.findByText("What does a sustainable week look like?");
  expect(screen.queryByRole("link", { name: "balance" })).toBeNull();
});

test("a question backlinked by a project shows a chip routing to that project (#354)", async () => {
  renderMonthly(BUNDLE);
  // The "surrogate-fidelity" question is referenced by projects/delta.md,
  // so its card carries a project chip linking to that project's route.
  const chip = await screen.findByRole("link", { name: "delta" });
  expect(chip.getAttribute("href")).toBe("/projects/delta");
});

test("a portfolio backlinked by a differing slug surfaces a routed chip (#354)", async () => {
  renderMonthly(BUNDLE);
  // A portfolio that body-links the question with a slug OTHER than the
  // question's (link_portfolio_to_question) still shows a chip, routed via
  // its parent-dir slug — not only the slug-correlated portfolios.
  const chip = await screen.findByRole("link", { name: "other-dossier" });
  expect(chip.getAttribute("href")).toBe("/portfolios/other-dossier");
});

test("an `other` backlink (e.g. a daily note) is not chipped on the grid (#354)", async () => {
  renderMonthly(BUNDLE);
  await screen.findByText("How faithful is the surrogate?");
  // The daily-note backlink in the `other` bucket is deliberately not
  // rendered — it's noise on the calm strategic grid.
  expect(screen.queryByRole("button", { name: "2026-07-01" })).toBeNull();
});

test("the allocator draws filled slots and dashed open slots from the cap", async () => {
  renderMonthly(BUNDLE);
  await goToStep("Projects");
  // Two active projects fill two slots; the cap of five leaves three
  // soft "open slot" placeholders — breathing room, not vacancy.
  await screen.findByText("alpha");
  expect(screen.getByText("gamma")).toBeDefined();
  expect(screen.getAllByText("open slot")).toHaveLength(3);
});

test("park fires park_project for the slot", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderMonthly(BUNDLE, (cmd, args) => {
    calls.push({ cmd, args });
    return undefined;
  });
  await goToStep("Projects");
  // The park button on the first active slot.
  const parkButtons = await screen.findAllByRole("button", { name: "park" });
  fireEvent.click(parkButtons[0]);
  await waitFor(() => expect(calls.some((c) => c.cmd === "park_project")).toBe(true));
  const parked = calls.find((c) => c.cmd === "park_project");
  expect(parked?.args).toMatchObject({ slug: "alpha" });
});

test("an over-cap activate opens the gentle modal listing the active projects", async () => {
  renderMonthly(BUNDLE, (cmd) => {
    if (cmd === "activate_project") {
      // The structured CmdError the allocator modal keys on.
      throw {
        kind: "project_cap_reached",
        data: { current: 5, max: 5, active: ["alpha", "gamma"] },
      };
    }
    return undefined;
  });
  await goToStep("Projects");
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
  renderMonthly(BUNDLE);
  await goToStep("Portfolios");
  expect(await screen.findByText("How does the surrogate behave?")).toBeDefined();
  const cell = screen.getByText("7d ago");
  // A fresh dossier (7 days) sits at full ink — a neutral tier, not a
  // semantic colour.
  expect(cell.className).toContain("text-ink");
});

test("a tracked stewardship renders a sparkline; a flat one does not", async () => {
  renderMonthly(BUNDLE);
  await goToStep("Stewardships");
  await screen.findByText("Health");
  // The expanded, tracked stewardship draws its 12-week spark.
  expect(screen.getByRole("img", { name: /Health: 12-week/ })).toBeDefined();
  // The flat stewardship's empty series renders nothing at all.
  expect(screen.queryByRole("img", { name: /Finances/ })).toBeNull();
});

test("the six-week timeline is read-only", async () => {
  renderMonthly(BUNDLE);
  await goToStep("Lookahead");
  expect(await screen.findByText("Submit the grant report")).toBeDefined();
  // Read-only: no completion control on any commitment row.
  expect(screen.queryByRole("button", { name: /Mark done/ })).toBeNull();
});

test("a portfolio-health row links to its portfolio (#440)", async () => {
  renderMonthly(BUNDLE);
  await goToStep("Portfolios");
  // The same portfolio already routed as a chip on a question card; in its
  // own health table it rendered as dead text. Both are links now.
  const row = await screen.findByRole("link", { name: "How does the surrogate behave?" });
  expect(row.getAttribute("href")).toBe("/portfolios/surrogate");
});

test("the review is stepped, and opens on Questions", async () => {
  renderMonthly(BUNDLE);
  const rail = within(await screen.findByRole("navigation", { name: "Review steps" }));
  for (const label of ["Questions", "Portfolios", "Projects", "Stewardships", "Lookahead", "Focus"]) {
    expect(rail.getByRole("button", { name: label }).textContent).toContain(label);
  }
  expect(rail.getByRole("button", { name: "Questions" }).getAttribute("aria-current")).toBe("step");
});

test("a stewardship row links to its detail", async () => {
  renderMonthly(BUNDLE);
  await goToStep("Stewardships");
  const link = await screen.findByRole("link", { name: /health/i });
  expect(link.getAttribute("href")).toBe("/stewardships/health");
});

test("the focus step writes only the sections you filled, to the right month", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderMonthly(BUNDLE, (cmd, args) => {
    calls.push({ cmd, args });
    return undefined;
  });
  await goToStep("Focus");

  fireEvent.change(screen.getByLabelText("Wins"), { target: { value: "shipped the epic" } });
  fireEvent.change(screen.getByLabelText("Next month's focus"), {
    target: { value: "the migration" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save to the note" }));

  await screen.findByText(/Saved to/);
  const writes = calls.filter((c) => c.cmd === "save_monthly_section");
  expect(writes).toHaveLength(2);
  expect(writes.map((c) => (c.args as { section: string }).section).sort()).toEqual([
    "next-months-focus",
    "wins",
  ]);
  expect((writes[0].args as { month: string }).month).toBe("2026-07");
});

test("a blank section is not written, even when another is filled", async () => {
  // Each section is its own compose/overwrite; leaving Wins empty must
  // not clobber a Wins section the note may already hold.
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderMonthly(BUNDLE, (cmd, args) => {
    calls.push({ cmd, args });
    return undefined;
  });
  await goToStep("Focus");

  // Only Themes; Wins and Focus left blank.
  fireEvent.change(screen.getByLabelText("Themes"), { target: { value: "steady progress" } });
  fireEvent.click(screen.getByRole("button", { name: "Save to the note" }));

  await screen.findByText(/Saved to/);
  const writes = calls.filter((c) => c.cmd === "save_monthly_section");
  expect(writes).toHaveLength(1);
  expect((writes[0].args as { section: string }).section).toBe("themes");
});

test("the focus step will not save an empty review", async () => {
  renderMonthly(BUNDLE);
  await goToStep("Focus");
  expect(
    (screen.getByRole("button", { name: "Save to the note" }) as HTMLButtonElement).disabled,
  ).toBe(true);
});

test("saving the focus step earns the 'already saved' stop line", async () => {
  renderMonthly(BUNDLE);
  await goToStep("Focus");
  expect(screen.queryByText(/already saved/)).toBeNull();

  fireEvent.change(screen.getByLabelText("Wins"), { target: { value: "a good month" } });
  fireEvent.click(screen.getByRole("button", { name: "Save to the note" }));

  expect(await screen.findByText(/already saved/)).toBeDefined();
});

test("Back and Next walk the steps", async () => {
  renderMonthly(BUNDLE);
  await screen.findByRole("navigation", { name: "Review steps" });
  expect((screen.getByRole("button", { name: "Back" }) as HTMLButtonElement).disabled).toBe(true);

  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  const rail = within(screen.getByRole("navigation", { name: "Review steps" }));
  expect(rail.getByRole("button", { name: "Portfolios" }).getAttribute("aria-current")).toBe("step");
});

test("a partial save says what landed and what did not", async () => {
  // The sections write in sequence, so a mid-batch failure must name the
  // section that failed AND the ones already on disk — a bare error would
  // leave the reader thinking nothing saved when Wins already did.
  renderMonthly(BUNDLE, (cmd, args) => {
    if (cmd === "save_monthly_section") {
      const section = (args as { section: string }).section;
      if (section === "themes") throw new Error("vault is read-only");
      return undefined;
    }
    return undefined;
  });
  await goToStep("Focus");

  fireEvent.change(screen.getByLabelText("Wins"), { target: { value: "shipped it" } });
  fireEvent.change(screen.getByLabelText("Themes"), { target: { value: "steady" } });
  fireEvent.click(screen.getByRole("button", { name: "Save to the note" }));

  const toast = await screen.findByText(/Saved wins, but themes failed/);
  expect(toast.textContent).toContain("read-only");
  // A failed batch is not "already saved".
  expect(screen.queryByText(/already saved/)).toBeNull();
});

test("a focus draft survives a jump to another step and back", async () => {
  // Every step stays mounted, so an unsaved draft must not be discarded
  // by stepping away to check a panel and returning.
  renderMonthly(BUNDLE);
  await goToStep("Focus");
  fireEvent.change(screen.getByLabelText("Wins"), { target: { value: "half-written" } });

  await goToStep("Portfolios");
  await goToStep("Focus");

  expect((screen.getByLabelText("Wins") as HTMLTextAreaElement).value).toBe("half-written");
});

test("a visited read step earns its tick, and the softer stop line", async () => {
  // Reaching a step counts as looking at it. Before, `completed` was
  // populated only by the write step, so no read step ever ticked and
  // the softer reassurance was unreachable dead code.
  renderMonthly(BUNDLE);
  const rail = within(await screen.findByRole("navigation", { name: "Review steps" }));

  await goToStep("Portfolios");
  await goToStep("Lookahead");

  // Portfolios is now a visited, non-current step — the stepper ticks it
  // (its button shows the checkmark, not its number).
  expect(rail.getByRole("button", { name: "Portfolios" }).textContent).toContain("✓");
  // And the softer line shows, never the write one.
  expect(screen.getByText(/nothing here demands finishing/)).toBeDefined();
  expect(screen.queryByText(/already saved/)).toBeNull();
});
