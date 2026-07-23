// Actions view (#445): a project rail with counts, a sticky filter bar
// whose chips carry their own counts — including the "untagged" bucket
// the old filter silently dropped — a text filter, and one shared
// ambiguity picker rather than one per row.
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

// jsdom lacks the APIs ClampedText's overflow measurement reaches for.
globalThis.ResizeObserver ||= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;

// `beta` sorts after `alpha`, and is declared first, so the group order
// also proves the sort rather than echoing the backend's order.
const GROUPS: ProjectActions[] = [
  {
    slug: "beta",
    context: "family",
    actions: [
      // A real attached bullet carries the wikilink the backend matched
      // on, and no energy suffix — the shape `list_actions` actually
      // produces (crates/cdno-domain/src/vault/projects/actions.rs).
      {
        text: "Water plants [[actions/water-plants]]",
        energy: null,
        attached: { slug: "water-plants", status: "active" },
      },
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
  return mount();
}

/** For the cases that need a mock which can throw — the write error
 * paths, which a return-only mock cannot reach at all. */
function renderWith(handler: (cmd: string, args: unknown) => unknown) {
  mockIPC(handler);
  return mount();
}

function mount() {
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
  expect(screen.getByRole("button", { name: "light, 1" })).toBeDefined();
  expect(screen.getByRole("button", { name: "untagged, 2" })).toBeDefined();
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
  expect(screen.getByText(/Water plants/)).toBeDefined();
});

test("Clear resets every dimension, not just the one last touched", async () => {
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });

  fireEvent.click(rail().getByRole("button", { name: /alpha/ }));
  fireEvent.click(screen.getByRole("button", { name: /^work/ }));
  fireEvent.click(screen.getByRole("button", { name: /^deep/ }));
  fireEvent.change(screen.getByLabelText("Filter actions by text"), {
    target: { value: "methods" },
  });
  expect(screen.getByText("Showing 1 of 4.")).toBeDefined();

  fireEvent.click(screen.getByRole("button", { name: "Clear" }));

  expect(screen.queryByText(/Showing/)).toBeNull();
  expect(screen.getByRole("region", { name: "beta" })).toBeDefined();
  expect((screen.getByLabelText("Filter actions by text") as HTMLInputElement).value).toBe("");
  expect(rail().getByRole("button", { name: /All actions/ }).getAttribute("aria-current")).toBe(
    "true",
  );
});

test("the rail counts follow the other filters, so a project never promises work it cannot show", async () => {
  // Unfiltered, a rail count is trivially the group's length. The claim is
  // the cross-dimension one: after choosing "deep", beta holds none, and
  // saying otherwise would send you to "Nothing matches that".
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });

  fireEvent.click(screen.getByRole("button", { name: /^deep/ }));

  expect(rail().getByRole("button", { name: /All actions/ }).textContent).toContain("1");
  expect(rail().getByRole("button", { name: /alpha/ }).textContent).toContain("1");
  expect(rail().getByRole("button", { name: /beta/ }).textContent).toContain("0");
});

test("only the contexts actually present get a chip", async () => {
  // Seven chips of which five read zero is a wall, not a filter.
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });

  const chips = within(screen.getByRole("group", { name: "Filter by context" }));
  expect(chips.getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual([
    "work, 3",
    "family, 1",
  ]);
});

test("the selected project is marked, not only tinted", async () => {
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });
  expect(rail().getByRole("button", { name: /alpha/ }).getAttribute("aria-current")).toBeNull();

  fireEvent.click(rail().getByRole("button", { name: /alpha/ }));

  expect(rail().getByRole("button", { name: /alpha/ }).getAttribute("aria-current")).toBe("true");
});

test("the context and text filters compose with the energy chips", async () => {
  renderActions();
  await screen.findByRole("navigation", { name: "Projects" });

  fireEvent.click(screen.getByRole("button", { name: "work, 3" }));
  expect(screen.queryByText(/Water plants/)).toBeNull();

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
  // And the rail agrees, which is the reason the sort was hoisted above
  // both of them rather than done in the list.
  expect(rail().getAllByRole("button").map((b) => b.textContent?.replace(/\d+$/, ""))).toEqual([
    "All actions",
    "alpha",
    "beta",
  ]);
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

test("an ambiguous complete opens the one picker; choosing re-invokes with the exact text", async () => {
  // The picker moved from one-per-row to one for the whole list, so this
  // is the path that proves the move landed. Without the mount, `handle()`
  // still returns true and suppresses the toast — an ambiguous write
  // becomes a completely silent no-op.
  const calls: Array<{ cmd: string; args: unknown }> = [];
  let attempts = 0;
  renderWith((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_all_actions") return GROUPS;
    if (cmd === "complete_action") {
      attempts += 1;
      if (attempts === 1) {
        throw {
          kind: "ambiguous",
          data: {
            query: "Draft methods (deep)",
            candidates: ["Draft methods for section 1", "Draft methods for section 2"],
          },
        };
      }
      return undefined;
    }
    return undefined;
  });

  fireEvent.click(await screen.findByRole("button", { name: /Mark done: Draft methods/ }));
  fireEvent.click(await screen.findByText("Draft methods for section 2"));

  await waitFor(() =>
    expect(calls.filter((c) => c.cmd === "complete_action")).toHaveLength(2),
  );
  expect(calls.filter((c) => c.cmd === "complete_action")[1].args).toMatchObject({
    project: "alpha",
    action: "Draft methods for section 2",
  });
});

test("a failed complete puts the row back, and says why", async () => {
  // Done removes the row optimistically, so a rejected write that did not
  // roll back would leave the action gone from the screen and present in
  // the vault — the worst of both.
  // The refetch `onSettled` fires would put the row back on its own, so
  // it is held open: what restores the row here can only be the rollback.
  let reads = 0;
  renderWith((cmd) => {
    if (cmd === "list_all_actions") {
      reads += 1;
      return reads === 1 ? GROUPS : new Promise(() => {});
    }
    if (cmd === "complete_action") throw new Error("vault is read-only");
    return undefined;
  });

  fireEvent.click(await screen.findByRole("button", { name: /Mark done: Draft methods/ }));

  expect(await screen.findByText(/read-only/)).toBeDefined();
  expect(reads).toBeGreaterThan(1);
  expect(screen.getByText("Draft methods")).toBeDefined();
});

test("done removes the row before the write settles", async () => {
  // The optimistic patch is what makes the list feel like a list rather
  // than a form. Without it the row lingers until the refetch lands.
  let settle: (() => void) | undefined;
  renderWith((cmd) => {
    if (cmd === "list_all_actions") return GROUPS;
    if (cmd === "complete_action") return new Promise<void>((resolve) => (settle = resolve));
    return undefined;
  });

  fireEvent.click(await screen.findByRole("button", { name: /Mark done: Draft methods/ }));

  await waitFor(() => expect(screen.queryByText("Draft methods")).toBeNull());
  settle?.();
});

test("a write in flight disables its own row, not every row", async () => {
  // One mutation serves the whole list, so a bare `isPending` would grey
  // out every done and promote button on the page for one write.
  let settle: (() => void) | undefined;
  renderWith((cmd) => {
    if (cmd === "list_all_actions") return GROUPS;
    if (cmd === "promote_action") return new Promise<void>((resolve) => (settle = resolve));
    return undefined;
  });

  const first = await screen.findByRole("button", { name: /Promote to a note: Draft methods/ });
  fireEvent.click(first);

  await waitFor(() => expect((first as HTMLButtonElement).disabled).toBe(true));
  const other = screen.getByRole("button", { name: /Promote to a note: File receipts/ });
  expect((other as HTMLButtonElement).disabled).toBe(false);
  settle?.();
});

test("an attached action opens its note", async () => {
  renderActions();
  const note = await screen.findByRole("button", { name: "note" });
  fireEvent.click(note);
  // The reader routes by vault path; a missing `.md` dead-ends it.
  expect(screen.getByRole("region", { name: "beta" })).toBeDefined();
});

test("a long action stays one line until expanded", async () => {
  // jsdom lays nothing out, so the overflow the clamp keys on is stubbed —
  // the same technique clamped-text's own suite uses.
  const LONG = "x".repeat(400);
  Object.defineProperty(HTMLElement.prototype, "scrollHeight", { configurable: true, get: () => 60 });
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 20 });
  try {
    renderActions(undefined, [
      { slug: "alpha", context: "work", actions: [{ text: LONG, energy: null, attached: null }] },
    ]);
    // The toggle appears only because the row is capped; a plain span, or
    // the component's three-line default, would show no toggle at all.
    const more = await screen.findByRole("button", { name: /more/i });
    fireEvent.click(more);
    expect(screen.getByRole("button", { name: /less/i })).toBeDefined();
  } finally {
    // @ts-expect-error restoring the jsdom getters
    delete HTMLElement.prototype.scrollHeight;
    // @ts-expect-error restoring the jsdom getters
    delete HTMLElement.prototype.clientHeight;
  }
});
