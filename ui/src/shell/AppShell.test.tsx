// The shell's navigation (#444). The sidebar is the first and most
// persistent thing the app says about itself, so what it claims about the
// method is worth asserting: two tracks and a cadence, the project cap
// visible rather than discovered by hitting it, and settings out of the
// list of notes.
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

// jsdom lacks the layout APIs cmdk and Radix Dialog reach for.
if (!Element.prototype.scrollIntoView) Element.prototype.scrollIntoView = () => {};
globalThis.ResizeObserver ||= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;

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
  // The project rows arrive with the orientation query, so wait for them
  // before reading the groups.
  await screen.findByRole("link", { name: "alpha" });

  // Rhythm is the cadence: the daily log, the log through time, and the
  // two reviews.
  const rhythm = within(screen.getByRole("navigation", { name: "Rhythm" }));
  for (const label of ["Today", "Calendar", "Weekly", "Monthly"]) {
    expect(rhythm.getByRole("link", { name: label })).toBeDefined();
  }

  const operations = within(screen.getByRole("navigation", { name: "Operations" }));
  for (const label of ["Actions", "Commitments", "Stewardships"]) {
    expect(operations.getByRole("link", { name: label })).toBeDefined();
  }
  // Projects are part of Operations, not a fourth thing beside it: the
  // heading and every active project sit inside the same landmark.
  expect(operations.getByText("Projects")).toBeDefined();
  expect(operations.getByRole("link", { name: "alpha" })).toBeDefined();

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

test("the settings button opens the dialog, on Appearance", async () => {
  // The shell holds the section state and the dialog renders from it. The
  // dialog's own suite drives a local stand-in for that state, so without
  // this the one wiring the PR introduced is asserted by nothing — and a
  // no-op gear button would ship green with Templates and Vault config
  // now unreachable from the sidebar.
  renderShell();
  fireEvent.click(await screen.findByRole("button", { name: "Open settings" }));

  expect(await screen.findByRole("heading", { name: "Settings" })).toBeDefined();
  const rail = within(screen.getByRole("navigation", { name: "Settings sections" }));
  expect(rail.getByRole("button", { name: "Appearance" }).getAttribute("aria-current")).toBe(
    "true",
  );
});

test("Cmd+, opens the dialog too", async () => {
  renderShell();
  await screen.findByRole("navigation", { name: "Rhythm" });
  fireEvent.keyDown(window, { key: ",", metaKey: true });

  expect(await screen.findByRole("heading", { name: "Settings" })).toBeDefined();
});

test("the palette opens Settings at the section it names", async () => {
  // Templates and Vault config left the sidebar, so the palette is the
  // only way to reach them by name. Driven through the real shell and the
  // real dialog: a spy would pass just as well with the entries mislabelled.
  for (const [entry, expected] of [
    ["Templates…", "Templates"],
    ["Vault config…", "Vault config"],
    ["Settings…", "Appearance"],
  ] as const) {
    renderShell();
    await screen.findByRole("navigation", { name: "Rhythm" });
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    fireEvent.click(await screen.findByText(entry));

    const rail = within(await screen.findByRole("navigation", { name: "Settings sections" }));
    expect(rail.getByRole("button", { name: expected }).getAttribute("aria-current")).toBe(
      "true",
    );
    cleanup();
  }
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
