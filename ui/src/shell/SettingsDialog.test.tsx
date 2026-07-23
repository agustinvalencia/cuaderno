// SettingsDialog (⌘,): renders the app preferences, persists theme and
// metrics choices, navigates its section rail, hosts the vault config and
// template editors (#444), refuses to close over an unsaved draft, and is
// axe-clean.
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import * as matchers from "vitest-axe/matchers";
import { axe } from "vitest-axe";
import type { AxeMatchers } from "vitest-axe";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { useState } from "react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ToastProvider } from "./Toasts";
import SettingsDialog, { type SettingsSection } from "./SettingsDialog";

expect.extend(matchers);
declare module "vitest" {
  interface Assertion<T = any> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}

// Give the theme/metrics stores a deterministic, isolated localStorage, plus
// the layout/media APIs Radix Dialog and the theme helper reach.
beforeAll(() => {
  const store = new Map<string, string>();
  const local: Storage = {
    getItem: (key) => store.get(key) ?? null,
    setItem: (key, value) => void store.set(key, String(value)),
    removeItem: (key) => void store.delete(key),
    clear: () => store.clear(),
    key: (index) => [...store.keys()][index] ?? null,
    get length() {
      return store.size;
    },
  };
  Object.defineProperty(window, "localStorage", {
    value: local,
    configurable: true,
  });
  if (!Element.prototype.scrollIntoView)
    Element.prototype.scrollIntoView = () => {};
  globalThis.ResizeObserver ||= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
  window.matchMedia ||= ((query: string) =>
    ({
      matches: false,
      media: query,
      addEventListener() {},
      removeEventListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia;
});

afterEach(() => {
  cleanup();
  clearMocks();
  localStorage.clear();
});

const CONFIG_TOML = '[vault]\nname = "Test"\nmax_active_projects = 5\n';

/** Serve the two hosted panels' reads. Only the config editor is
 * exercised here; the template browser has its own suite. */
function installMock() {
  mockIPC((cmd) => {
    switch (cmd) {
      case "read_config":
        return { content: CONFIG_TOML, hash: "deadbeefdeadbeef" };
      case "list_templates":
        return [];
      case "validate_config":
        return undefined;
      default:
        return undefined;
    }
  });
}

/** The dialog is controlled by the shell, so the harness holds the same
 * state the shell does: the rail writes back through `onSectionChange`,
 * and `null` is closed. */
function Harness({
  onSectionChange,
  initial = "appearance",
}: {
  onSectionChange?: (section: SettingsSection | null) => void;
  initial?: SettingsSection;
}) {
  const [section, setSection] = useState<SettingsSection | null>(initial);
  return (
    <SettingsDialog
      section={section}
      onSectionChange={(next) => {
        onSectionChange?.(next);
        setSection(next);
      }}
    />
  );
}

function renderDialog(
  onSectionChange?: (section: SettingsSection | null) => void,
  initial: SettingsSection = "appearance",
) {
  installMock();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={["/"]}>
          <Harness onSectionChange={onSectionChange} initial={initial} />
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

/** Move to a section by clicking its rail entry. */
function openSection(label: string) {
  fireEvent.click(screen.getByRole("button", { name: label }));
}

test("renders the preference controls", () => {
  renderDialog();
  expect(screen.getByRole("heading", { name: "Settings" })).toBeDefined();
  expect(screen.getByRole("button", { name: "System" })).toBeDefined();
  expect(screen.getByRole("button", { name: "Dark" })).toBeDefined();
});

test("the rail names every section, configuration included", () => {
  renderDialog();
  const rail = within(screen.getByRole("navigation", { name: "Settings sections" }));
  for (const label of [
    "Appearance",
    "Reading",
    "General",
    "Vault config",
    "Templates",
    "Advanced",
  ]) {
    expect(rail.getByRole("button", { name: label })).toBeDefined();
  }
});

test("only the chosen section is showing", () => {
  renderDialog();
  // Reading and General live behind their own rail entries, so their
  // controls are out of the tree until chosen.
  expect(screen.queryByRole("switch", { name: "Show progress metrics" })).toBeNull();
  openSection("General");
  expect(screen.getByRole("switch", { name: "Show progress metrics" })).toBeDefined();
});

test("choosing a theme marks it selected and persists", () => {
  renderDialog();
  // The theme options are a segmented toggle group (aria-pressed), not radios.
  // Default is System (no stored override).
  expect(
    screen.getByRole("button", { name: "System" }).getAttribute("aria-pressed"),
  ).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: "Dark" }));
  expect(
    screen.getByRole("button", { name: "Dark" }).getAttribute("aria-pressed"),
  ).toBe("true");
  expect(localStorage.getItem("cuaderno-theme")).toBe("dark");
});

test("choosing a reading setting marks it selected and persists", () => {
  renderDialog();
  openSection("Reading");
  // A Reading-section segmented control (aria-pressed), like the theme group.
  fireEvent.click(screen.getByRole("button", { name: "Large" }));
  expect(
    screen.getByRole("button", { name: "Large" }).getAttribute("aria-pressed"),
  ).toBe("true");
  expect(localStorage.getItem("cuaderno-text-size")).toBe("large");
});

test("reduce transparency flips the switch and persists", () => {
  renderDialog();
  const toggle = screen.getByRole("switch", { name: "Reduce transparency" });
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-checked")).toBe("true");
  expect(localStorage.getItem("cuaderno-reduce-transparency")).toBe("true");
});

test("toggling metrics flips the switch and persists", () => {
  renderDialog();
  openSection("General");
  const toggle = screen.getByRole("switch", { name: "Show progress metrics" });
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-checked")).toBe("true");
  expect(localStorage.getItem("cuaderno-show-metrics")).toBe("true");
});

test("the vault config editor opens in the dialog rather than routing away", async () => {
  // It used to be an "Edit…" row that closed the dialog and navigated to
  // /config. Configuration is not content, so it lives here now.
  renderDialog();
  openSection("Vault config");
  const editor = (await screen.findByLabelText("config.toml content")) as HTMLTextAreaElement;
  expect(editor.value).toContain("max_active_projects");
});

test("Done closes the dialog", () => {
  const onSectionChange = vi.fn();
  renderDialog(onSectionChange);
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(onSectionChange).toHaveBeenCalledWith(null);
});

test("an unsaved draft blocks the close, and can be kept or discarded", async () => {
  // Every other preference here applies on click, so nothing ever needed
  // guarding. The hosted editors have a real Save, and Radix would hand a
  // silent discard to Esc, the overlay or Done.
  const onSectionChange = vi.fn();
  renderDialog(onSectionChange);
  openSection("Vault config");
  const editor = await screen.findByLabelText("config.toml content");
  fireEvent.change(editor, { target: { value: "[vault]\nname = \"Edited\"\n" } });

  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(onSectionChange).not.toHaveBeenCalledWith(null);
  expect((await screen.findByRole("alert")).textContent).toContain("Vault config");

  // Keeping editing leaves the draft in hand.
  fireEvent.click(screen.getByRole("button", { name: "Keep editing" }));
  expect(screen.queryByRole("alert")).toBeNull();
  expect((screen.getByLabelText("config.toml content") as HTMLTextAreaElement).value).toContain(
    "Edited",
  );

  // Discarding is a second, deliberate click.
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  fireEvent.click(await screen.findByRole("button", { name: "Discard and close" }));
  expect(onSectionChange).toHaveBeenCalledWith(null);
});

test("Esc closes a dialog with nothing unsaved", () => {
  // The control for the guard test below: without this, "Esc did not
  // close" would pass just as well if Esc never reached Radix at all.
  const onSectionChange = vi.fn();
  renderDialog(onSectionChange);
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
  expect(onSectionChange).toHaveBeenCalledWith(null);
});

test("Esc over an unsaved draft is guarded too, not just Done", async () => {
  // The guard sits on Radix's one close channel, so every route out is
  // covered — a guard on the button alone would leave two doors open.
  const onSectionChange = vi.fn();
  renderDialog(onSectionChange);
  openSection("Vault config");
  const editor = await screen.findByLabelText("config.toml content");
  fireEvent.change(editor, { target: { value: "[vault]\nname = \"Edited\"\n" } });

  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
  expect(onSectionChange).not.toHaveBeenCalledWith(null);
  expect((await screen.findByRole("alert")).textContent).toContain("Vault config");
});

test("a draft survives a look at another section", async () => {
  // The panes unmount-on-switch would be the same silent discard the
  // close guard exists to prevent, one click earlier.
  renderDialog();
  openSection("Vault config");
  const editor = await screen.findByLabelText("config.toml content");
  fireEvent.change(editor, { target: { value: "[vault]\nname = \"Edited\"\n" } });

  openSection("Appearance");
  // Still mounted, but `hidden` — so it is out of the accessibility tree
  // (a role query cannot see it) while its draft stays in hand.
  expect(screen.queryByRole("textbox")).toBeNull();
  // The rail still marks it, so the draft is not out of sight entirely.
  await waitFor(() =>
    expect(screen.getByText(/Unsaved changes in Vault config/)).toBeDefined(),
  );

  openSection("Vault config");
  expect((screen.getByLabelText("config.toml content") as HTMLTextAreaElement).value).toContain(
    "Edited",
  );
});

test("is axe-clean", async () => {
  const { baseElement } = renderDialog();
  expect(await axe(baseElement)).toHaveNoViolations();
});
