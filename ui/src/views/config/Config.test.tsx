// Config inspector (#365, PR1): the view renders the raw config.toml
// content read-only, and the Check button dry-runs validate_config —
// surfacing a calm OK for a valid config and the backend error (with
// line/col) for an invalid one.
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ToastProvider } from "../../shell/Toasts";
import Config from "./Config";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

const AXE_OPTIONS = { rules: { "color-contrast": { enabled: false } } };

const CONFIG_TOML = '[vault]\nname = "Test"\nmax_active_projects = 5\n';

/** A validate_config outcome the mock should emulate: `ok` resolves the
 * command, `error` makes it reject with the serialised `{ message, line,
 * col }` the backend returns for an invalid config. */
type ValidateOutcome = { ok: true } | { ok: false; message: string; line: number | null; col: number | null };

/** Install a mockIPC recording every call. `validate` decides how the
 * validate_config command answers. */
function installMock(
  calls: Array<{ cmd: string; args: unknown }>,
  validate: ValidateOutcome = { ok: true },
) {
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    switch (cmd) {
      case "read_config":
        return { content: CONFIG_TOML, hash: "deadbeefdeadbeef" };
      case "validate_config":
        if (validate.ok) return undefined;
        // Reject exactly as the backend does — a plain object, no `kind`.
        throw { message: validate.message, line: validate.line, col: validate.col };
      default:
        return undefined;
    }
  });
}

function renderView() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter>
          <main>
            <Config />
          </main>
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("renders the raw config content read-only", async () => {
  installMock([]);
  renderView();

  // The config.toml content appears verbatim in the viewer.
  const pane = await screen.findByText(/max_active_projects = 5/);
  expect(pane).toBeDefined();
  // Read-only: there is no editable textbox in PR1.
  expect(screen.queryByRole("textbox")).toBeNull();
});

test("Check calls validate_config against the current content", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView();

  await screen.findByText(/max_active_projects = 5/);
  fireEvent.click(screen.getByRole("button", { name: "Check" }));

  await waitFor(() => {
    const validated = calls.find((c) => c.cmd === "validate_config");
    expect(validated?.args).toMatchObject({ content: CONFIG_TOML });
  });
});

test("a valid config surfaces a calm OK", async () => {
  installMock([], { ok: true });
  renderView();

  await screen.findByText(/max_active_projects = 5/);
  fireEvent.click(screen.getByRole("button", { name: "Check" }));

  const status = await screen.findByRole("status");
  expect(status.textContent).toContain("valid");
});

test("an invalid config surfaces the backend error with line/col", async () => {
  installMock([], { ok: false, message: "expected `=`", line: 2, col: 5 });
  renderView();

  await screen.findByText(/max_active_projects = 5/);
  fireEvent.click(screen.getByRole("button", { name: "Check" }));

  const status = await screen.findByRole("status");
  expect(status.textContent).toContain("not valid");
  expect(status.textContent).toContain("expected `=`");
  expect(status.textContent).toContain("line 2");
  expect(status.textContent).toContain("column 5");
});

test("has no axe violations", async () => {
  installMock([]);
  const { container } = renderView();
  await screen.findByText(/max_active_projects = 5/);
  expect(await axe(container, AXE_OPTIONS)).toHaveNoViolations();
});
