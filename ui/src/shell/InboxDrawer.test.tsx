// Inbox drawer: renders captures from the list_inbox fixture and fires
// discard_inbox_item with the item's slug on discard.
import { createRef } from "react";
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ReaderProvider } from "./reader";
import { ToastProvider } from "./Toasts";
import type { InboxItem } from "../api/bindings/InboxItem";
import InboxDrawer from "./InboxDrawer";

const FIXTURE: InboxItem[] = [
  { slug: "2026-07-07-buy-milk", text: "buy milk" },
  { slug: "2026-07-06-call-the-dentist", text: "call the dentist" },
];

function renderDrawer() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const ref = createRef<HTMLButtonElement>();
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter>
          <ReaderProvider>
            <InboxDrawer onClose={() => {}} returnFocusRef={ref} />
          </ReaderProvider>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("renders inbox items from the fixture", async () => {
  mockIPC((cmd) => (cmd === "list_inbox" ? FIXTURE : undefined));
  renderDrawer();

  expect(await screen.findByText("buy milk")).toBeDefined();
  expect(screen.getByText("call the dentist")).toBeDefined();
});

test("renders a capture's markdown (heading, bold, wikilink) rather than raw text", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  const md: InboxItem[] = [
    { slug: "2026-07-08-draft", text: "## Draft\nPing **Sam** about [[weekly-sync]]" },
  ];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_inbox") return md;
    if (cmd === "resolve_wikilink") {
      return { path: "commitments/weekly-sync.md", note_type: "commitment" };
    }
    return undefined;
  });
  const { container } = renderDrawer();

  // The markdown renders as elements — the raw `##`/`**` never leak as text.
  expect(await screen.findByRole("heading", { name: "Draft" })).toBeDefined();
  expect(container.querySelector("strong")?.textContent).toBe("Sam");
  // The wikilink is a real anchor whose click resolves via resolve_wikilink.
  const link = container.querySelector("a[data-wikilink]");
  expect(link).not.toBeNull();
  fireEvent.click(link!);
  await waitFor(() => {
    expect(calls.find((c) => c.cmd === "resolve_wikilink")).toBeDefined();
  });
});

test("empty inbox renders the calm zero state", async () => {
  mockIPC((cmd) => (cmd === "list_inbox" ? [] : undefined));
  renderDrawer();

  expect(await screen.findByText(/Inbox zero/)).toBeDefined();
});

test("discard fires discard_inbox_item with the item's slug", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_inbox") return FIXTURE;
    return undefined;
  });
  renderDrawer();

  const discardBtn = await screen.findByRole("button", { name: /Discard: buy milk/ });
  fireEvent.click(discardBtn);

  await screen.findByText("call the dentist");
  const discarded = calls.find((c) => c.cmd === "discard_inbox_item");
  expect(discarded?.args).toMatchObject({ slug: "2026-07-07-buy-milk" });
});
