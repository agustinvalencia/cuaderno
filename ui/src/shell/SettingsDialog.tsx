// The app Settings panel (user request 2026-07-12), summoned with ⌘, — the
// macOS Preferences convention. A calm centred modal (the shared Dialog
// primitive) rather than a second window: it floats over the current view
// without disturbing it or the back/forward history.
//
// Sections, reached by a left rail (#444):
//   • Appearance   — theme mode, palette, accent, sidebar width, and the
//     reduce-transparency (opaque sidebar) toggle.
//   • Reading      — the markdown reader's typography: text size, column
//     width, body font, and line spacing (the "tweak how markdown renders"
//     surface; maths scales with text size for free).
//   • General      — the metrics switch.
//   • Vault config — the full `.cuaderno/config.toml` editor.
//   • Templates    — the note-type template browser and editor.
//   • Advanced     — custom CSS.
//
// Vault config and Templates used to sit in the sidebar's "Browse" list,
// beside Portfolios and Stewardships, which invited the reading that a
// template is a note. They are configuration, so they live here. That the
// dialog can host a real editor was already settled by the custom-CSS
// CodeMirror below.
//
// It also changes the dialog's contract. Every other preference here
// applies the instant it is clicked, so "closing is always safe" was
// assumed rather than enforced; these two panels carry real drafts, and
// the guard at the bottom is what keeps Esc from discarding one.
import { lazy, Suspense, useCallback, useEffect, useState, type ReactNode } from "react";
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from "../components/ui/dialog";
import CustomCssEditor from "./CustomCssEditor";
import { SettingsDirtyProvider } from "./settingsDirty";
import { openCustomCss } from "../api/commands";
import { loadCustomCss } from "../lib/customCss";
import { setShowMetrics, useMetrics } from "../lib/metrics";
import {
  setAccent,
  setLineSpacing,
  setPalette,
  setReadingFont,
  setReadingWidth,
  setReduceTransparency,
  setSidebarWidth,
  setTextSize,
  setTheme,
  useAccent,
  useLineSpacing,
  usePalette,
  useReadingFont,
  useReadingWidth,
  useReduceTransparency,
  useSidebarWidth,
  useTextSize,
  useThemeMode,
  type Accent,
  type LineSpacing,
  type Palette,
  type ReadingFont,
  type ReadingWidth,
  type SidebarWidth,
  type TextSize,
  type Theme,
} from "../lib/theme";

const THEMES: { value: Theme; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const PALETTES: { value: Palette; label: string }[] = [
  { value: "default", label: "Default" },
  { value: "warm", label: "Warm" },
  { value: "cool", label: "Cool" },
  { value: "high-contrast", label: "Contrast" },
];

// Preview swatch colours reference the single-source `--accent-*` tokens
// (globals.css), so the picker can never drift from the actual accents the
// way duplicated literals would.
const ACCENTS: { value: Accent; label: string; swatch: string }[] = [
  { value: "blue", label: "Blue", swatch: "var(--accent-blue)" },
  { value: "teal", label: "Teal", swatch: "var(--accent-teal)" },
  { value: "violet", label: "Violet", swatch: "var(--accent-violet)" },
  { value: "green", label: "Green", swatch: "var(--accent-green)" },
  { value: "graphite", label: "Graphite", swatch: "var(--accent-graphite)" },
  { value: "rose", label: "Rose", swatch: "var(--accent-rose)" },
];

const SIDEBAR_WIDTHS: { value: SidebarWidth; label: string }[] = [
  { value: "narrow", label: "Narrow" },
  { value: "default", label: "Default" },
  { value: "wide", label: "Wide" },
];

const TEXT_SIZES: { value: TextSize; label: string }[] = [
  { value: "small", label: "Small" },
  { value: "medium", label: "Medium" },
  { value: "large", label: "Large" },
];

const READING_WIDTHS: { value: ReadingWidth; label: string }[] = [
  { value: "narrow", label: "Narrow" },
  { value: "comfortable", label: "Default" },
  { value: "wide", label: "Wide" },
];

// "Sans" (not "System") so the label never collides with the Theme
// group's "System" — for the test query and, more importantly, for a
// screen-reader user hearing two identical button names.
const READING_FONTS: { value: ReadingFont; label: string }[] = [
  { value: "system", label: "Sans" },
  { value: "serif", label: "Serif" },
  { value: "mono", label: "Mono" },
];

const LINE_SPACINGS: { value: LineSpacing; label: string }[] = [
  { value: "compact", label: "Compact" },
  { value: "normal", label: "Normal" },
  { value: "relaxed", label: "Relaxed" },
];

// Lazily mounted, and only the two heavy ones: `App.tsx` code-splits both
// precisely to keep them off the first paint, and this dialog is imported
// eagerly by the shell — so importing them at the top would pull the
// config editor and the template browser into the shell chunk and undo
// that split.
const ConfigPanel = lazy(() =>
  import("../views/config/Config").then((m) => ({ default: m.ConfigPanel })),
);
const TemplatesPanel = lazy(() =>
  import("../views/templates/Templates").then((m) => ({ default: m.TemplatesPanel })),
);

/** Which pane of the Settings dialog is showing. Also the value the
 * command palette passes to open the dialog straight at Vault config or
 * Templates — the two destinations that left the sidebar. */
export type SettingsSection =
  | "appearance"
  | "reading"
  | "general"
  | "config"
  | "templates"
  | "advanced";

const SECTIONS: { key: SettingsSection; label: string }[] = [
  { key: "appearance", label: "Appearance" },
  { key: "reading", label: "Reading" },
  { key: "general", label: "General" },
  { key: "config", label: "Vault config" },
  { key: "templates", label: "Templates" },
  { key: "advanced", label: "Advanced" },
];

export default function SettingsDialog({
  section,
  onSectionChange,
}: {
  /** The showing section, or `null` for closed. One piece of state rather
   * than an `open` flag beside it, so the palette can open the dialog at a
   * section without a second round trip. */
  section: SettingsSection | null;
  onSectionChange: (section: SettingsSection | null) => void;
}) {
  const open = section !== null;
  const showMetrics = useMetrics();
  const [cssEditorOpen, setCssEditorOpen] = useState(false);
  // Reactive reads (useSyncExternalStore) — the store, not local state, is
  // the source of truth, so each choice previews the moment it's set.
  const theme = useThemeMode();
  const palette = usePalette();
  const accent = useAccent();
  const sidebarWidth = useSidebarWidth();
  const reduceTransparency = useReduceTransparency();
  const textSize = useTextSize();
  const readingWidth = useReadingWidth();
  const readingFont = useReadingFont();
  const lineSpacing = useLineSpacing();

  // Which panes have been opened at least once. The two editors mount
  // lazily on first visit and then stay mounted (hidden), because
  // unmounting them on a rail switch would throw away a draft just as
  // quietly as closing the dialog would — the thing the guard below
  // exists to prevent.
  const [visited, setVisited] = useState<SettingsSection[]>([]);
  useEffect(() => {
    if (section === null) {
      setVisited((prev) => (prev.length === 0 ? prev : []));
      return;
    }
    setVisited((prev) => (prev.includes(section) ? prev : [...prev, section]));
  }, [section]);

  // What the hosted panels report as unsaved, keyed by panel.
  const [drafts, setDrafts] = useState<Record<string, string | null>>({});
  // Stable, or the panels' effects would re-report on every render.
  const report = useCallback((key: string, label: string | null) => {
    setDrafts((prev) => ((prev[key] ?? null) === label ? prev : { ...prev, [key]: label }));
  }, []);
  const unsaved = Object.values(drafts).filter((label): label is string => label !== null);

  const [confirmingClose, setConfirmingClose] = useState(false);
  // Saving while the prompt is up answers it: nothing is at risk any more.
  useEffect(() => {
    if (unsaved.length === 0) setConfirmingClose(false);
  }, [unsaved.length]);

  function close() {
    setConfirmingClose(false);
    onSectionChange(null);
  }

  /** Radix's single close channel — Esc, the overlay, and Done all land
   * here, so the guard covers every route out rather than just the button. */
  function requestOpenChange(next: boolean) {
    if (next) return;
    if (unsaved.length > 0) {
      setConfirmingClose(true);
      return;
    }
    close();
  }

  return (
    <SettingsDirtyProvider value={report}>
    <Dialog open={open} onOpenChange={requestOpenChange}>
      <DialogContent
        aria-describedby={undefined}
        // Inline, not a class: the primitive already sets a `w-[min(...)]`
        // arbitrary value, and two arbitrary widths in the same utility
        // group resolve by stylesheet order, which is not ours to rely on.
        style={{ width: "min(56rem, calc(100vw - 2rem))" }}
        className="max-h-[85vh]"
      >
        <DialogTitle className="text-base font-semibold text-ink">
          Settings
        </DialogTitle>

        <div className="mt-4 flex min-h-0 flex-1 gap-6">
          <nav
            aria-label="Settings sections"
            className="flex w-36 shrink-0 flex-col gap-0.5"
          >
            {SECTIONS.map(({ key, label }) => (
              <button
                key={key}
                type="button"
                aria-current={section === key ? "true" : undefined}
                onClick={() => onSectionChange(key)}
                className={`flex items-center justify-between gap-2 rounded px-2 py-1 text-left text-sm ${
                  section === key
                    ? "bg-bg-sunken font-medium text-ink"
                    : "text-ink-muted hover:text-ink"
                }`}
              >
                <span className="truncate">{label}</span>
                {/* A pane holding an unsaved draft is marked, so the
                    warning at the door is never the first you hear of it.
                    Ink, not a hue — there is no red token. */}
                {drafts[key] != null && (
                  <span
                    aria-hidden
                    className="h-1.5 w-1.5 shrink-0 rounded-full bg-ink-faint"
                  />
                )}
              </button>
            ))}
          </nav>

          {/* Scrollable: a pane outgrows the viewport on short windows. */}
          <div className="min-h-0 min-w-0 flex-1 overflow-y-auto pr-1">
            <div className="space-y-6" hidden={section !== "appearance"}>
              <section className="space-y-4">
                <SectionLabel>Appearance</SectionLabel>

                <Row label="Theme" hint="Follows the system unless overridden.">
                  <Segmented ariaLabel="Theme" value={theme} options={THEMES} onChange={setTheme} />
                </Row>

                <Row label="Palette" hint="Surface and text colour family.">
                  <Segmented
                    ariaLabel="Palette"
                    value={palette}
                    options={PALETTES}
                    onChange={setPalette}
                  />
                </Row>

                <Row label="Accent" hint="The one interactive colour.">
                  <div role="group" aria-label="Accent" className="flex items-center gap-1.5">
                    {ACCENTS.map((option) => (
                      <button
                        key={option.value}
                        type="button"
                        aria-pressed={accent === option.value}
                        aria-label={option.label}
                        title={option.label}
                        onClick={() => setAccent(option.value)}
                        style={{ backgroundColor: option.swatch }}
                        className={`h-5 w-5 rounded-full transition-transform ${
                          accent === option.value
                            ? "ring-2 ring-offset-2 ring-offset-bg-surface ring-ink scale-110"
                            : "hover:scale-110"
                        }`}
                      />
                    ))}
                  </div>
                </Row>

                <Row label="Sidebar" hint="Width of the navigation rail.">
                  <Segmented
                    ariaLabel="Sidebar width"
                    value={sidebarWidth}
                    options={SIDEBAR_WIDTHS}
                    onChange={setSidebarWidth}
                  />
                </Row>

                <Row label="Reduce transparency" hint="Make the sidebar opaque.">
                  <Switch
                    checked={reduceTransparency}
                    onChange={setReduceTransparency}
                    ariaLabel="Reduce transparency"
                  />
                </Row>
              </section>
            </div>

            <div className="space-y-6" hidden={section !== "reading"}>
              <section className="space-y-4">
                <SectionLabel>Reading</SectionLabel>

                <Row label="Text size" hint="Note body and editor text.">
                  <Segmented
                    ariaLabel="Text size"
                    value={textSize}
                    options={TEXT_SIZES}
                    onChange={setTextSize}
                  />
                </Row>

                <Row label="Reading width" hint="How wide a note's column runs.">
                  <Segmented
                    ariaLabel="Reading width"
                    value={readingWidth}
                    options={READING_WIDTHS}
                    onChange={setReadingWidth}
                  />
                </Row>

                <Row label="Reading font" hint="The note body typeface.">
                  <Segmented
                    ariaLabel="Reading font"
                    value={readingFont}
                    options={READING_FONTS}
                    onChange={setReadingFont}
                  />
                </Row>

                <Row label="Line spacing" hint="Space between lines of prose.">
                  <Segmented
                    ariaLabel="Line spacing"
                    value={lineSpacing}
                    options={LINE_SPACINGS}
                    onChange={setLineSpacing}
                  />
                </Row>
              </section>
            </div>

            <div className="space-y-6" hidden={section !== "general"}>
              <section className="space-y-4">
                <SectionLabel>General</SectionLabel>

                <Row label="Show metrics" hint="Progress bars and charts, hidden by default.">
                  <Switch
                    checked={showMetrics}
                    onChange={setShowMetrics}
                    ariaLabel="Show progress metrics"
                  />
                </Row>
              </section>
            </div>

            {/* The two hosted editors. Rendered from first visit onward and
                only hidden thereafter, so a draft survives a look at another
                pane. */}
            {visited.includes("config") && (
              <div hidden={section !== "config"}>
                <Suspense fallback={<p className="text-sm text-ink-muted">Loading…</p>}>
                  <ConfigPanel />
                </Suspense>
              </div>
            )}

            {visited.includes("templates") && (
              <div hidden={section !== "templates"}>
                <Suspense fallback={<p className="text-sm text-ink-muted">Loading…</p>}>
                  <TemplatesPanel />
                </Suspense>
              </div>
            )}

            <div className="space-y-6" hidden={section !== "advanced"}>
              <section className="space-y-4">
                <SectionLabel>Advanced</SectionLabel>

                <Row
                  label="Custom CSS"
                  hint="Redefine any token in .cuaderno/custom.css."
                >
                  {/* Both Edit actions eject the file immediately (seeding a
                      documented template the first time). Edit in app opens the
                      built-in editor (applies on Save); Edit in editor hands it
                      to the external editor (applies on window focus); Reload
                      re-applies the current file now. */}
                  <div className="flex flex-wrap justify-end gap-2">
                    <button
                      type="button"
                      onClick={() => setCssEditorOpen(true)}
                      className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
                    >
                      Edit in app
                    </button>
                    <button
                      type="button"
                      onClick={() => void openCustomCss()}
                      className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
                    >
                      Edit in editor
                    </button>
                    <button
                      type="button"
                      onClick={() => void loadCustomCss()}
                      className="rounded px-3 py-1 text-sm text-ink-muted hover:text-ink"
                    >
                      Reload
                    </button>
                  </div>
                </Row>
              </section>
            </div>
          </div>
        </div>

        <div className="mt-5 flex items-center justify-end gap-3 border-t border-line pt-4">
          {confirmingClose ? (
            <>
              {/* Stated, then offered a way out — the discard is never the
                  default action, and never the only one. */}
              <p role="alert" className="mr-auto text-sm text-attention">
                Unsaved changes in {unsaved.join(" and ")}. Closing now discards them.
              </p>
              <button
                type="button"
                onClick={() => setConfirmingClose(false)}
                className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
              >
                Keep editing
              </button>
              <button
                type="button"
                onClick={close}
                className="rounded px-3 py-1 text-sm text-ink-muted hover:text-ink"
              >
                Discard and close
              </button>
            </>
          ) : (
            <>
              {unsaved.length > 0 && (
                <p className="mr-auto text-xs text-ink-faint">
                  Unsaved changes in {unsaved.join(" and ")}.
                </p>
              )}
              <button
                type="button"
                onClick={() => requestOpenChange(false)}
                className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
              >
                Done
              </button>
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
    {/* Mounted only while open: it pulls the CSS-language CodeMirror and
        uses the toast context, neither of which should load (or be
        required) until the editor is actually summoned. */}
    {cssEditorOpen && <CustomCssEditor open onOpenChange={setCssEditorOpen} />}
    </SettingsDirtyProvider>
  );
}

/** A small uppercase group heading separating the settings sections. */
function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <p className="text-xs font-medium uppercase tracking-wider text-ink-faint">
      {children}
    </p>
  );
}

/** A labelled preference row: title + hint on the left, its control on
 * the right. */
function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint: string;
  children: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <div className="min-w-0">
        <p className="text-sm text-ink">{label}</p>
        <p className="text-xs text-ink-faint">{hint}</p>
      </div>
      {children}
    </div>
  );
}

/** A segmented toggle group, not a form radio set: each option applies
 * immediately on click, so `aria-pressed` buttons are honest about the
 * interaction — no roving-tabindex/arrow-key contract a `radiogroup` would
 * imply but not deliver. */
function Segmented<T extends string>({
  ariaLabel,
  value,
  options,
  onChange,
}: {
  ariaLabel: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <div
      role="group"
      aria-label={ariaLabel}
      className="flex gap-0.5 rounded-md bg-bg-sunken p-0.5"
    >
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
          className={`rounded px-2.5 py-1 text-xs ${
            value === option.value
              ? "bg-bg-surface font-medium text-ink shadow-sm"
              : "text-ink-muted hover:text-ink"
          }`}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

/** A binary toggle switch (aria `switch`), applied immediately on click. */
function Switch({
  checked,
  onChange,
  ariaLabel,
}: {
  checked: boolean;
  onChange: (on: boolean) => void;
  ariaLabel: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      onClick={() => onChange(!checked)}
      className={`relative h-5 w-9 shrink-0 rounded-full transition-colors ${
        checked ? "bg-accent-interactive" : "bg-bg-sunken"
      }`}
    >
      <span
        className={`absolute top-0.5 h-4 w-4 rounded-full bg-bg-surface transition-all ${
          checked ? "left-4" : "left-0.5"
        }`}
      />
    </button>
  );
}
