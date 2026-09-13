// The unplanned-start form's own tests (#568). Home.test.tsx covers it
// in place; these need to re-render the component with a CHANGED project
// list, which the Home-level IPC mock cannot express.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import type { OrientationProject } from "../../api/bindings/OrientationProject";
import { ToastProvider } from "../../shell/Toasts";
import UnplannedStart from "./UnplannedStart";

const ALPHA: OrientationProject = {
  slug: "alpha",
  status: "active",
  state_snippet: "Underway.",
  top_action: { text: "Draft methods", energy: "deep" },
  context: "work",
  actions: [{ text: "Draft methods (deep)", energy: "deep", attached: null }],
};

const BETA: OrientationProject = { ...ALPHA, slug: "beta", top_action: null, actions: [] };

/** The provider-wrapped element, for tests that render siblings too. */
function renderFormInto(projects: OrientationProject[]) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={client}>
      <ToastProvider>
        <UnplannedStart projects={projects} energy={null} />
      </ToastProvider>
    </QueryClientProvider>
  );
}

function renderForm(projects: OrientationProject[]) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <UnplannedStart projects={projects} energy={null} />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("a project that vanishes from the list cannot be submitted behind the user's back", async () => {
  // `projects` comes from get_orientation, which is invalidated while
  // this form can be open — the Now band's Done, this form's own success,
  // any `vault:changed` from the CLI or an agent. A controlled <select>
  // whose value matches no option silently renders the FIRST option, so
  // remembering the raw pick showed one project and submitted another.
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return undefined;
  });

  const { rerender } = renderForm([ALPHA, BETA]);
  fireEvent.click(screen.getByRole("button", { name: /isn't listed/ }));
  const form = screen.getByRole("form", { name: "Start something that isn't listed" });

  fireEvent.change(within(form).getByLabelText("On"), { target: { value: "beta" } });
  fireEvent.change(within(form).getByLabelText("What are you starting?"), {
    target: { value: "Fix the CI badge" },
  });

  // beta is parked/completed elsewhere; the list refetches without it.
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  rerender(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <UnplannedStart projects={[ALPHA]} energy={null} />
      </ToastProvider>
    </QueryClientProvider>,
  );

  const live = screen.getByRole("form", { name: "Start something that isn't listed" });
  const select = within(live).getByLabelText("On") as HTMLSelectElement;
  expect(select.value).toBe("alpha");
  expect(live.textContent).toContain("Adds it to alpha and starts it");

  fireEvent.click(within(live).getByRole("button", { name: "Start" }));
  await waitFor(() => {
    expect(calls.find((c) => c.cmd === "start_unplanned_action")?.args).toMatchObject({
      project: "alpha",
    });
  });
  // What is on screen is what was sent — the whole point.
  expect(calls.find((c) => c.cmd === "start_unplanned_action")?.args).not.toMatchObject({
    project: "beta",
  });
});

test("a late success does not haul focus back from wherever the user moved to", async () => {
  // close() runs from Cancel AND from the mutation's success, and Cancel
  // is not disabled while the request is in flight, so both fire for one
  // submission. Focus must return only if it is still inside the form.
  let resolve: () => void = () => {};
  const gate = new Promise<void>((r) => {
    resolve = r;
  });
  mockIPC(async (cmd) => {
    if (cmd === "start_unplanned_action") await gate;
    return undefined;
  });

  render(
    <div>
      <button data-testid="elsewhere">elsewhere</button>
      {renderFormInto([ALPHA])}
    </div>,
  );
  fireEvent.click(screen.getByRole("button", { name: /isn't listed/ }));
  const form = screen.getByRole("form", { name: "Start something that isn't listed" });
  fireEvent.change(within(form).getByLabelText("What are you starting?"), {
    target: { value: "Fix the CI badge" },
  });
  fireEvent.click(within(form).getByRole("button", { name: "Start" }));

  // The user gives up waiting and goes elsewhere.
  const elsewhere = screen.getByTestId("elsewhere");
  elsewhere.focus();
  resolve();

  await waitFor(() => {
    expect(screen.queryByRole("form", { name: "Start something that isn't listed" })).toBeNull();
  });
  expect(document.activeElement).toBe(elsewhere);
});

test("submitting from the Start button still returns focus, despite the browser's focus fixup", async () => {
  // Browsers implement the HTML focus-fixup rule: when the focused
  // element stops being focusable, focus reverts to <body>. The Start
  // button goes `disabled` the instant the mutation turns pending, so a
  // user who submitted from it is on <body> by the time success resolves
  // — outside the form. jsdom implements NONE of this (it neither
  // focuses on click nor blurs on disable), so the blur below stands in
  // for the browser, and without it this regression is invisible to the
  // suite. That is exactly how it shipped once.
  let resolve: () => void = () => {};
  const gate = new Promise<void>((r) => {
    resolve = r;
  });
  mockIPC(async (cmd) => {
    if (cmd === "start_unplanned_action") await gate;
    return undefined;
  });

  renderForm([ALPHA]);
  fireEvent.click(screen.getByRole("button", { name: /isn't listed/ }));
  const form = screen.getByRole("form", { name: "Start something that isn't listed" });
  fireEvent.change(within(form).getByLabelText("What are you starting?"), {
    target: { value: "Fix the CI badge" },
  });

  const submit = within(form).getByRole("button", { name: "Start" }) as HTMLButtonElement;
  submit.focus();
  fireEvent.click(submit);
  await waitFor(() => expect(submit.disabled).toBe(true));
  // The browser's fixup, by hand. jsdom's blur() is a no-op on an
  // already-disabled element, so re-enable across the blur to move focus
  // the way a real browser does when `disabled` lands on it.
  submit.disabled = false;
  submit.blur();
  submit.disabled = true;
  expect(document.activeElement).toBe(document.body);
  resolve();

  await waitFor(() => {
    expect(screen.queryByRole("form", { name: "Start something that isn't listed" })).toBeNull();
  });
  expect(document.activeElement).toBe(screen.getByRole("button", { name: /isn't listed/ }));
});

test("the toast names the project actually submitted, not one re-derived later", async () => {
  // `selected` is re-derived from the live list every render, and
  // react-query runs onSuccess with the closure from the render at
  // RESOLUTION time. The submission is carried in mutation variables so
  // the message cannot drift from the write.
  let resolve: () => void = () => {};
  const gate = new Promise<void>((r) => {
    resolve = r;
  });
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC(async (cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "start_unplanned_action") await gate;
    return undefined;
  });

  const { rerender } = renderForm([ALPHA, BETA]);
  fireEvent.click(screen.getByRole("button", { name: /isn't listed/ }));
  const form = screen.getByRole("form", { name: "Start something that isn't listed" });
  fireEvent.change(within(form).getByLabelText("On"), { target: { value: "beta" } });
  fireEvent.change(within(form).getByLabelText("What are you starting?"), {
    target: { value: "Fix the CI badge" },
  });
  fireEvent.click(within(form).getByRole("button", { name: "Start" }));
  await waitFor(() => {
    expect(calls.find((c) => c.cmd === "start_unplanned_action")).toBeDefined();
  });

  // beta disappears from the list mid-request.
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  rerender(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <UnplannedStart projects={[ALPHA]} energy={null} />
      </ToastProvider>
    </QueryClientProvider>,
  );
  resolve();

  await waitFor(() => {
    expect(document.body.textContent).toContain("Started on beta");
  });
  expect(document.body.textContent).not.toContain("Started on alpha");
  expect(calls.find((c) => c.cmd === "start_unplanned_action")?.args).toMatchObject({
    project: "beta",
  });
});

test("a failed start leaves the form open with the typed text intact", async () => {
  // The path a user hits when a project is parked from the CLI between
  // the refetch and the submit — losing the text here would be the worst
  // possible moment for it.
  mockIPC((cmd) => {
    if (cmd === "start_unplanned_action") throw new Error("alpha is parked");
    return undefined;
  });

  renderForm([ALPHA]);
  fireEvent.click(screen.getByRole("button", { name: /isn't listed/ }));
  const form = screen.getByRole("form", { name: "Start something that isn't listed" });
  fireEvent.change(within(form).getByLabelText("What are you starting?"), {
    target: { value: "Fix the CI badge" },
  });
  fireEvent.click(within(form).getByRole("button", { name: "Start" }));

  await waitFor(() => {
    expect(document.body.textContent).toContain("alpha is parked");
  });
  const live = screen.getByRole("form", { name: "Start something that isn't listed" });
  expect((within(live).getByLabelText("What are you starting?") as HTMLInputElement).value).toBe(
    "Fix the CI badge",
  );
});

test("closing the form returns focus to the trigger that opened it", async () => {
  mockIPC(() => undefined);
  renderForm([ALPHA]);

  const trigger = screen.getByRole("button", { name: /isn't listed/ });
  expect(trigger.getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(trigger);

  const form = screen.getByRole("form", { name: "Start something that isn't listed" });
  fireEvent.click(within(form).getByRole("button", { name: "Cancel" }));

  // Without the focus return this lands on document.body, stranding
  // keyboard users at the top of the page.
  await waitFor(() => {
    expect(document.activeElement).toBe(screen.getByRole("button", { name: /isn't listed/ }));
  });
});
