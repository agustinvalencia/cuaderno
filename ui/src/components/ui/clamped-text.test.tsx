// ClampedText shows a "more"/"less" toggle only when its content overflows
// the collapsed cap. jsdom reports zero layout heights, so overflow is
// forced by stubbing scrollHeight/clientHeight (and ResizeObserver, which
// jsdom lacks) for the overflow cases.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ClampedText } from "./clamped-text";

globalThis.ResizeObserver ||= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;

function forceOverflow(scroll: number, client: number) {
  Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
    configurable: true,
    get() {
      return scroll;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "clientHeight", {
    configurable: true,
    get() {
      return client;
    },
  });
}

afterEach(() => {
  cleanup();
  // Restore jsdom's zero-height getters between tests.
  forceOverflow(0, 0);
});

test("renders its content", () => {
  render(
    <ClampedText>
      <p>a short line</p>
    </ClampedText>,
  );
  expect(screen.getByText("a short line")).toBeDefined();
});

test("content within the cap shows no toggle", () => {
  // jsdom's default zero heights are non-overflowing (0 > 0 is false).
  render(
    <ClampedText>
      <p>fits</p>
    </ClampedText>,
  );
  expect(screen.queryByRole("button")).toBeNull();
});

test("overflowing content reveals a more/less toggle that expands in place", () => {
  forceOverflow(500, 100);
  render(
    <ClampedText>
      <p>a very long wall of text that overflows the collapsed cap</p>
    </ClampedText>,
  );
  const toggle = screen.getByRole("button", { name: "more" });
  expect(toggle.getAttribute("aria-expanded")).toBe("false");

  fireEvent.click(toggle);
  expect(
    screen.getByRole("button", { name: "less" }).getAttribute("aria-expanded"),
  ).toBe("true");

  // Collapse again.
  fireEvent.click(screen.getByRole("button", { name: "less" }));
  expect(screen.getByRole("button", { name: "more" })).toBeDefined();
});

test("changing resetKey re-collapses (a swapped-in item never inherits expansion)", () => {
  // The project card swaps its surfaced action in place (energy filter,
  // refetch) without remounting ClampedText; a stale `expanded` would blow
  // the new action out fully. `resetKey` (the action identity) forces a
  // collapse on swap.
  forceOverflow(500, 100);
  const { rerender } = render(
    <ClampedText resetKey="action-a">
      <p>action A — a long wall of text past the cap</p>
    </ClampedText>,
  );
  fireEvent.click(screen.getByRole("button", { name: "more" }));
  expect(screen.getByRole("button", { name: "less" })).toBeDefined();

  // A different action swaps in — same component instance, new resetKey.
  rerender(
    <ClampedText resetKey="action-b">
      <p>action B — also a long wall of text past the cap</p>
    </ClampedText>,
  );
  expect(screen.getByRole("button", { name: "more" })).toBeDefined();
  expect(screen.queryByRole("button", { name: "less" })).toBeNull();
});
