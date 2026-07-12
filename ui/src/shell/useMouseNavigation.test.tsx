// useMouseNavigation: the back (button 3) and forward (button 4) mouse
// side buttons step through router history, on mouseup.
import { afterEach, expect, test } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router";
import { useMouseNavigation } from "./useMouseNavigation";

afterEach(cleanup);

function Probe() {
  useMouseNavigation();
  const location = useLocation();
  return <div data-testid="path">{location.pathname}</div>;
}

function renderAt(entries: string[], index: number) {
  return render(
    <MemoryRouter initialEntries={entries} initialIndex={index}>
      <Probe />
    </MemoryRouter>,
  );
}

function press(button: number) {
  act(() => {
    window.dispatchEvent(new MouseEvent("mouseup", { button, bubbles: true }));
  });
}

test("button 3 (back) steps to the previous history entry", () => {
  renderAt(["/a", "/b"], 1);
  expect(screen.getByTestId("path").textContent).toBe("/b");
  press(3);
  expect(screen.getByTestId("path").textContent).toBe("/a");
});

test("button 4 (forward) steps to the next history entry", () => {
  renderAt(["/a", "/b"], 0);
  expect(screen.getByTestId("path").textContent).toBe("/a");
  press(4);
  expect(screen.getByTestId("path").textContent).toBe("/b");
});

test("a normal button (0) does not navigate", () => {
  renderAt(["/a", "/b"], 1);
  press(0);
  expect(screen.getByTestId("path").textContent).toBe("/b");
});

test("back at the first entry is a no-op, not a crash", () => {
  renderAt(["/a"], 0);
  press(3);
  expect(screen.getByTestId("path").textContent).toBe("/a");
});
