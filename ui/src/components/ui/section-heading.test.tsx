// SectionHeading: the shared section-label heading. Renders an h2 by
// default, an h3 under `as`, appends extra layout classes, and is
// axe-clean.
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, render, screen } from "@testing-library/react";
import { SectionHeading } from "./section-heading";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

afterEach(cleanup);

test("renders an h2 with the section-label classes by default", () => {
  render(<SectionHeading>Due soon</SectionHeading>);
  const heading = screen.getByRole("heading", { level: 2, name: "Due soon" });
  expect(heading.className).toBe(
    "text-xs font-medium uppercase tracking-wider text-ink-faint",
  );
});

test("renders an h3 when as='h3'", () => {
  render(<SectionHeading as="h3">Sub</SectionHeading>);
  expect(screen.getByRole("heading", { level: 3, name: "Sub" })).toBeDefined();
});

test("appends extra layout classes after the base", () => {
  render(<SectionHeading className="mt-6 px-2">Group</SectionHeading>);
  expect(screen.getByRole("heading", { name: "Group" }).className).toBe(
    "text-xs font-medium uppercase tracking-wider text-ink-faint mt-6 px-2",
  );
});

test("is axe-clean", async () => {
  const { container } = render(<SectionHeading>Section</SectionHeading>);
  expect(await axe(container)).toHaveNoViolations();
});
