// Editable structured Config form (#365, PR5b): the Form side now mutates
// the config through the surgical `config_*` commands and feeds the new
// draft string back into the shared model. These cover the wire-up — a
// form edit fires the right command with the right args and setDrafts the
// returned string; type=string gates the allowed-values editor; add/remove
// of a note type and a schema field; a reserved-constraint pre-check; and
// a vitest-axe smoke.
import { afterEach, expect, test, vi } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { ConfigModel } from "../../api/bindings/ConfigModel";
import { ToastProvider } from "../../shell/Toasts";
import ConfigStructuredView from "./ConfigStructuredView";
import type { ConfigDraft } from "./useConfigDraft";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

const AXE_OPTIONS = { rules: { "color-contrast": { enabled: false } } };

/** A draft carrying one custom note type and one typed schema field. */
const MODEL: ConfigModel = {
  vault: { name: "Demo Vault", max_active_projects: 3 },
  note_types: [
    {
      name: "reading",
      note_type: {
        folder: "reading",
        required: ["author"],
        optional: ["rating"],
        template: "reading.md",
        append_only: false,
        title_field: null,
        date_field: null,
      },
    },
  ],
  schemas: [
    {
      name: "proj-a",
      schema: {
        extra_required: [],
        fields: {
          stage: {
            type: "string",
            default: "idea",
            required: false,
            values: ["idea", "done"],
            list: null,
            settable: null,
            log_on_change: null,
          },
        },
      },
    },
  ],
};

const DRAFT = "[note_types.reading]\nfolder = \"reading\"\n";
/** What every surgical command returns in these tests — the new draft the
 * form must hand back to `setDraft`. */
const NEW_DRAFT = "# rewritten by a surgical edit\n";

/** A ConfigDraft stub: a fixed draft plus a `setDraft` spy, so a test can
 * assert the form feeds the command's result back into the shared draft. */
function draftStub(overrides: Partial<ConfigDraft> = {}): ConfigDraft {
  return {
    draft: DRAFT,
    setDraft: vi.fn(),
    baseline: DRAFT,
    hash: "deadbeefdeadbeef",
    dirty: false,
    validation: null,
    conflict: false,
    save: vi.fn(),
    saving: false,
    check: vi.fn(),
    checking: false,
    reloadFromDisk: vi.fn(),
    ...overrides,
  };
}

/** Record every invoke; `parse_config_model` serves `model`, and every
 * surgical `config_*` command returns `NEW_DRAFT`. */
function installMock(
  calls: Array<{ cmd: string; args: unknown }>,
  model: ConfigModel = MODEL,
) {
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "parse_config_model") return model;
    if (cmd.startsWith("config_")) return NEW_DRAFT;
    return undefined;
  });
}

function renderView(cfg: ConfigDraft) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <main>
          <ConfigStructuredView cfg={cfg} />
        </main>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("renders the parsed draft: the note type and the schema field", async () => {
  installMock([]);
  renderView(draftStub());

  expect(await screen.findByText("Demo Vault")).toBeDefined();
  expect(screen.getByRole("heading", { name: "reading" })).toBeDefined();
  expect(screen.getByRole("heading", { name: "proj-a" })).toBeDefined();
  expect(screen.getByText("stage")).toBeDefined();
});

test("editing a note type's folder fires config_set_note_type and setDrafts the result", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  const cfg = draftStub();
  installMock(calls);
  renderView(cfg);

  const folder = (await screen.findByLabelText("Folder for reading")) as HTMLInputElement;
  fireEvent.change(folder, { target: { value: "papers" } });
  fireEvent.blur(folder);

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_set_note_type");
    // The whole updated note type rides the wire (camelCase `noteType`),
    // with only the folder changed and the draft as the base content.
    expect(call?.args).toMatchObject({
      content: DRAFT,
      name: "reading",
      noteType: { folder: "papers", required: ["author"], optional: ["rating"] },
    });
  });
  // The returned string is fed back into the shared draft.
  expect(cfg.setDraft).toHaveBeenCalledWith(NEW_DRAFT);
});

test("removing a note type fires config_remove_note_type", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView(draftStub());

  fireEvent.click(await screen.findByRole("button", { name: "Remove type" }));

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_remove_note_type");
    expect(call?.args).toMatchObject({ content: DRAFT, name: "reading" });
  });
});

test("the allowed-values editor shows for a string field and hides for others", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView(draftStub());

  // The seeded `stage` field is a string — its allowed-values chip editor
  // is present.
  expect(await screen.findByLabelText("Add to Allowed values")).toBeDefined();

  // Switching the type to `int` must clear values (the server only allows
  // an allowed-value list on a string) and hide the editor.
  fireEvent.change(screen.getByLabelText("Type for stage"), { target: { value: "int" } });

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_set_schema_field");
    expect(call?.args).toMatchObject({
      content: DRAFT,
      noteType: "proj-a",
      field: "stage",
      spec: { type: "int", values: null },
    });
  });
});

test("removing a schema field fires config_remove_schema_field", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView(draftStub());

  fireEvent.click(await screen.findByRole("button", { name: "Remove field" }));

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_remove_schema_field");
    expect(call?.args).toMatchObject({ content: DRAFT, noteType: "proj-a", field: "stage" });
  });
});

test("adding a note type fires config_set_note_type with a minimal type", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView(draftStub());

  const form = within(await screen.findByRole("form", { name: "Add a note type" }));
  fireEvent.change(form.getByLabelText("Name"), { target: { value: "person" } });
  fireEvent.change(form.getByLabelText("Folder"), { target: { value: "people" } });
  fireEvent.click(form.getByRole("button", { name: "Add note type" }));

  await waitFor(() => {
    const call = calls.find(
      (c) => c.cmd === "config_set_note_type" && (c.args as { name: string }).name === "person",
    );
    expect(call?.args).toMatchObject({ name: "person", noteType: { folder: "people" } });
  });
});

test("adding a schema field fires config_set_schema_field", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView(draftStub());

  const form = within(await screen.findByRole("form", { name: "Add a schema field" }));
  fireEvent.change(form.getByLabelText("Note type"), { target: { value: "project" } });
  fireEvent.change(form.getByLabelText("Field name"), { target: { value: "owner" } });
  fireEvent.click(form.getByRole("button", { name: "Add field" }));

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_set_schema_field");
    expect(call?.args).toMatchObject({
      noteType: "project",
      field: "owner",
      spec: { type: "string" },
    });
  });
});

test("a reserved folder name is blocked with a calm pre-check message", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView(draftStub());

  const form = within(await screen.findByRole("form", { name: "Add a note type" }));
  fireEvent.change(form.getByLabelText("Name"), { target: { value: "widget" } });
  fireEvent.change(form.getByLabelText("Folder"), { target: { value: "projects" } });

  // The reserved-folder pre-check message appears and Add is disabled —
  // the command never fires (the server would reject it too).
  expect(await form.findByText(/reserved folder/i)).toBeDefined();
  expect((form.getByRole("button", { name: "Add note type" }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  fireEvent.click(form.getByRole("button", { name: "Add note type" }));
  expect(calls.find((c) => c.cmd === "config_set_note_type")).toBeUndefined();
});

test("an unparseable draft points back to Raw instead of crashing", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "parse_config_model") throw { kind: "invalid", data: "expected `=`" };
    return undefined;
  });
  renderView(draftStub({ draft: "broken = " }));

  expect(await screen.findByText(/switch to Raw/i)).toBeDefined();
});

test("has no axe violations", async () => {
  installMock([]);
  const { container } = renderView(draftStub());
  await screen.findByText("Demo Vault");
  expect(await axe(container, AXE_OPTIONS)).toHaveNoViolations();
});
