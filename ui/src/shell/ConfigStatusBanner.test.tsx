// The config-status banner (GH #365 PR4, #384), driven through the module
// store directly — the same seam the event bridge writes to. Valid: nothing
// renders. Invalid: the calm (non-red) "config has an error" banner. Deferred:
// the calm "vault was busy" banner. A later valid edit clears either.
import { afterEach, expect, test } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import { setConfigStatus } from "../lib/configStatus";
import ConfigStatusBanner from "./ConfigStatusBanner";

afterEach(() => {
  cleanup();
  // The store is module-global; reset so ordering between tests cannot
  // leak a non-valid state.
  setConfigStatus({ health: "valid", message: null });
});

test("renders nothing while the config is valid", () => {
  render(<ConfigStatusBanner />);
  expect(screen.queryByRole("status")).toBeNull();
});

test("shows the calm banner with the open error when the config is invalid", () => {
  render(<ConfigStatusBanner />);
  act(() => setConfigStatus({ health: "invalid", message: "expected `=`" }));

  const banner = screen.getByRole("status");
  expect(banner.textContent).toContain("config.toml has an error");
  expect(banner.textContent).toContain("expected `=`");
  // Calm design: attention (amber) tier, never red.
  expect(banner.className).toContain("text-attention");
});

test("shows a distinct calm banner when the reload was deferred (vault busy)", () => {
  render(<ConfigStatusBanner />);
  act(() => setConfigStatus({ health: "deferred", message: "vault write lock timed out" }));

  const banner = screen.getByRole("status");
  // The deferred wording is distinct from the invalid-config wording — it
  // must not accuse the config of being broken (#384).
  expect(banner.textContent).toContain("vault was busy");
  expect(banner.textContent).not.toContain("config.toml has an error");
  expect(banner.textContent).toContain("vault write lock timed out");
  // Same calm tier as the invalid banner.
  expect(banner.className).toContain("text-attention");
});

test("omits the detail line when the payload carries no message", () => {
  render(<ConfigStatusBanner />);
  act(() => setConfigStatus({ health: "invalid", message: null }));

  const banner = screen.getByRole("status");
  // The lead sentence renders; no monospace detail span follows it.
  expect(banner.textContent).toContain("was not applied");
  expect(banner.querySelector(".font-mono")).toBeNull();
});

test("the banner disappears again once a valid edit clears the notice", () => {
  render(<ConfigStatusBanner />);
  act(() => setConfigStatus({ health: "deferred", message: "busy" }));
  expect(screen.getByRole("status")).toBeDefined();
  act(() => setConfigStatus({ health: "valid", message: null }));
  expect(screen.queryByRole("status")).toBeNull();
});
