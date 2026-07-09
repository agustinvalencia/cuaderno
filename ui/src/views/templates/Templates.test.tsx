// Templates view (#357): the list renders each note type with a source
// badge, selecting one reads its effective content, editing + Save fires
// save_template, an unknown {{token}} raises a calm non-blocking notice,
// and a template-less custom type offers Create.
import { afterEach, expect, test } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ToastProvider } from "../../shell/Toasts";
import type { TemplateSummary } from "../../api/bindings/TemplateSummary";
import type { TemplatePlaceholder } from "../../api/bindings/TemplatePlaceholder";
import Templates from "./Templates";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

const AXE_OPTIONS = { rules: { "color-contrast": { enabled: false } } };

// project = built-in with no override (its default is effective);
// daily = a built-in with a custom override; person = a config custom
// type with no template yet (the Create branch). project is first, so it
// is the default selection.
const TEMPLATES: TemplateSummary[] = [
  {
    note_type: "project",
    display_name: "Project",
    is_custom_type: false,
    source: "builtin_default",
    has_custom_file: false,
    path: ".cuaderno/templates/project.md",
  },
  {
    note_type: "daily",
    display_name: "Daily",
    is_custom_type: false,
    source: "custom_base",
    has_custom_file: true,
    path: ".cuaderno/templates/daily.md",
  },
  {
    note_type: "person",
    display_name: "Person",
    is_custom_type: true,
    source: null,
    has_custom_file: false,
    path: ".cuaderno/templates/person.md",
  },
];

const PROJECT_PLACEHOLDERS: TemplatePlaceholder[] = [
  { name: "title", source: { kind: "supplied" } },
  { name: "context", source: { kind: "supplied" } },
  { name: "status", source: { kind: "supplied" } },
];

const PROJECT_CONTENT = "---\ntype: project\ncontext: {{context}}\n---\n\n# {{title}}\n";

/** Install a mockIPC recording every call and returning template
 * fixtures. `onCall` observes writes; `saveResult`/`createResult` let a
 * test stub the two mutations. */
function installMock(
  calls: Array<{ cmd: string; args: unknown }>,
  templates: TemplateSummary[] = TEMPLATES,
) {
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    switch (cmd) {
      case "list_templates":
        return templates;
      case "read_template": {
        const noteType = (args as { noteType: string }).noteType;
        if (noteType === "project") {
          return { content: PROJECT_CONTENT, source: "builtin_default" };
        }
        return { content: "custom body", source: "custom_base" };
      }
      case "list_template_placeholders":
        return PROJECT_PLACEHOLDERS;
      case "save_template":
        return undefined;
      case "create_template":
        return undefined;
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
            <Templates />
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

test("lists every note type with its source badge", async () => {
  installMock([]);
  renderView();

  // Each type appears with its source badge.
  expect(await screen.findByRole("button", { name: /Project/ })).toBeDefined();
  expect(screen.getByRole("button", { name: /Daily.*Custom/ })).toBeDefined();
  expect(screen.getByRole("button", { name: /Person.*No template/ })).toBeDefined();
});

test("selecting a type reads and shows its effective content", async () => {
  installMock([]);
  renderView();

  // project is the default selection — its content loads into the editor.
  const editor = (await screen.findByRole("textbox")) as HTMLTextAreaElement;
  expect(editor.value).toContain("type: project");
  // The built-in-override hint signals the edit-and-save model.
  expect(screen.getByText(/Saving creates a custom override/)).toBeDefined();
});

test("editing and saving fires save_template with the new content", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView();

  const editor = (await screen.findByRole("textbox")) as HTMLTextAreaElement;
  const next = "---\ntype: project\n---\n\n# {{title}}\n";
  fireEvent.change(editor, { target: { value: next } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() => {
    const saved = calls.find((c) => c.cmd === "save_template");
    expect(saved?.args).toMatchObject({ noteType: "project", content: next, variant: null });
  });
});

test("an unknown {{token}} raises a calm non-blocking notice", async () => {
  installMock([]);
  renderView();

  const editor = (await screen.findByRole("textbox")) as HTMLTextAreaElement;
  // {{context}} and {{title}} are known; {{bogus}} is not.
  fireEvent.change(editor, { target: { value: "{{title}} {{bogus}}" } });

  const notice = await screen.findByRole("status");
  expect(notice.textContent).toContain("Unrecognised placeholder");
  expect(notice.textContent).toContain("{{bogus}}");
  // Saving is never blocked — the Save button stays enabled after an edit.
  expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(false);
});

test("a custom type with no template offers Create", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  installMock(calls);
  renderView();

  // Select the template-less custom type.
  fireEvent.click(await screen.findByRole("button", { name: /Person.*No template/ }));

  const create = await screen.findByRole("button", { name: "Create template" });
  fireEvent.click(create);

  await waitFor(() => {
    const called = calls.find((c) => c.cmd === "create_template");
    expect(called?.args).toMatchObject({ noteType: "person" });
  });
});

test("has no axe violations", async () => {
  installMock([]);
  const { container } = renderView();
  await screen.findByRole("textbox");
  expect(await axe(container, AXE_OPTIONS)).toHaveNoViolations();
});
