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
