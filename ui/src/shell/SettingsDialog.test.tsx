// SettingsDialog (⌘,): renders the app preferences, persists theme and
// metrics choices, jumps to the vault config editor, and is axe-clean.
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router";
import SettingsDialog from "./SettingsDialog";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

// Give the theme/metrics stores a deterministic, isolated localStorage, plus
// the layout/media APIs Radix Dialog and the theme helper reach.
beforeAll(() => {
  const store = new Map<string, string>();
  const local: Storage = {
    getItem: (key) => store.get(key) ?? null,
    setItem: (key, value) => void store.set(key, String(value)),
    removeItem: (key) => void store.delete(key),
    clear: () => store.clear(),
    key: (index) => [...store.keys()][index] ?? null,
    get length() {
      return store.size;
    },
  };
  Object.defineProperty(window, "localStorage", {
    value: local,
    configurable: true,
  });
  if (!Element.prototype.scrollIntoView)
    Element.prototype.scrollIntoView = () => {};
  globalThis.ResizeObserver ||= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
  window.matchMedia ||= ((query: string) =>
    ({
      matches: false,
      media: query,
      addEventListener() {},
      removeEventListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia;
});

afterEach(() => {
  cleanup();
  localStorage.clear();
});

function Probe() {
  const location = useLocation();
  return <div data-testid="path">{location.pathname}</div>;
}

function renderDialog(onOpenChange = () => {}) {
  return render(
    <MemoryRouter initialEntries={["/"]}>
      <Probe />
      <SettingsDialog open onOpenChange={onOpenChange} />
    </MemoryRouter>,
  );
}

test("renders the preference controls", () => {
  renderDialog();
  expect(screen.getByRole("heading", { name: "Settings" })).toBeDefined();
  expect(screen.getByRole("button", { name: "System" })).toBeDefined();
  expect(screen.getByRole("button", { name: "Dark" })).toBeDefined();
  expect(
    screen.getByRole("switch", { name: "Show progress metrics" }),
  ).toBeDefined();
  expect(screen.getByRole("button", { name: "Edit…" })).toBeDefined();
});

test("choosing a theme marks it selected and persists", () => {
  renderDialog();
  // The theme options are a segmented toggle group (aria-pressed), not radios.
  // Default is System (no stored override).
  expect(
    screen.getByRole("button", { name: "System" }).getAttribute("aria-pressed"),
  ).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: "Dark" }));
  expect(
    screen.getByRole("button", { name: "Dark" }).getAttribute("aria-pressed"),
  ).toBe("true");
  expect(localStorage.getItem("cuaderno-theme")).toBe("dark");
});

test("toggling metrics flips the switch and persists", () => {
  renderDialog();
  const toggle = screen.getByRole("switch", { name: "Show progress metrics" });
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-checked")).toBe("true");
  expect(localStorage.getItem("cuaderno-show-metrics")).toBe("true");
});

test("Edit… closes the dialog and routes to the config editor", () => {
  const onOpenChange = vi.fn();
  renderDialog(onOpenChange);
  fireEvent.click(screen.getByRole("button", { name: "Edit…" }));
  expect(onOpenChange).toHaveBeenCalledWith(false);
  expect(screen.getByTestId("path").textContent).toBe("/config");
});

test("Done closes the dialog", () => {
  const onOpenChange = vi.fn();
  renderDialog(onOpenChange);
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(onOpenChange).toHaveBeenCalledWith(false);
});

test("is axe-clean", async () => {
  const { baseElement } = renderDialog();
  expect(await axe(baseElement)).toHaveNoViolations();
});
