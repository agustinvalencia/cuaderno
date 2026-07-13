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

afterEach(() => {
  cleanup();
  handler = undefined;
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
