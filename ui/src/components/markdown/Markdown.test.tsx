// Markdown renders LaTeX maths via remark-math + rehype-katex: inline
// `$…$` and display `$$…$$` become KaTeX output (a `.katex` element), the
// raw `$` source never leaks to the reader, and a malformed expression
// degrades to legible source rather than throwing (no red error box).
import { afterEach, expect, test } from "vitest";
import { cleanup, render } from "@testing-library/react";
import Markdown from "./Markdown";

afterEach(cleanup);

test("renders inline maths as KaTeX, not raw source", () => {
  const { container } = render(
    <Markdown body={"The edge $\\varphi_{\\ell,j}$ carries a spline."} onWikilink={() => {}} />,
  );
  expect(container.querySelector(".katex")).not.toBeNull();
  // The raw `$…$` delimiters must not survive into the rendered text.
  expect(container.textContent).not.toContain("$");
});

test("renders a display-maths block (its own line) as centred KaTeX", () => {
  // Display maths in real notes sits on its own line — the form that
  // yields a KaTeX *display* block (centred, own line), not inline maths.
  const { container } = render(
    <Markdown body={"before\n\n$$\n\\sum_k c_k B_k(x)\n$$\n\nafter"} onWikilink={() => {}} />,
  );
  expect(container.querySelector(".katex-display")).not.toBeNull();
});

test("a malformed expression degrades to source instead of throwing", () => {
  // `throwOnError: false` — an unbalanced brace renders the source, so the
  // whole note still renders (the reader never blanks on one bad formula).
  const { container } = render(
    <Markdown body={"broken $\\frac{1$ maths"} onWikilink={() => {}} />,
  );
  expect(container.textContent).toContain("broken");
  expect(container.textContent).toContain("maths");
});
