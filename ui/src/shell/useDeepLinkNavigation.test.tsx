// useDeepLinkNavigation: a `deeplink:open-note` event (emitted by the Rust
// deep-link handler for a `cuaderno://note/<path>` link) navigates the reader
// to `/note/<path>`; an empty payload is a no-op.
import { afterEach, expect, test, vi } from "vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router";
import { useDeepLinkNavigation } from "./useDeepLinkNavigation";

// Capture the event callback so the test can fire a deep-link event.
let handler: ((event: { payload: string }) => void) | undefined;
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((_name: string, cb: (event: { payload: string }) => void) => {
    handler = cb;
    return Promise.resolve(() => {});
  }),
}));

// The mount-time `take_pending_deeplink` drain — `pending` is the buffered
// cold-start path (null for the warm/event tests).
let pending: string | null = null;
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => (cmd === "take_pending_deeplink" ? pending : null)),
}));

afterEach(() => {
  cleanup();
  handler = undefined;
  pending = null;
});

function Probe() {
  useDeepLinkNavigation();
  return <div data-testid="path">{useLocation().pathname}</div>;
}

test("a deeplink:open-note event navigates the reader to /note/<path>", async () => {
  render(
    <MemoryRouter initialEntries={["/"]}>
      <Probe />
    </MemoryRouter>,
  );
  await waitFor(() => expect(handler).toBeDefined());
  act(() => handler!({ payload: "portfolios/x/2026-07-13-note.md" }));
  expect(screen.getByTestId("path").textContent).toBe(
    "/note/portfolios/x/2026-07-13-note.md",
  );
});

test("a buffered cold-start deep link opens on mount (drained via take_pending_deeplink)", async () => {
  pending = "journal/2026/daily/2026-07-13.md";
  render(
    <MemoryRouter initialEntries={["/"]}>
      <Probe />
    </MemoryRouter>,
  );
  await waitFor(() =>
    expect(screen.getByTestId("path").textContent).toBe(
      "/note/journal/2026/daily/2026-07-13.md",
    ),
  );
});

test("an empty payload does not navigate", async () => {
  render(
    <MemoryRouter initialEntries={["/today"]}>
      <Probe />
    </MemoryRouter>,
  );
  await waitFor(() => expect(handler).toBeDefined());
  act(() => handler!({ payload: "" }));
  expect(screen.getByTestId("path").textContent).toBe("/today");
});
