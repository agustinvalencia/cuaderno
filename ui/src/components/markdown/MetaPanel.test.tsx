// MetaPanel (UI request 2026-07-12): a note's scalar frontmatter shown as
// a distinct, labelled metadata block — separated from the prose so it no
// longer reads as the note's first heading. Scalars render as key/value
// pairs; nested/null values are skipped; empty frontmatter renders
// nothing (not an empty frame).
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, render, screen } from "@testing-library/react";
import { MetaPanel, scalarFrontmatter } from "./MetaPanel";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

afterEach(cleanup);

test("scalarFrontmatter keeps scalars and drops nested/null values", () => {
  const pairs = scalarFrontmatter({
    type: "project",
    active: true,
    priority: 2,
    created: null,
    tags: ["a", "b"],
    meta: { nested: 1 },
  });
  expect(pairs).toEqual([
    ["type", "project"],
    ["active", "true"],
    ["priority", "2"],
  ]);
});

test("scalarFrontmatter returns [] for non-objects", () => {
  expect(scalarFrontmatter(null)).toEqual([]);
  expect(scalarFrontmatter("frontmatter")).toEqual([]);
  expect(scalarFrontmatter(["a"])).toEqual([]);
});

test("renders a labelled properties block with the scalar pairs", () => {
  render(<MetaPanel frontmatter={{ type: "daily", context: "work" }} />);
  expect(screen.getByText("Properties")).toBeTruthy();
  expect(screen.getByText("type")).toBeTruthy();
  expect(screen.getByText("daily")).toBeTruthy();
  expect(screen.getByText("context")).toBeTruthy();
  expect(screen.getByText("work")).toBeTruthy();
});

test("renders nothing when there is no scalar frontmatter", () => {
  const { container } = render(<MetaPanel frontmatter={{ tags: ["x"] }} />);
  expect(container.firstChild).toBeNull();
});

test("is axe-clean", async () => {
  const { container } = render(<MetaPanel frontmatter={{ type: "project", context: "work" }} />);
  expect(await axe(container)).toHaveNoViolations();
});
