// Structured Config view (#365, PR5a): the read-only panel renders the
// parsed config — vault meta, note-type cards, and schema-field tables —
// from a single read_config_model read. These cover a populated model
// (note type + schema field both surface), the empty states, and a
// vitest-axe smoke.
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { ConfigModel } from "../../api/bindings/ConfigModel";
import ConfigStructuredView from "./ConfigStructuredView";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

const AXE_OPTIONS = { rules: { "color-contrast": { enabled: false } } };

/** A model carrying one custom note type and one typed schema field —
 * neutral placeholders only. */
const MODEL_WITH_CONTENT: ConfigModel = {
  vault: { name: "Demo Vault", max_active_projects: 3 },
  note_types: [
    {
      name: "reading",
      note_type: {
        folder: "reading",
        required: ["author"],
        optional: ["rating"],
        template: "reading.md",
        append_only: true,
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
            required: true,
            values: ["idea", "active", "done"],
            list: null,
            settable: null,
            log_on_change: null,
          },
        },
      },
    },
  ],
};

/** An empty model — no custom note types, no schema fields. */
const EMPTY_MODEL: ConfigModel = {
  vault: { name: "Empty Vault", max_active_projects: 5 },
  note_types: [],
  schemas: [],
};

function installMock(model: ConfigModel) {
  mockIPC((cmd) => {
    if (cmd === "read_config_model") return model;
    return undefined;
  });
}

function renderView() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <main>
        <ConfigStructuredView />
      </main>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  clearMocks();
});

test("renders vault meta, the note type, and the schema field", async () => {
  installMock(MODEL_WITH_CONTENT);
  renderView();

  // Vault meta.
  expect(await screen.findByText("Demo Vault")).toBeDefined();
  expect(screen.getByText("3")).toBeDefined();

  // Note type card: name, folder, template, append-only, field chips.
  expect(screen.getByRole("heading", { name: "reading" })).toBeDefined();
  expect(screen.getByText("append-only")).toBeDefined();
  expect(screen.getByText("reading.md")).toBeDefined();
  expect(screen.getByText("author")).toBeDefined();
  expect(screen.getByText("rating")).toBeDefined();

  // Schema field table: name, type, default, required, values.
  expect(screen.getByRole("heading", { name: "proj-a" })).toBeDefined();
  expect(screen.getByText("stage")).toBeDefined();
  expect(screen.getByText("string")).toBeDefined();
  expect(screen.getByText("idea")).toBeDefined();
  expect(screen.getByText("idea, active, done")).toBeDefined();
});

test("renders a dash for an absent default and absent values", async () => {
  // A minimal field: no default, no values — both must render as the
  // muted "—" placeholder, distinct from a present falsey value.
  const model: ConfigModel = {
    vault: { name: "Demo Vault", max_active_projects: 3 },
    note_types: [],
    schemas: [
      {
        name: "proj-a",
        schema: {
          extra_required: [],
          fields: {
            done: {
              type: "bool",
              default: null,
              required: false,
              values: null,
              list: null,
              settable: null,
              log_on_change: null,
            },
          },
        },
      },
    ],
  };
  installMock(model);
  renderView();

  expect(await screen.findByText("done")).toBeDefined();
  // Both the default cell and the values cell fall back to the dash.
  expect(screen.getAllByText("—").length).toBeGreaterThanOrEqual(2);
});

test("hides a schema that declares only extra_required (no typed fields)", async () => {
  // A `[schemas.<type>]` with legacy extra_required but no `fields` block
  // carries nothing the field table can show, so it is filtered out and the
  // empty state stands.
  const model: ConfigModel = {
    vault: { name: "Demo Vault", max_active_projects: 3 },
    note_types: [],
    schemas: [
      { name: "proj-a", schema: { extra_required: ["author"], fields: {} } },
    ],
  };
  installMock(model);
  renderView();

  expect(await screen.findByText("No schema field definitions.")).toBeDefined();
  expect(screen.queryByRole("heading", { name: "proj-a" })).toBeNull();
});

test("shows the empty states when nothing is declared", async () => {
  installMock(EMPTY_MODEL);
  renderView();

  expect(await screen.findByText("No custom note types.")).toBeDefined();
  expect(screen.getByText("No schema field definitions.")).toBeDefined();
});

test("has no axe violations", async () => {
  installMock(MODEL_WITH_CONTENT);
  const { container } = renderView();
  await screen.findByText("Demo Vault");
  expect(await axe(container, AXE_OPTIONS)).toHaveNoViolations();
});
