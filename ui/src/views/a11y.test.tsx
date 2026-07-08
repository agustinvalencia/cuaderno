// Axe smoke pass over the main views (M10; the M2/M5 keyboard/a11y
// criteria): render each populated view against a bindings-shaped
// fixture, run axe, assert no violations.
//
// Rule exclusions, and why:
// - "color-contrast": needs real paint (canvas + resolved CSS custom
//   properties); jsdom has neither, so the check can only misfire.
//   Contrast is owned by the theme tokens in styles/globals.css.
// Views render inside a <main> landmark because that is where the
// AppShell mounts them — without it axe's "region" rule would flag an
// artefact of the test harness, not the app.
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ReaderProvider } from "../shell/reader";
import { ToastProvider } from "../shell/Toasts";
import type { OrientationView } from "../api/bindings/OrientationView";
import type { CommitmentsView } from "../api/bindings/CommitmentsView";
import type { StrategicBundle } from "../api/bindings/StrategicBundle";
import Home from "./home/Home";
import Commitments from "./commitments/Commitments";
import Strategic from "./strategic/Strategic";
import Calendar from "./calendar/Calendar";

expect.extend(matchers);
// vitest-axe 0.1.0 ships type augmentation for the pre-1.0 `Vi`
// namespace only; teach the current vitest module the matcher here.
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

const AXE_OPTIONS = {
  rules: { "color-contrast": { enabled: false } },
};

const ORIENTATION: OrientationView = {
  today: "2026-07-08",
  commitments: [
    {
      date: "2026-07-09",
      title: "submit-report",
      source: { kind: "project_milestone", slug: "alpha" },
      is_overdue: false,
      context: "work",
    },
  ],
  projects: [
    {
      slug: "alpha",
      status: "active",
      state_snippet: "Core loop underway.",
      top_action: { text: "Draft methods", energy: "deep" },
      context: "work",
      actions: [{ text: "Draft methods (deep)", energy: "deep", attached: null }],
    },
  ],
  lapsed_habits: [{ stewardship: "health", detail: "Swimming 1x/week — lapsed since March" }],
};

const COMMITMENTS: CommitmentsView = {
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
      date: "2026-08-05",
      title: "Water bill",
      source: { kind: "stewardship", slug: "finances" },
      is_overdue: false,
      context: "household",
    },
  ],
};

const STRATEGIC: StrategicBundle = {
  today: "2026-07-08",
  questions: [
    {
      slug: "surrogate-fidelity",
      domain: "research",
      status: "active",
      question_text: "How faithful is the surrogate?",
      updated: "2026-06-15",
    },
  ],
  portfolios: [
    {
      // Shares the question's slug, so a portfolio chip renders on the
      // question card — this is what proves the chip carries no
      // nested-interactive (button-in-button) axe violation.
      slug: "surrogate-fidelity",
      question: "How does the surrogate behave?",
      evidence_count: 3,
      last_updated: "2026-07-01",
      staleness_days: 7n,
    },
  ],
  active: [{ slug: "alpha", context: "work" }],
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
      sparkline: [0, 1, 2, 1, 0, 3, 2, 1, 0, 1, 2, 1],
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

function renderView(view: React.ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <ReaderProvider>
          <MemoryRouter>
            <main>{view}</main>
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

test("Home has no axe violations", async () => {
  mockIPC((cmd) => (cmd === "get_orientation" ? ORIENTATION : undefined));
  const { container } = renderView(<Home />);
  await screen.findByText("alpha");
  expect(await axe(container, AXE_OPTIONS)).toHaveNoViolations();
});

test("Commitments has no axe violations", async () => {
  mockIPC((cmd) => (cmd === "get_commitments" ? COMMITMENTS : undefined));
  const { container } = renderView(<Commitments />);
  await screen.findByText("Ship v1");
  expect(await axe(container, AXE_OPTIONS)).toHaveNoViolations();
});

test("Strategic has no axe violations", async () => {
  mockIPC((cmd) => (cmd === "get_strategic_bundle" ? STRATEGIC : undefined));
  const { container } = renderView(<Strategic />);
  await screen.findByText("alpha");
  expect(await axe(container, AXE_OPTIONS)).toHaveNoViolations();
});

test("Calendar has no axe violations", async () => {
  mockIPC((cmd, args) => {
    switch (cmd) {
      case "get_today":
        return "2026-07-15";
      case "list_daily_dates":
        return ["2026-07-15"];
      case "read_daily":
        return {
          date: (args as { date: string }).date,
          exists: true,
          markdown: "# Wednesday\n\nShipped the calendar grid.",
          path: "journal/2026/daily/2026-07-15.md",
          prev_date: "2026-07-14",
          next_date: "2026-07-16",
          week_of: "2026-07-13",
          month: "2026-07",
        };
      default:
        return undefined;
    }
  });
  const { container } = renderView(<Calendar />);
  await screen.findByRole("heading", { name: "Wednesday" });
  expect(await axe(container, AXE_OPTIONS)).toHaveNoViolations();
});
