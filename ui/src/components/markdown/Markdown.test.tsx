// Markdown renders LaTeX maths via remark-math + rehype-katex: inline
// `$…$` and display `$$…$$` become KaTeX output (a `.katex` element), the
// raw `$` source never leaks to the reader, and a malformed expression
// degrades to legible source rather than throwing (no red error box). It
// also resolves a note-relative embedded image to vault bytes (a `data:`
// URI) when a note-path context is present, and degrades to the caption
// when it isn't.
import { afterEach, expect, test } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import Markdown, { NotePathProvider } from "./Markdown";

afterEach(() => {
  cleanup();
  clearMocks();
});

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

test("a note-relative image renders as a data URI via read_note_asset", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "read_note_asset") return "data:image/png;base64,AAAA";
    return undefined;
  });
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  });
  const { container } = render(
    <QueryClientProvider client={client}>
      <NotePathProvider path="portfolios/x/note.md">
        <Markdown body={"![a figure](assets/fig.png)"} onWikilink={() => {}} />
      </NotePathProvider>
    </QueryClientProvider>,
  );
  const img = await waitFor(() => {
    const el = container.querySelector("img");
    if (!el) throw new Error("image not resolved yet");
    return el;
  });
  expect(img.getAttribute("src")).toBe("data:image/png;base64,AAAA");
  expect(img.getAttribute("alt")).toBe("a figure");
  // The command was asked for the src relative to the note path.
  const asset = calls.find((c) => c.cmd === "read_note_asset");
  expect(asset?.args).toMatchObject({ notePath: "portfolios/x/note.md", src: "assets/fig.png" });
});

test("a relative image with no note context degrades to its caption", () => {
  const { container } = render(
    <Markdown body={"![the caption](assets/fig.png)"} onWikilink={() => {}} />,
  );
  // No note path in scope → nothing to fetch; show the caption, not a
  // broken <img>.
  expect(container.querySelector("img")).toBeNull();
  expect(screen.getByText("the caption")).toBeDefined();
});

test("a single newline renders as a line break (Obsidian-style), not a joined paragraph", () => {
  // A standup's sub-lines (`Yesterday` / `Today` / `Due soon`) are separated
  // by single newlines; remark-breaks keeps them on their own lines rather
  // than collapsing them into one flowing paragraph (CommonMark's default).
  const { container } = render(
    <Markdown body={"**Yesterday** — did a thing\n**Today** — do another\n**Due soon** — soon"} onWikilink={() => {}} />,
  );
  // The three lines live in one paragraph, separated by hard <br>s.
  const paragraph = container.querySelector("p");
  expect(paragraph).not.toBeNull();
  expect(paragraph?.querySelectorAll("br").length).toBe(2);
  // All three labels survived.
  expect(container.textContent).toContain("Yesterday");
  expect(container.textContent).toContain("Today");
  expect(container.textContent).toContain("Due soon");
});

test("remark-breaks leaves code-block newlines alone (no <br> inside <pre>)", () => {
  // A regression guard for the global soft-break change: newlines INSIDE a
  // fenced code block are literal content, not soft breaks, so they must never
  // become <br> — the code's line structure has to survive verbatim.
  const { container } = render(
    <Markdown body={"```\nline1\nline2\n```"} onWikilink={() => {}} />,
  );
  const pre = container.querySelector("pre");
  expect(pre).not.toBeNull();
  expect(pre?.querySelectorAll("br").length).toBe(0);
  expect(pre?.textContent).toContain("line1");
  expect(pre?.textContent).toContain("line2");
});
