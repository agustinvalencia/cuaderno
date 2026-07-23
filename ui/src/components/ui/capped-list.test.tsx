import { expect, test } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach } from "vitest";

import { CappedList } from "./capped-list";

afterEach(cleanup);

function items(n: number) {
  return Array.from({ length: n }, (_, i) => <p key={i}>item {i + 1}</p>);
}

test("a short list shows everything and offers no toggle", () => {
  render(<CappedList items={items(3)} limit={5} label="things" />);

  expect(screen.getByText("item 3")).toBeDefined();
  expect(screen.queryByRole("button")).toBeNull();
});

test("a list exactly at the limit still offers no toggle", () => {
  // The affordance appears when it earns its place, not one item early.
  render(<CappedList items={items(5)} limit={5} label="things" />);

  expect(screen.queryByRole("button")).toBeNull();
});

test("a long list caps, and names the true total", () => {
  render(<CappedList items={items(23)} limit={5} label="backlinks" />);

  expect(screen.getByText("item 5")).toBeDefined();
  expect(screen.queryByText("item 6")).toBeNull();
  expect(screen.getByRole("button", { name: "Show all 23 backlinks" })).toBeDefined();
});

test("expanding reveals the rest in place and can be reversed", () => {
  render(<CappedList items={items(23)} limit={5} label="backlinks" />);

  fireEvent.click(screen.getByRole("button", { name: "Show all 23 backlinks" }));
  expect(screen.getByText("item 23")).toBeDefined();

  fireEvent.click(screen.getByRole("button", { name: "Show fewer backlinks" }));
  expect(screen.queryByText("item 23")).toBeNull();
});

test("the toggle reports its state for assistive tech", () => {
  render(<CappedList items={items(23)} limit={5} label="backlinks" />);

  const toggle = screen.getByRole("button");
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
});
