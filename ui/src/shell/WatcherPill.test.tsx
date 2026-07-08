// The degraded-watcher pill (plan §3.1), driven through the module
// store directly — the same seam the event bridge writes to. Healthy:
// nothing renders. Degraded: the muted pill appears and the 60s poll
// fallback invalidates all queries until recovery clears both.
import { afterEach, expect, test, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { setWatcherState } from "../lib/watcherStatus";
import WatcherPill from "./WatcherPill";

function renderPill() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={client}>
      <WatcherPill />
    </QueryClientProvider>,
  );
  return { client, view };
}

afterEach(() => {
  cleanup();
  // The store is module-global; reset so ordering between tests
  // cannot leak a degraded state.
  setWatcherState("ok");
  vi.useRealTimers();
});

test("renders nothing while the watcher is healthy", () => {
  renderPill();
  expect(screen.queryByText("live updates paused")).toBeNull();
});

test("shows the muted pill when the store reports degraded", () => {
  renderPill();
  act(() => setWatcherState("degraded"));
  const pill = screen.getByText("live updates paused");
  expect(pill.getAttribute("title")).toBe("live updates paused — refresh with focus");
});

test("the pill disappears again on recovery", () => {
  renderPill();
  act(() => setWatcherState("degraded"));
  expect(screen.getByText("live updates paused")).toBeDefined();
  act(() => setWatcherState("ok"));
  expect(screen.queryByText("live updates paused")).toBeNull();
});

test("while degraded, all queries are invalidated every 60s; recovery stops the poll", () => {
  vi.useFakeTimers();
  const { client } = renderPill();
  const invalidate = vi.spyOn(client, "invalidateQueries");

  act(() => setWatcherState("degraded"));
  act(() => vi.advanceTimersByTime(60_000));
  expect(invalidate).toHaveBeenCalledTimes(1);
  // The poll fallback is global on purpose: a degraded watcher means
  // area classification can no longer be trusted.
  expect(invalidate).toHaveBeenCalledWith();

  act(() => vi.advanceTimersByTime(60_000));
  expect(invalidate).toHaveBeenCalledTimes(2);

  act(() => setWatcherState("ok"));
  act(() => vi.advanceTimersByTime(180_000));
  expect(invalidate).toHaveBeenCalledTimes(2);
});
