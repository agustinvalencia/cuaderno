// The shell's navigation (#444). The sidebar is the first and most
// persistent thing the app says about itself, so what it claims about the
// method is worth asserting: two tracks and a cadence, the project cap
// visible rather than discovered by hitting it, and settings out of the
// list of notes.
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import type { OrientationView } from "../api/bindings/OrientationView";
import type { ProjectSummary } from "../api/bindings/ProjectSummary";
import App from "../App";
import AppShell from "./AppShell";
import { ToastProvider } from "./Toasts";

/** The shell's banner query runs on every render; answer it so the
 * console is not full of react-query's undefined-data warning. */
function EXCLUSIONS_OR_NOTHING(cmd: string) {
  if (cmd === "get_index_exclusions") {
    return {
      ignored: 0,
      artefacts: 0,
      indexed: 10,
      ignore_looks_over_broad: false,
      config_generation: 1,
    };
  }
  return undefined;
}

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

function project(slug: string): ProjectSummary {
  return {
    slug,
    status: "active",
    state_snippet: "",
    top_action: null,
    context: "work",
  };
}

const ORIENTATION: OrientationView = {
  today: "2026-07-23",
  commitments: [],
  projects: [
    { ...project("alpha"), actions: [] },
    { ...project("beta"), actions: [] },
  ],
  lapsed_habits: [],
  max_active: 5,
};

function renderShell(orientation: OrientationView = ORIENTATION, at = "/") {
  mockIPC((cmd) => {
    if (cmd === "get_orientation") return orientation;
    if (cmd === "list_inbox") return [];
    return EXCLUSIONS_OR_NOTHING(cmd);
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={[at]}>
          <AppShell />
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("the sidebar is three tracks, not one flat pile", async () => {
  renderShell();
  // Rhythm is the cadence: the daily log, the log through time, and the
  // two reviews.
  const rhythm = within(await screen.findByRole("navigation", { name: "Rhythm" }));
  for (const label of ["Today", "Calendar", "Weekly", "Monthly"]) {
    expect(rhythm.getByRole("link", { name: label })).toBeDefined();
  }

  const operations = within(screen.getByRole("navigation", { name: "Operations" }));
  for (const label of ["Actions", "Commitments", "Stewardships"]) {
    expect(operations.getByRole("link", { name: label })).toBeDefined();
  }

  const inquiry = within(screen.getByRole("navigation", { name: "Inquiry" }));
  for (const label of ["Questions", "Portfolios"]) {
    expect(inquiry.getByRole("link", { name: label })).toBeDefined();
  }
});

test("the project list says how many slots are taken", async () => {
  // The 5-project cap is a rule of the method, not a progress metric — it
  // is why a sixth project has to displace one — so it is stated up front
  // rather than met as a refusal later.
  renderShell();
  expect(await screen.findByText("2 of 5 slots")).toBeDefined();
});

test("the slot count is the vault's configured cap, not a hardcoded five", async () => {
  renderShell({ ...ORIENTATION, max_active: 3 });
  expect(await screen.findByText("2 of 3 slots")).toBeDefined();
});

test("settings surfaces are no longer filed beside notes", async () => {
  // Templates and Config edit files under .cuaderno/. Listing them next to
  // Portfolios invited the reading that a template is a note; they live
  // behind Cmd+, now.
  renderShell();
  await screen.findByRole("navigation", { name: "Inquiry" });
  expect(screen.queryByRole("link", { name: "Templates" })).toBeNull();
  expect(screen.queryByRole("link", { name: "Config" })).toBeNull();
  expect(screen.queryByRole("navigation", { name: "Browse" })).toBeNull();
  // The way in is the settings button, and it advertises the shortcut.
  expect(screen.getByRole("button", { name: "Open settings" })).toBeDefined();
});

test("Strategic is Monthly in the sidebar", async () => {
  // "Strategic" named a dashboard; the review belongs to the cadence it
  // runs on.
  renderShell();
  await screen.findByRole("navigation", { name: "Rhythm" });
  expect(screen.queryByRole("link", { name: "Strategic" })).toBeNull();
  expect(screen.getByRole("link", { name: "Monthly" }).getAttribute("href")).toBe("/monthly");
});

test("the old /strategic path still lands, on Monthly", async () => {
  // Anything written before the rename — a bookmark, a `cuaderno://` link,
  // a path in a note — still has to arrive somewhere.
  function Probe() {
    return <span data-testid="path">{useLocation().pathname}</span>;
  }
  mockIPC((cmd) => {
    if (cmd === "get_orientation") return ORIENTATION;
    if (cmd === "list_inbox") return [];
    return EXCLUSIONS_OR_NOTHING(cmd);
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={["/strategic"]}>
          <Probe />
          <App />
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
  await waitFor(() => expect(screen.getByTestId("path").textContent).toBe("/monthly"));
});

test("the restructured sidebar is axe-clean", async () => {
  const { container } = renderShell();
  await screen.findByRole("navigation", { name: "Rhythm" });
  expect(await axe(container, { rules: { "color-contrast": { enabled: false } } })).toHaveNoViolations();
});
