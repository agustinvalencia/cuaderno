// useHistoryNavigation: back (button 3 / Cmd+[) and forward (button 4 /
// Cmd+]) step through router history; the side buttons' mousedown is
// suppressed (so a native webview default can't double-step); a keystroke
// in a text field is left alone; boundary presses are no-ops.
import { afterEach, expect, test, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router";
import { useHistoryNavigation } from "./useHistoryNavigation";

// The native-bridge `listen` isn't under test here; stub it so the hook's
// effect doesn't touch the (absent) Tauri IPC in jsdom.
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

afterEach(cleanup);

function Probe() {
  useHistoryNavigation();
  const location = useLocation();
  return (
    <div>
      <div data-testid="path">{location.pathname}</div>
      <input data-testid="field" />
    </div>
  );
}

function renderAt(entries: string[], index: number) {
  return render(
    <MemoryRouter initialEntries={entries} initialIndex={index}>
      <Probe />
    </MemoryRouter>,
  );
}

const path = () => screen.getByTestId("path").textContent;

function mouseUp(button: number) {
  act(() => {
    window.dispatchEvent(new MouseEvent("mouseup", { button, bubbles: true }));
  });
}

function key(k: string, target?: Element) {
  act(() => {
    const event = new KeyboardEvent("keydown", {
      key: k,
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    (target ?? window).dispatchEvent(event);
  });
}

test("mouse button 3 (back) / 4 (forward) step through history", () => {
  renderAt(["/a", "/b"], 1);
  expect(path()).toBe("/b");
  mouseUp(3);
  expect(path()).toBe("/a");
  mouseUp(4);
  expect(path()).toBe("/b");
});

test("Cmd+[ goes back and Cmd+] goes forward", () => {
  renderAt(["/a", "/b"], 1);
  key("[");
  expect(path()).toBe("/a");
  key("]");
  expect(path()).toBe("/b");
});

test("the side buttons' mousedown is preventDefault'd; a normal button isn't", () => {
  renderAt(["/a", "/b"], 1);
  const back = new MouseEvent("mousedown", {
    button: 3,
    bubbles: true,
    cancelable: true,
  });
  act(() => window.dispatchEvent(back));
  expect(back.defaultPrevented).toBe(true);

  const left = new MouseEvent("mousedown", {
    button: 0,
    bubbles: true,
    cancelable: true,
  });
  act(() => window.dispatchEvent(left));
  expect(left.defaultPrevented).toBe(false);
});

test("the keyboard shortcut is ignored while typing in a field", () => {
  renderAt(["/a", "/b"], 1);
  key("[", screen.getByTestId("field"));
  expect(path()).toBe("/b");
});

test("a normal button and a bare key do not navigate", () => {
  renderAt(["/a", "/b"], 1);
  mouseUp(0);
  act(() =>
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "[", bubbles: true }),
    ),
  );
  expect(path()).toBe("/b");
});

test("boundary presses are no-ops, not crashes", () => {
  renderAt(["/a"], 0);
  mouseUp(3); // back at the first entry
  expect(path()).toBe("/a");
  mouseUp(4); // forward at the last entry
  expect(path()).toBe("/a");
});
