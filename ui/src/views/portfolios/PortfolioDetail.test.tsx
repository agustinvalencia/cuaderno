// Portfolio Detail: the evidence timeline and links sidebar render; an
// evidence row opens the reader; the quick-add composer submits with its
// args; and an unresolvable origin surfaces its message inline.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useParams } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { PortfolioDetail as PortfolioDetailData } from "../../api/bindings/PortfolioDetail";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
import PortfolioDetail from "./PortfolioDetail";

const DETAIL: PortfolioDetailData = {
  slug: "surrogate",
  question: "How does the surrogate behave?",
  created: "2026-06-01",
  project: "projects/alpha",
  questions: ["questions/research/surrogate-fidelity"],
  evidence: [
    {
      path: "portfolios/surrogate/2026-07-01-smith-2024.md",
      created: "2026-07-01",
      source: "Smith 2024",
      origin: "projects/alpha",
    },
  ],
};

const EMPTY: PortfolioDetailData = {
  slug: "fresh",
  question: "A brand-new question?",
  created: "2026-07-01",
  project: null,
  questions: [],
  evidence: [],
};

// The note page opening on `path` is now a navigation to `/note/<path>`;
// this stand-in route surfaces the navigated path so a test can assert a
// click opened the right note.
function NotePathProbe() {
  return <div data-testid="reader-path">{useParams()["*"] ?? ""}</div>;
}

function renderDetail(
  fixture: PortfolioDetailData,
  handlers?: {
    onCall?: (cmd: string, args: unknown) => void;
    addEvidence?: () => unknown;
  },
) {
  mockIPC((cmd, args) => {
    handlers?.onCall?.(cmd, args);
    if (cmd === "get_portfolio") return fixture;
    if (cmd === "add_evidence") return handlers?.addEvidence?.() ?? undefined;
    if (cmd === "resolve_wikilink") return null;
    return undefined;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={[`/portfolios/${fixture.slug}`]}>
          {/* ReaderProvider needs a Router above it (it navigates); the
              `/note/*` stand-in route surfaces the opened path. */}
          <ReaderProvider>
            <Routes>
              <Route path="/portfolios/:slug" element={<PortfolioDetail />} />
              <Route path="/note/*" element={<NotePathProbe />} />
            </Routes>
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

test("renders the evidence timeline and the links sidebar", async () => {
  renderDetail(DETAIL);
  expect(await screen.findByText("How does the surrogate behave?")).toBeDefined();
  // Evidence row: source, and the origin chip (last segment of the link).
  expect(screen.getByText("Smith 2024")).toBeDefined();
  expect(screen.getByRole("region", { name: "Evidence" })).toBeDefined();
  // Links sidebar: the project and the related question, by last segment.
  const sidebar = screen.getByRole("complementary", { name: "Links" });
  expect(sidebar.textContent).toContain("alpha");
  expect(sidebar.textContent).toContain("surrogate-fidelity");
});

test("the empty state invites the first artefact", async () => {
  renderDetail(EMPTY);
  expect(await screen.findByText("A brand-new question?")).toBeDefined();
  expect(screen.getByText(/waiting for its first artefact/)).toBeDefined();
});

test("an evidence row opens the note page at its path", async () => {
  renderDetail(DETAIL);
  fireEvent.click(await screen.findByText("Smith 2024"));
  expect((await screen.findByTestId("reader-path")).textContent).toBe(
    "portfolios/surrogate/2026-07-01-smith-2024.md",
  );
});

test("the quick-add composer submits with its args", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderDetail(DETAIL, { onCall: (cmd, args) => calls.push({ cmd, args }) });

  fireEvent.click(await screen.findByRole("button", { name: "File evidence" }));
  fireEvent.change(screen.getByLabelText("Source"), { target: { value: "Lab notebook" } });
  fireEvent.change(screen.getByLabelText("Origin"), { target: { value: "projects/alpha" } });
  fireEvent.change(screen.getByLabelText("Notes"), { target: { value: "Reran the sweep." } });

  fireEvent.click(screen.getByRole("button", { name: "File it" }));
  expect(await screen.findByText("Filed.")).toBeDefined();

  const filed = calls.find((c) => c.cmd === "add_evidence");
  expect(filed?.args).toMatchObject({
    portfolio: "surrogate",
    source: "Lab notebook",
    origin: "projects/alpha",
    content: "Reran the sweep.",
  });
});

test("an unresolvable origin shows its message inline", async () => {
  renderDetail(DETAIL, {
    addEvidence: () => {
      // The backend's invalid-origin refusal, shaped as CmdError.
      throw { kind: "invalid", data: "origin does not resolve to a note: [[nope]]" };
    },
  });

  fireEvent.click(await screen.findByRole("button", { name: "File evidence" }));
  fireEvent.change(screen.getByLabelText("Source"), { target: { value: "Stray" } });
  fireEvent.change(screen.getByLabelText("Origin"), { target: { value: "nope" } });
  fireEvent.click(screen.getByRole("button", { name: "File it" }));

  // The message shows inline (the form stays open), not just as a toast.
  expect(await screen.findByText(/origin does not resolve to a note/)).toBeDefined();
  expect(screen.getByLabelText("Origin")).toBeDefined();
});

test("a stale error is cleared when the form is cancelled and reopened", async () => {
  renderDetail(DETAIL, {
    addEvidence: () => {
      throw { kind: "invalid", data: "origin does not resolve to a note: [[nope]]" };
    },
  });

  // Submit an invalid origin so the inline error appears.
  fireEvent.click(await screen.findByRole("button", { name: "File evidence" }));
  fireEvent.change(screen.getByLabelText("Source"), { target: { value: "Stray" } });
  fireEvent.change(screen.getByLabelText("Origin"), { target: { value: "nope" } });
  fireEvent.click(screen.getByRole("button", { name: "File it" }));
  expect(await screen.findByText(/origin does not resolve to a note/)).toBeDefined();

  // Cancel, then reopen the composer.
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  fireEvent.click(await screen.findByRole("button", { name: "File evidence" }));

  // The reopened, cleared form no longer greets the user with the error.
  expect(screen.queryByText(/origin does not resolve to a note/)).toBeNull();
});
