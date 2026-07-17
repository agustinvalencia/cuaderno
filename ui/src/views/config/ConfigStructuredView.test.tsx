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
import type { FieldSpec } from "../../api/bindings/FieldSpec";
import type { NamedValue } from "../../api/bindings/NamedValue";
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
  variables: { static_vars: [], prompt: [] },
};

/** MODEL with the `stage` field's spec overridden — for the reserved-key
 * toggles (#375), which the seeded MODEL leaves unset. */
function modelWithStage(spec: Partial<FieldSpec>): ConfigModel {
  const base: FieldSpec = {
    type: "string",
    default: "idea",
    required: false,
    values: ["idea", "done"],
    list: null,
    settable: null,
    log_on_change: null,
  };
  return {
    ...MODEL,
    schemas: [
      {
        name: "proj-a",
        schema: { extra_required: [], fields: { stage: { ...base, ...spec } } },
      },
    ],
  };
}

/** MODEL with the `[variables]` block populated — for the variables editor
 * (#376), which the seeded MODEL leaves empty. */
function modelWithVariables(staticVars: NamedValue[], prompt: NamedValue[]): ConfigModel {
  return { ...MODEL, variables: { static_vars: staticVars, prompt } };
}

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

test("the parsed model fills the form inputs (folder, type)", async () => {
  installMock([]);
  renderView(draftStub());

  const folder = (await screen.findByLabelText("Folder for reading")) as HTMLInputElement;
  expect(folder.value).toBe("reading");
  const type = screen.getByLabelText("Type for stage") as HTMLSelectElement;
  expect(type.value).toBe("string");
});

test("a schema declaring only extra_required (no fields) is hidden", async () => {
  // The field editor has nothing to show for an extra_required-only schema,
  // so it is filtered out and the empty state stands — the guarantee the
  // read-only PR5a shipped, still held now the view is editable.
  const model: ConfigModel = {
    vault: { name: "Demo Vault", max_active_projects: 3 },
    note_types: [],
    schemas: [{ name: "proj-a", schema: { extra_required: ["author"], fields: {} } }],
    variables: { static_vars: [], prompt: [] },
  };
  installMock([], model);
  renderView(draftStub());

  expect(await screen.findByText("No schema field definitions.")).toBeDefined();
  expect(screen.queryByRole("heading", { name: "proj-a" })).toBeNull();
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

test("rapid successive edits serialise: the second builds on the first's result", async () => {
  // Each surgical command echoes the content it was HANDED plus a marker,
  // so the chain is observable: a correctly serialised second edit is handed
  // the first edit's OUTPUT (not the stale starting draft). The old
  // read-cfg.draft-at-call-time code handed both edits the same base and
  // dropped the first.
  const seen: string[] = [];
  let n = 0;
  mockIPC((cmd, args: unknown) => {
    if (cmd === "parse_config_model") return MODEL;
    if (cmd.startsWith("config_")) {
      const content = (args as { content: string }).content;
      seen.push(content);
      n += 1;
      return `${content}#${n}`;
    }
    return undefined;
  });
  const cfg = draftStub();
  renderView(cfg);

  // Two instant-commit edits fired back-to-back, before the first command
  // resolves: toggle append-only (a note-type edit), then flip the field
  // type (a schema edit).
  fireEvent.click(await screen.findByLabelText("Append-only"));
  fireEvent.change(screen.getByLabelText("Type for stage"), { target: { value: "int" } });

  await waitFor(() => expect(seen.length).toBe(2));
  // The load-bearing assertion: the second command was handed the FIRST
  // command's result, proving the queue threaded the accumulated draft.
  expect(seen[0]).toBe(DRAFT);
  expect(seen[1]).toBe(`${DRAFT}#1`);
  // And the final draft carries BOTH edits, in order.
  expect(cfg.setDraft).toHaveBeenLastCalledWith(`${DRAFT}#1#2`);
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
    // Switching type clears BOTH the values list and the now-type-mismatched
    // default, so no stale value reaches (and is rejected by) the server.
    expect(call?.args).toMatchObject({
      content: DRAFT,
      noteType: "proj-a",
      field: "stage",
      spec: { type: "int", values: null, default: null },
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

test("toggling Settable on fires config_set_schema_field with settable true", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls); // seeded stage.settable = null → unchecked
  renderView(draftStub());

  fireEvent.click(await screen.findByLabelText("Settable"));

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_set_schema_field");
    expect(call?.args).toMatchObject({
      content: DRAFT,
      noteType: "proj-a",
      field: "stage",
      spec: { settable: true },
    });
  });
});

test("Log changes to daily is disabled for a non-settable field", async () => {
  installMock([]); // seeded stage.settable = null
  renderView(draftStub());

  const log = (await screen.findByLabelText("Log changes to daily")) as HTMLInputElement;
  expect(log.disabled).toBe(true);
});

test("Log changes to daily is enabled once the field is settable", async () => {
  installMock([], modelWithStage({ settable: true }));
  renderView(draftStub());

  const log = (await screen.findByLabelText("Log changes to daily")) as HTMLInputElement;
  expect(log.disabled).toBe(false);
});

test("toggling Log changes to daily on writes log_on_change true", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls, modelWithStage({ settable: true }));
  renderView(draftStub());

  fireEvent.click(await screen.findByLabelText("Log changes to daily"));

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_set_schema_field");
    expect(call?.args).toMatchObject({ field: "stage", spec: { log_on_change: true } });
  });
});

test("turning Settable off clears log_on_change", async () => {
  // `log_on_change` only fires on a settable field, so dropping settable must
  // clear it — the config never keeps an inert log_on_change on a locked field.
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls, modelWithStage({ settable: true, log_on_change: true }));
  renderView(draftStub());

  fireEvent.click(await screen.findByLabelText("Settable")); // currently on → turn off

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_set_schema_field");
    expect(call?.args).toMatchObject({
      field: "stage",
      spec: { settable: null, log_on_change: null },
    });
  });
});

test("unchecking Log changes to daily writes log_on_change null", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls, modelWithStage({ settable: true, log_on_change: true }));
  renderView(draftStub());

  fireEvent.click(await screen.findByLabelText("Log changes to daily")); // on → off

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_set_schema_field");
    expect(call?.args).toMatchObject({ field: "stage", spec: { log_on_change: null } });
  });
});

test("toggling Settable on preserves an existing log_on_change and leaves list untouched", async () => {
  // A Raw-authored field can carry log_on_change while settable is off; turning
  // settable on must keep that flag (not reset it) and never touch `list`.
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls, modelWithStage({ settable: false, log_on_change: true, list: null }));
  renderView(draftStub());

  fireEvent.click(await screen.findByLabelText("Settable")); // off → on

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_set_schema_field");
    expect(call?.args).toMatchObject({
      field: "stage",
      spec: { settable: true, log_on_change: true, list: null },
    });
  });
});

test("editing an unrelated key preserves a set settable flag in the args", async () => {
  // The persistence design hinges on the form re-sending lifted flags: an edit
  // to a settable field's TYPE must carry settable:true through, so the writer
  // re-emits it rather than the flag vanishing on the next reparse.
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls, modelWithStage({ settable: true }));
  renderView(draftStub());

  fireEvent.change(await screen.findByLabelText("Type for stage"), { target: { value: "int" } });

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_set_schema_field");
    expect(call?.args).toMatchObject({ field: "stage", spec: { type: "int", settable: true } });
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

// --- Template variables editor (#376) ---

test("renders static and prompted variables from the model", async () => {
  installMock(
    [],
    modelWithVariables([{ name: "author", value: "Anon" }], [{ name: "topic", value: "What topic?" }]),
  );
  renderView(draftStub());

  const staticVal = (await screen.findByLabelText("Value for author")) as HTMLInputElement;
  expect(staticVal.value).toBe("Anon");
  const promptVal = screen.getByLabelText("Prompt for topic") as HTMLInputElement;
  expect(promptVal.value).toBe("What topic?");
});

test("adding a static variable fires config_set_variable", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls); // MODEL has an empty variables block
  renderView(draftStub());

  const form = within(await screen.findByRole("form", { name: "Add a static variable" }));
  fireEvent.change(form.getByLabelText("Name"), { target: { value: "author" } });
  fireEvent.change(form.getByLabelText("Value"), { target: { value: "Anon" } });
  fireEvent.click(form.getByRole("button", { name: "Add variable" }));

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_set_variable");
    expect(call?.args).toMatchObject({ content: DRAFT, name: "author", value: "Anon" });
  });
});

test("editing a variable's value fires config_set_variable with the new value", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls, modelWithVariables([{ name: "author", value: "Old" }], []));
  renderView(draftStub());

  const input = await screen.findByLabelText("Value for author");
  fireEvent.change(input, { target: { value: "New" } });
  fireEvent.blur(input);

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_set_variable");
    expect(call?.args).toMatchObject({ name: "author", value: "New" });
  });
});

test("removing a static variable fires config_remove_variable", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls, modelWithVariables([{ name: "author", value: "Anon" }], []));
  renderView(draftStub());

  fireEvent.click(await screen.findByRole("button", { name: "Remove author" }));

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_remove_variable");
    expect(call?.args).toMatchObject({ name: "author" });
  });
});

test("adding a prompted variable fires config_set_prompt_variable", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView(draftStub());

  const form = within(await screen.findByRole("form", { name: "Add a prompted variable" }));
  fireEvent.change(form.getByLabelText("Name"), { target: { value: "topic" } });
  fireEvent.change(form.getByLabelText("Prompt"), { target: { value: "What topic?" } });
  fireEvent.click(form.getByRole("button", { name: "Add variable" }));

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_set_prompt_variable");
    expect(call?.args).toMatchObject({ name: "topic", message: "What topic?" });
  });
});

test("removing a prompted variable fires config_remove_prompt_variable", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls, modelWithVariables([], [{ name: "topic", value: "What topic?" }]));
  renderView(draftStub());

  fireEvent.click(await screen.findByRole("button", { name: "Remove topic" }));

  await waitFor(() => {
    const call = calls.find((c) => c.cmd === "config_remove_prompt_variable");
    expect(call?.args).toMatchObject({ name: "topic" });
  });
});

test("the reserved name 'prompt' is blocked in the static add form", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView(draftStub());

  const form = within(await screen.findByRole("form", { name: "Add a static variable" }));
  fireEvent.change(form.getByLabelText("Name"), { target: { value: "prompt" } });
  fireEvent.change(form.getByLabelText("Value"), { target: { value: "x" } });

  // The pre-check blocks it and Add is disabled — the command never fires
  // (the server would reject it too).
  expect(await form.findByText(/reserved name/i)).toBeDefined();
  expect((form.getByRole("button", { name: "Add variable" }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  fireEvent.click(form.getByRole("button", { name: "Add variable" }));
  expect(calls.find((c) => c.cmd === "config_set_variable")).toBeUndefined();
});

test("has no axe violations", async () => {
  installMock([]);
  const { container } = renderView(draftStub());
  await screen.findByText("Demo Vault");
  expect(await axe(container, AXE_OPTIONS)).toHaveNoViolations();
});
