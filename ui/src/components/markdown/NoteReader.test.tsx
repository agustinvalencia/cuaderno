// NoteReader: renders a fixture note via mockIPC, and routes a
// wikilink click three ways — a project target navigates (and closes),
// a plain note replaces the reader in place, and an unresolved target
// is a silent no-op.
import { afterEach, expect, test, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { NoteView } from "../../api/bindings/NoteView";
import type { ResolvedLink } from "../../api/bindings/ResolvedLink";
import { ToastProvider } from "../../shell/Toasts";
import NoteReader from "./NoteReader";

// jsdom lacks the layout APIs Radix Dialog reaches for.
if (!Element.prototype.scrollIntoView)
  Element.prototype.scrollIntoView = () => {};
globalThis.ResizeObserver ||= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;

const NOTE: NoteView = {
  path: "zettels/example.md",
  note_type: "zettel",
  title: "Example note",
  frontmatter: { type: "zettel", context: "work", tags: ["a", "b"] },
  body: "Body with a [[garden]] link and a [[some-note|plain one]].",
};

function renderReader(
  onClose: () => void,
  onNavigate: (path: string) => void,
  resolve: (target: string) => ResolvedLink | null,
) {
  mockIPC((cmd, args) => {
    if (cmd === "read_note") return NOTE;
    if (cmd === "resolve_wikilink")
      return resolve((args as { target: string }).target);
    return undefined;
  });
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter>
          <NoteReader
            path={NOTE.path}
            onClose={onClose}
            onNavigate={onNavigate}
          />
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("renders the note title, a separated frontmatter panel, and body", async () => {
  renderReader(
    () => {},
    () => {},
    () => null,
  );
  expect(await screen.findByText("Example note")).toBeDefined();
  // Scalar frontmatter shows in a distinct, labelled metadata block as
  // key/value pairs; the array field is skipped.
  expect(screen.getByText("Properties")).toBeDefined();
  expect(screen.getByText("context")).toBeDefined();
  expect(screen.getByText("work")).toBeDefined();
  expect(screen.queryByText(/tags/)).toBeNull();
});

test("sections a daily-shaped note body and renders its Logs as cards", async () => {
  // The reader now shares the calendar's sectioned rendering: a body with
  // `## ` sections gets titled blocks, and a `## Logs` history becomes a
  // stack of timestamped cards — not one flat markdown blob.
  const daily: NoteView = {
    path: "journal/2026/daily/2026-07-12.md",
    note_type: "daily",
    title: "Saturday 12 July",
    frontmatter: { type: "daily" },
    body: "## Standup\n\nPlan the day.\n\n## Logs\n\n- **09:05**: started\n- **14:32**: shipped it\n",
  };
  mockIPC((cmd) => (cmd === "read_note" ? daily : undefined));
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter>
          <NoteReader
            path={daily.path}
            onClose={() => {}}
            onNavigate={() => {}}
          />
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );

  expect(await screen.findByRole("heading", { name: "Logs" })).toBeDefined();
  expect(screen.getByRole("heading", { name: "Standup" })).toBeDefined();
  expect(screen.getByText("09:05")).toBeDefined();
  expect(screen.getByText("started")).toBeDefined();
  expect(screen.getByText("shipped it")).toBeDefined();
});

test("a project wikilink navigates and closes the reader", async () => {
  const onClose = vi.fn();
  const onNavigate = vi.fn();
  renderReader(onClose, onNavigate, (target) =>
    target === "garden"
      ? { path: "projects/garden.md", note_type: "project" }
      : null,
  );
  fireEvent.click(await screen.findByText("garden"));
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(onNavigate).not.toHaveBeenCalled();
});

test("a plain-note wikilink replaces the reader in place", async () => {
  const onClose = vi.fn();
  const onNavigate = vi.fn();
  renderReader(onClose, onNavigate, () => ({
    path: "zettels/other.md",
    note_type: "zettel",
  }));
  fireEvent.click(await screen.findByText("plain one"));
  await waitFor(() =>
    expect(onNavigate).toHaveBeenCalledWith("zettels/other.md"),
  );
  expect(onClose).not.toHaveBeenCalled();
});

test("an unresolved wikilink is a silent no-op", async () => {
  const onClose = vi.fn();
  const onNavigate = vi.fn();
  renderReader(onClose, onNavigate, () => null);
  fireEvent.click(await screen.findByText("garden"));
  // Give the async resolve a tick to settle before asserting nothing.
  await new Promise((r) => setTimeout(r, 0));
  expect(onClose).not.toHaveBeenCalled();
  expect(onNavigate).not.toHaveBeenCalled();
});
