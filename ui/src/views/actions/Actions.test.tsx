// Actions view: groups open actions by project, filters by energy
// (dropping groups that empty out), and promotes an unattached bullet.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { ProjectActions } from "../../api/bindings/ProjectActions";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
import Actions from "./Actions";

const GROUPS: ProjectActions[] = [
  {
    slug: "alpha",
    context: "work",
    actions: [
      { text: "Draft methods (deep)", energy: "deep", attached: null },
      { text: "File receipts (light)", energy: "light", attached: null },
    ],
  },
  {
    slug: "beta",
    context: "family",
    actions: [
      { text: "Water plants (light)", energy: "light", attached: { slug: "water-plants", status: "active" } },
    ],
  },
];

function renderActions(onCall?: (cmd: string, args: unknown) => void) {
  mockIPC((cmd, args) => {
    onCall?.(cmd, args);
    if (cmd === "list_all_actions") return GROUPS;
    return undefined;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <ReaderProvider>
          <MemoryRouter>
            <Actions />
          </MemoryRouter>
        </ReaderProvider>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("groups actions under their project headers", async () => {
  renderActions();
  expect(await screen.findByText("alpha")).toBeDefined();
  expect(screen.getByText("beta")).toBeDefined();
  expect(screen.getByText("Draft methods (deep)")).toBeDefined();
  expect(screen.getByText("Water plants (light)")).toBeDefined();
});

test("the energy filter narrows to matching bullets and drops empty groups", async () => {
  renderActions();
  await screen.findByText("alpha");
  fireEvent.click(screen.getByRole("button", { name: "deep" }));
  expect(screen.getByText("Draft methods (deep)")).toBeDefined();
  // beta has no deep action, so its group disappears entirely.
  expect(screen.queryByText("File receipts (light)")).toBeNull();
  expect(screen.queryByText("beta")).toBeNull();
});

test("promote fires promote_action for an unattached bullet", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderActions((cmd, args) => calls.push({ cmd, args }));
  fireEvent.click(await screen.findByRole("button", { name: /Promote to a note: Draft methods/ }));
  // Await the success toast so the (microtask-deferred) mutation has run.
  expect(await screen.findByText(/Promoted to an action note/)).toBeDefined();
  const promoted = calls.find((c) => c.cmd === "promote_action");
  expect(promoted?.args).toMatchObject({ project: "alpha", action: "Draft methods (deep)" });
});
