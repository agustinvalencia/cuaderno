// The invalid-config banner (GH #365 PR4), driven through the module
// store directly — the same seam the event bridge writes to. Valid:
// nothing renders. Invalid: the calm (non-red) banner appears with the
// open error, and a later valid edit clears it.
import { afterEach, expect, test } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import { setConfigStatus } from "../lib/configStatus";
import ConfigStatusBanner from "./ConfigStatusBanner";

afterEach(() => {
  cleanup();
  // The store is module-global; reset so ordering between tests cannot
  // leak an invalid state.
  setConfigStatus({ valid: true, message: null });
});

test("renders nothing while the config is valid", () => {
  render(<ConfigStatusBanner />);
  expect(screen.queryByRole("status")).toBeNull();
});

test("shows the calm banner with the open error when the config is invalid", () => {
  render(<ConfigStatusBanner />);
  act(() => setConfigStatus({ valid: false, message: "expected `=`" }));

  const banner = screen.getByRole("status");
  expect(banner.textContent).toContain("config.toml has an error");
  expect(banner.textContent).toContain("expected `=`");
  // Calm design: attention (amber) tier, never red.
  expect(banner.className).toContain("text-attention");
});

test("omits the detail line when the payload carries no message", () => {
  render(<ConfigStatusBanner />);
  act(() => setConfigStatus({ valid: false, message: null }));

  const banner = screen.getByRole("status");
  // The lead sentence renders; no monospace detail span follows it.
  expect(banner.textContent).toContain("was not applied");
  expect(banner.querySelector(".font-mono")).toBeNull();
});

test("the banner disappears again once a valid edit clears the notice", () => {
  render(<ConfigStatusBanner />);
  act(() => setConfigStatus({ valid: false, message: "bad" }));
  expect(screen.getByRole("status")).toBeDefined();
  act(() => setConfigStatus({ valid: true, message: null }));
  expect(screen.queryByRole("status")).toBeNull();
});
