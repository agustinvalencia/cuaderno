// Config editor (#365, PR3): the view loads config.toml into an editable
// textarea (draft/baseline dirty model), validates on demand + debounced,
// and Save runs the backend save_config gate. These cover the wire-up:
// Save fires save_config with the edited content and current hash; a
// validation rejection surfaces inline; a compare-and-swap conflict
// surfaces the distinct reload notice; plus a vitest-axe smoke.
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
const CONFIG_HASH = "deadbeefdeadbeef";

/** How save_config should answer: resolve with a new document, or reject
 * with a tagged ConfigSaveError (validation / conflict / internal). */
type SaveOutcome =
  | { ok: true; content: string; hash: string }
  | { ok: false; error: unknown };

/** How validate_config should answer for the debounced/Check dry-run. */
type ValidateOutcome = { ok: true } | { ok: false; error: unknown };

/** Install a mockIPC recording every call. `save`/`validate` decide how
 * those commands answer; read_config always serves the baseline. */
function installMock(
  calls: Array<{ cmd: string; args: unknown }>,
  opts: { save?: SaveOutcome; validate?: ValidateOutcome; newDraft?: string } = {},
) {
  const validate = opts.validate ?? { ok: true };
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    switch (cmd) {
      case "read_config":
        return { content: CONFIG_TOML, hash: CONFIG_HASH };
      case "read_config_model":
      case "parse_config_model":
        // The structured panel's read; only hit once the Form toggle is
        // active. `parse_config_model` is the editable form's draft-parse
        // seam (PR5b). A minimal empty model is enough for the toggle test.
        return {
          vault: { name: "Test", max_active_projects: 5 },
          note_types: [],
          schemas: [],
          variables: { static_vars: [], prompt: [] },
        };
      case "config_set_note_type":
      case "config_remove_note_type":
      case "config_set_schema_field":
      case "config_remove_schema_field":
        // Every surgical form edit returns the new draft string; the form
        // feeds it back into the shared draft (PR5b).
        return opts.newDraft ?? CONFIG_TOML;
      case "validate_config":
        if (validate.ok) return undefined;
        throw validate.error;
      case "save_config":
        if (opts.save === undefined || opts.save.ok) {
          const s = opts.save;
          return { content: s?.content ?? CONFIG_TOML, hash: s?.hash ?? CONFIG_HASH };
        }
        throw opts.save.error;
      default:
        return undefined;
    }
  });
}

/** The last-rendered view's query client, so a test can force the
 * refetch an external config edit would cause. */
let lastClient: QueryClient;

function renderView() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  lastClient = client;
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

/** The editor's textarea right now, for re-reading after a refetch. */
function findEditorSync(): HTMLTextAreaElement {
  return screen.getByLabelText("config.toml content") as HTMLTextAreaElement;
}

/** The editor's textarea, once the config has loaded. */
async function findEditor(): Promise<HTMLTextAreaElement> {
  return (await screen.findByLabelText("config.toml content")) as HTMLTextAreaElement;
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("loads the config into an editable textarea", async () => {
  installMock([]);
  renderView();

  const editor = await findEditor();
  expect(editor.value).toContain("max_active_projects = 5");
  // Save is disabled until the draft diverges from the baseline.
  expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
});

test("editing enables Save and fires save_config with the content and hash", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  const edited = CONFIG_TOML + '[note_types.widget]\nfolder = "widgets"\n';
  installMock(calls, { save: { ok: true, content: edited, hash: "feedface1234abcd" } });
  renderView();

  const editor = await findEditor();
  fireEvent.change(editor, { target: { value: edited } });

  const saveButton = screen.getByRole("button", { name: "Save" }) as HTMLButtonElement;
  expect(saveButton.disabled).toBe(false);
  fireEvent.click(saveButton);

  await waitFor(() => {
    const saved = calls.find((c) => c.cmd === "save_config");
    // The edited buffer and the current on-disk hash ride the wire (the
    // camelCase `expectedHash` seam).
    expect(saved?.args).toMatchObject({ content: edited, expectedHash: CONFIG_HASH });
  });
});

test("a validation rejection on save surfaces inline", async () => {
  const edited = CONFIG_TOML + "[note_types.Project]\n";
  installMock([], {
    save: {
      ok: false,
      error: { kind: "validation", data: { message: "reserved type name", line: null, col: null } },
    },
  });
  renderView();

  const editor = await findEditor();
  fireEvent.change(editor, { target: { value: edited } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() => {
    const status = screen.getAllByRole("status").find((n) => n.textContent?.includes("not valid"));
    expect(status?.textContent).toContain("reserved type name");
  });
});

test("a compare-and-swap conflict surfaces the reload notice", async () => {
  const edited = CONFIG_TOML + "# a local edit\n";
  installMock([], { save: { ok: false, error: { kind: "conflict" } } });
  renderView();

  const editor = await findEditor();
  fireEvent.change(editor, { target: { value: edited } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() => {
    const status = screen
      .getAllByRole("status")
      .find((n) => n.textContent?.includes("changed on disk"));
    expect(status).toBeDefined();
  });
  // The recovery affordance is offered.
  expect(screen.getByRole("button", { name: "Reload" })).toBeDefined();
});

test("Check dry-runs validate_config against the current draft", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls, { validate: { ok: false, error: { message: "expected `=`", line: 2, col: 5 } } });
  renderView();

  await findEditor();
  fireEvent.click(screen.getByRole("button", { name: "Check" }));

  await waitFor(() => {
    const status = screen.getAllByRole("status").find((n) => n.textContent?.includes("not valid"));
    expect(status?.textContent).toContain("expected `=`");
    expect(status?.textContent).toContain("line 2");
    expect(status?.textContent).toContain("column 5");
  });
});

test("toggling to Form shows the structured panel; Raw restores the textarea", async () => {
  installMock([]);
  renderView();
  // Raw is the default — the textarea is present.
  await findEditor();

  fireEvent.click(screen.getByRole("button", { name: "Form" }));
  // The structured panel's sections appear; the raw textarea is gone.
  await screen.findByRole("heading", { name: "Note types" });
  expect(screen.queryByLabelText("config.toml content")).toBeNull();

  // Back to Raw restores the editor.
  fireEvent.click(screen.getByRole("button", { name: "Raw" }));
  await findEditor();
});

test("a form edit flows through the shared draft into save_config", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  const newDraft = CONFIG_TOML + '[note_types.person]\nfolder = "people"\n';
  installMock(calls, { newDraft, save: { ok: true, content: newDraft, hash: "feedface1234abcd" } });
  renderView();

  // Land on Raw, then switch to the editable Form.
  await findEditor();
  fireEvent.click(screen.getByRole("button", { name: "Form" }));
  const form = within(await screen.findByRole("form", { name: "Add a note type" }));

  // Add a note type: the surgical command returns the new draft, which the
  // shared hook adopts — dirtying the draft and enabling Save.
  fireEvent.change(form.getByLabelText("Name"), { target: { value: "person" } });
  fireEvent.change(form.getByLabelText("Folder"), { target: { value: "people" } });
  fireEvent.click(form.getByRole("button", { name: "Add note type" }));

  const saveButton = screen.getByRole("button", { name: "Save" }) as HTMLButtonElement;
  await waitFor(() => expect(saveButton.disabled).toBe(false));
  fireEvent.click(saveButton);

  await waitFor(() => {
    const saved = calls.find((c) => c.cmd === "save_config");
    // Save persists the surgically-produced draft (not a client
    // re-serialise) against the original on-disk hash.
    expect(saved?.args).toMatchObject({ content: newDraft, expectedHash: CONFIG_HASH });
  });
});

test("has no axe violations", async () => {
  installMock([]);
  const { container } = renderView();
  await findEditor();
  expect(await axe(container, AXE_OPTIONS)).toHaveNoViolations();
});

test("an external config change is adopted while the editor is clean", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  let onDisk = { content: CONFIG_TOML, hash: CONFIG_HASH };
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "read_config") return onDisk;
    return undefined;
  });
  renderView();
  const editor = await findEditor();
  expect(editor.value).toBe(CONFIG_TOML);

  // Someone edits config.toml in nvim; the watcher's `config` area
  // invalidates the read.
  onDisk = { content: '[vault]\nname = "Renamed"\n', hash: "0123456789abcdef" };
  await lastClient.invalidateQueries({ queryKey: ["read_config"] });

  await waitFor(() => expect(findEditorSync().value).toContain("Renamed"));
});

test("an external config change does NOT clobber an unsaved draft", async () => {
  // The view used to be keyed by the on-disk hash, so a background
  // refetch re-seeded it and the draft vanished — with the dialog now
  // promising a draft is safe, that is the guard defeated by the watcher.
  // The dirty case has its own recovery: the save's compare-and-swap
  // refuses to clobber the newer file and offers a reload.
  const calls: Array<{ cmd: string; args: unknown }> = [];
  let onDisk = { content: CONFIG_TOML, hash: CONFIG_HASH };
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "read_config") return onDisk;
    return undefined;
  });
  renderView();
  const editor = await findEditor();
  fireEvent.change(editor, { target: { value: '[vault]\nname = "Mine"\n' } });

  onDisk = { content: '[vault]\nname = "Theirs"\n', hash: "0123456789abcdef" };
  await lastClient.invalidateQueries({ queryKey: ["read_config"] });

  // Give the refetch every chance to land before asserting it did nothing.
  await waitFor(() => expect(calls.filter((c) => c.cmd === "read_config").length).toBeGreaterThan(1));
  expect(findEditorSync().value).toContain("Mine");
});
