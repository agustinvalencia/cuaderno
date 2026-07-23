// Actions view (#445): a project rail with counts, a sticky filter bar
// whose chips carry their own counts — including the "untagged" bucket
// the old filter silently dropped — a text filter, and one shared
// ambiguity picker rather than one per row.
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { ProjectActions } from "../../api/bindings/ProjectActions";
import { ReaderProvider } from "../../shell/reader";
import { ToastProvider } from "../../shell/Toasts";
import Actions from "./Actions";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

// `beta` sorts after `alpha`, and is declared first, so the group order
// also proves the sort rather than echoing the backend's order.
const GROUPS: ProjectActions[] = [
  {
    slug: "beta",
    context: "family",
    actions: [
      { text: "Water plants", energy: "light", attached: { slug: "water-plants", status: "active" } },
    ],
  },
  {
    slug: "alpha",
    context: "work",
    actions: [
      { text: "Draft methods (deep)", energy: "deep", attached: null },
      { text: "File receipts (light)", energy: "light", attached: null },
      // No energy suffix: the bullet the old filter had no chip for.
      { text: "Chase the supplier", energy: null, attached: null },
    ],
  },
];

function renderActions(
  onCall?: (cmd: string, args: unknown) => void,
  groups: ProjectActions[] = GROUPS,
) {
  mockIPC((cmd, args) => {
    onCall?.(cmd, args);
    if (cmd === "list_all_actions") return groups;
    return undefined;
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter>
          <ReaderProvider>
            <Actions />
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

/** The list pane, excluding the rail — both name the projects. */
function list() {
  return within(screen.getByRole("region", { name: "alpha" }));
}

function rail() {
  return within(screen.getByRole("navigation", { name: "Projects" }));
}

test("the rail lists every project with its count, newest filters applied", async () => {
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });

  expect(rail().getByRole("button", { name: /All actions/ }).textContent).toContain("4");
  expect(rail().getByRole("button", { name: /alpha/ }).textContent).toContain("3");
  expect(rail().getByRole("button", { name: /beta/ }).textContent).toContain("1");
});

test("choosing a project narrows the list to it", async () => {
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });
  expect(screen.getByRole("region", { name: "beta" })).toBeDefined();

  fireEvent.click(rail().getByRole("button", { name: /alpha/ }));

  expect(screen.queryByRole("region", { name: "beta" })).toBeNull();
  expect(list().getByText("Draft methods")).toBeDefined();
});

test("an untagged action is reachable, not silently dropped", async () => {
  // The old filter tested `a.energy === energy` with no untagged chip, so
  // a bullet with no suffix vanished the moment you touched the filter —
  // and nothing said how much had gone.
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });

  fireEvent.click(screen.getByRole("button", { name: /^untagged/ }));

  expect(screen.getByText("Chase the supplier")).toBeDefined();
  expect(screen.queryByText("Draft methods")).toBeNull();
});

test("every chip carries its count, so a filter is never a leap", async () => {
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });

  // Counts ride the accessible name too — a number only a sighted reader
  // gets is the same silent drop in another form.
  expect(screen.getByRole("button", { name: "deep, 1" })).toBeDefined();
  expect(screen.getByRole("button", { name: "light, 2" })).toBeDefined();
  expect(screen.getByRole("button", { name: "untagged, 1" })).toBeDefined();
  expect(screen.getByRole("button", { name: "work, 3" })).toBeDefined();
  expect(screen.getByRole("button", { name: "family, 1" })).toBeDefined();
});

test("a filter says how much it is hiding, and offers a way back", async () => {
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });
  expect(screen.queryByText(/Showing/)).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "deep, 1" }));
  expect(screen.getByText("Showing 1 of 4.")).toBeDefined();

  fireEvent.click(screen.getByRole("button", { name: "Clear" }));
  expect(screen.queryByText(/Showing/)).toBeNull();
  expect(screen.getByText("Water plants")).toBeDefined();
});

test("the context and text filters compose with the energy chips", async () => {
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });

  fireEvent.click(screen.getByRole("button", { name: "work, 3" }));
  expect(screen.queryByText("Water plants")).toBeNull();

  fireEvent.change(screen.getByLabelText("Filter actions by text"), {
    target: { value: "receipts" },
  });
  expect(screen.getByText("File receipts")).toBeDefined();
  expect(screen.queryByText("Draft methods")).toBeNull();
  expect(screen.getByText("Showing 1 of 4.")).toBeDefined();

  // And a chip that now matches nothing still says so rather than going.
  expect(screen.getByRole("button", { name: "deep, 0" })).toBeDefined();
});

test("a filter matching nothing says so, and says what is there", async () => {
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });

  fireEvent.change(screen.getByLabelText("Filter actions by text"), {
    target: { value: "nothing like this" },
  });

  expect(screen.getByText(/Nothing matches that/)).toBeDefined();
  expect(screen.getByText(/4 open in all/)).toBeDefined();
});

test("projects are ordered by name, not by whatever the index yielded", async () => {
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });

  const headings = screen.getAllByRole("region").map((r) => r.getAttribute("aria-label"));
  expect(headings).toEqual(["alpha", "beta"]);
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

test("done fires complete_action with the bullet verbatim", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  renderActions((cmd, args) => calls.push({ cmd, args }));
  fireEvent.click(await screen.findByRole("button", { name: /Mark done: Draft methods/ }));

  expect(await screen.findByText(/one step further on alpha/)).toBeDefined();
  const done = calls.find((c) => c.cmd === "complete_action");
  expect(done?.args).toMatchObject({ project: "alpha", action: "Draft methods (deep)" });
});

test("an empty vault gets a calm empty state", async () => {
  renderActions(undefined, []);
  expect(await screen.findByText(/No open actions anywhere/)).toBeDefined();
});

test("has no axe violations", async () => {
  const { container } = renderActions();
  await screen.findByRole("navigation", { name: "Projects" });
  expect(
    await axe(container, { rules: { "color-contrast": { enabled: false } } }),
  ).toHaveNoViolations();
});
