// Command palette: opens on Cmd+K (via the shell), renders note search
// results from mockIPC, and the Capture verb submits capture_quick.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { OrientationView } from "../api/bindings/OrientationView";
import type { SearchResultEntry } from "../api/bindings/SearchResultEntry";
import AppShell from "./AppShell";
import CommandPalette from "./CommandPalette";
import { ReaderProvider } from "./reader";
import { ToastProvider } from "./Toasts";

// jsdom lacks the layout APIs cmdk + Radix Dialog reach for.
if (!Element.prototype.scrollIntoView) Element.prototype.scrollIntoView = () => {};
globalThis.ResizeObserver ||= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;

const EMPTY_ORIENTATION: OrientationView = {
  today: "2026-07-07",
  commitments: [],
  projects: [],
  lapsed_habits: [],
};

const RESULTS: SearchResultEntry[] = [
  { path: "zettels/foo.md", note_type: "zettel", title: "Foo idea", snippet: "…", score: 1 },
];

function client() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("Cmd+K opens the palette", async () => {
  mockIPC((cmd) => {
    if (cmd === "get_orientation") return EMPTY_ORIENTATION;
    if (cmd === "list_inbox") return [];
    return undefined;
  });
  render(
    <QueryClientProvider client={client()}>
      <ToastProvider>
        <MemoryRouter>
          <AppShell />
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
  fireEvent.keyDown(window, { key: "k", metaKey: true });
  expect(await screen.findByPlaceholderText(/Search notes or jump/)).toBeDefined();
});

function renderPalette(onCall?: (cmd: string, args: unknown) => void) {
  mockIPC((cmd, args) => {
    onCall?.(cmd, args);
    if (cmd === "search_vault") return RESULTS;
    return undefined;
  });
  return render(
    <QueryClientProvider client={client()}>
      <ToastProvider>
        <MemoryRouter>
          <ReaderProvider>
            <CommandPalette open onOpenChange={() => {}} />
          </ReaderProvider>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

test("typing surfaces note search results (debounced) from mockIPC", async () => {
  renderPalette();
  fireEvent.change(screen.getByPlaceholderText(/Search notes or jump/), {
    target: { value: "foo" },
  });
  // The 150ms debounce lands well inside findBy's default timeout.
  expect(await screen.findByText("Foo idea")).toBeDefined();
  expect(screen.getByText("zettel")).toBeDefined();
});

test("the Capture verb submits capture_quick", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderPalette((cmd, args) => calls.push({ cmd, args }));
  fireEvent.click(await screen.findByText("Capture…"));
  const input = await screen.findByLabelText("Capture to inbox");
  fireEvent.change(input, { target: { value: "buy milk" } });
  fireEvent.submit(input.closest("form")!);
  // Await the success toast so the (microtask-deferred) mutation has run.
  expect(await screen.findByText(/Captured to your inbox/)).toBeDefined();
  const captured = calls.find((c) => c.cmd === "capture_quick");
  expect(captured?.args).toMatchObject({ text: "buy milk" });
});
