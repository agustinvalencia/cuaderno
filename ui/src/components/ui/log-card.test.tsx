// LogCard (UI request 2026-07-12): a timestamped log line as a card with
// a time/date column beside the text. Renders the stamp when present,
// drops the column entirely when neither time nor date is given, and is
// axe-clean.
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, render, screen } from "@testing-library/react";
import { LogCard } from "./log-card";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

afterEach(cleanup);

test("renders time, date and text together", () => {
  render(
    <LogCard time="14:32" date="Jul 2">
      wired the watcher
    </LogCard>,
  );
  expect(screen.getByText("14:32")).toBeTruthy();
  expect(screen.getByText("Jul 2")).toBeTruthy();
  expect(screen.getByText("wired the watcher")).toBeTruthy();
});

test("shows a time-only stamp when no date is given", () => {
  render(<LogCard time="09:05">morning check-in</LogCard>);
  expect(screen.getByText("09:05")).toBeTruthy();
  expect(screen.getByText("morning check-in")).toBeTruthy();
});

test("renders just the text when neither time nor date is present", () => {
  const { container } = render(<LogCard>a bare line</LogCard>);
  expect(screen.getByText("a bare line")).toBeTruthy();
  // No stamp column — the card is a single content cell.
  expect(container.querySelector(".font-mono")).toBeNull();
});

test("is axe-clean", async () => {
  const { container } = render(
    <LogCard time="14:32" date="Jul 2">
      an entry
    </LogCard>,
  );
  expect(await axe(container)).toHaveNoViolations();
});
