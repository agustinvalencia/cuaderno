// NotePage: the centred full-page reader/editor reached at `/note/<path>`.
// It renders a fixture note (title, separated frontmatter, body), runs the
// Read/Edit flow (raw load → Save writes → re-edit seeds from the saved
// text), and routes a wikilink click three ways — a project target
// navigates to its detail, a plain note replaces the page in place (a new
// `/note/*` navigation), and an unresolved target is a silent no-op.
import { afterEach, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useParams } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { NoteView } from "../../api/bindings/NoteView";
import type { ResolvedLink } from "../../api/bindings/ResolvedLink";
import { ToastProvider } from "../../shell/Toasts";
import { ReaderProvider } from "../../shell/reader";
import NotePage from "./NotePage";

// CodeMirror needs layout APIs jsdom lacks; stub the editor with a textarea
// that mirrors its seed-once + onChange contract, so the edit flow is testable.
vi.mock("../../components/markdown/MarkdownEditor", () => ({
  default: ({
    initialDoc,
    onChange,
  }: {
    initialDoc: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="editor"
      defaultValue={initialDoc}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const NOTE: NoteView = {
  path: "zettels/example.md",
  note_type: "zettel",
  title: "Example note",
  frontmatter: { type: "zettel", context: "work", tags: ["a", "b"] },
  body: "Body with a [[garden]] link and a [[some-note|plain one]].",
};

const RAW = "---\ntype: zettel\n---\n\n# Example note\n\nold body\n";

/** Stand-in for the project detail route, so a project wikilink's
 * navigation is observable by the slug it lands on. */
function ProjectProbe() {
  return <div data-testid="project-slug">{useParams().slug}</div>;
}

/** Render NotePage at `/note/<path>` with the raw-read + write commands
 * stubbed, capturing every IPC call. `staleTime: Infinity` mirrors the app
 * (main.tsx) so a save-primed cache isn't discarded on a re-edit. */
function renderNote(
  path: string,
  resolve?: (target: string) => ResolvedLink | null,
) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "read_note") return NOTE;
    if (cmd === "read_note_raw") return RAW;
    if (cmd === "write_note_raw") return null;
    if (cmd === "resolve_wikilink")
      return resolve?.((args as { target: string }).target) ?? null;
    return undefined;
  });
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  });
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={[`/note/${path}`]}>
          <ReaderProvider>
            <Routes>
              <Route path="/note/*" element={<NotePage />} />
              <Route path="/projects/:slug" element={<ProjectProbe />} />
              <Route path="/stewardships" element={<div data-testid="stewardships" />} />
            </Routes>
          </ReaderProvider>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
  return calls;
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("renders the title, a separated frontmatter panel, and body", async () => {
  renderNote(NOTE.path);
  expect(await screen.findByText("Example note")).toBeDefined();
  // Scalar frontmatter shows as key/value pairs in a labelled block; the
  // array field is skipped.
  expect(screen.getByText("Properties")).toBeDefined();
  expect(screen.getByText("context")).toBeDefined();
  expect(screen.getByText("work")).toBeDefined();
  expect(screen.queryByText(/tags/)).toBeNull();
});

test("Edit loads the raw note into the editor, and Save writes the change", async () => {
  const calls = renderNote(NOTE.path);
  fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
  const editor = (await screen.findByLabelText("editor")) as HTMLTextAreaElement;
  expect(editor.value).toContain("old body");

  fireEvent.change(editor, { target: { value: "brand new body" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => {
    const write = calls.find((c) => c.cmd === "write_note_raw");
    expect(write?.args).toMatchObject({ path: NOTE.path, content: "brand new body" });
  });
});

test("Save with no keystroke writes the original raw content (draft seeded)", async () => {
  const calls = renderNote(NOTE.path);
  fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
  await screen.findByLabelText("editor"); // raw loaded → draft seeded
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => {
    const write = calls.find((c) => c.cmd === "write_note_raw");
    expect(write?.args).toMatchObject({ path: NOTE.path, content: RAW });
  });
});

test("re-editing after a save starts from the saved content, not the pre-save cache", async () => {
  renderNote(NOTE.path);
  fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
  const editor = (await screen.findByLabelText("editor")) as HTMLTextAreaElement;
  fireEvent.change(editor, { target: { value: "saved version two" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByRole("button", { name: "Edit" }); // back in read mode

  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const editor2 = (await screen.findByLabelText("editor")) as HTMLTextAreaElement;
  expect(editor2.value).toBe("saved version two");
});

test("navigating to another note resets edit state (no draft bleed across notes)", async () => {
  // Regression (severe): the route reused one component instance across
  // notes, so an in-progress draft for note A could be saved onto note B.
  // Keying the reader by path remounts a fresh instance on navigation, so
  // editing resets and the stale draft is gone.
  mockIPC((cmd) => {
    if (cmd === "read_note") return NOTE;
    if (cmd === "read_note_raw") return RAW;
    if (cmd === "write_note_raw") return null;
    return undefined;
  });
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  });
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter
          initialEntries={["/note/zettels/first.md", `/note/${NOTE.path}`]}
          initialIndex={1}
        >
          <ReaderProvider>
            <Routes>
              <Route path="/note/*" element={<NotePage />} />
            </Routes>
          </ReaderProvider>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
  fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
  await screen.findByLabelText("editor");
  // Back to the other note — allowed even mid-edit. The fresh instance must
  // be in read mode with no editor and no Save (the draft can't carry over).
  fireEvent.click(screen.getByRole("button", { name: "← back" }));
  expect(await screen.findByRole("button", { name: "Edit" })).toBeDefined();
  expect(screen.queryByLabelText("editor")).toBeNull();
  expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
});

test("a project wikilink navigates to its detail", async () => {
  renderNote(NOTE.path, (target) =>
    target === "garden"
      ? { path: "projects/garden.md", note_type: "project" }
      : null,
  );
  fireEvent.click(await screen.findByText("garden"));
  expect((await screen.findByTestId("project-slug")).textContent).toBe("garden");
});

test("a plain-note wikilink replaces the page in place", async () => {
  const calls = renderNote(NOTE.path, () => ({
    path: "zettels/other.md",
    note_type: "zettel",
  }));
  fireEvent.click(await screen.findByText("plain one"));
  // The page navigates to the linked note — a fresh read of the new path.
  await waitFor(() => {
    const read = calls.find(
      (c) => c.cmd === "read_note" && (c.args as { path: string }).path === "zettels/other.md",
    );
    expect(read).toBeDefined();
  });
});

test("an unresolved wikilink is a silent no-op", async () => {
  renderNote(NOTE.path, () => null);
  fireEvent.click(await screen.findByText("garden"));
  await new Promise((r) => setTimeout(r, 0));
  // Still on the original note — no project/stewardship route rendered.
  expect(screen.queryByTestId("project-slug")).toBeNull();
  expect(screen.getByText("Example note")).toBeDefined();
});
