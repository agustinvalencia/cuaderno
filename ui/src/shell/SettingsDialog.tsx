// The app Settings panel (user request 2026-07-12), summoned with ⌘, — the
// macOS Preferences convention. A calm centred modal (the shared Dialog
// primitive) rather than a second window: it floats over the current view
// without disturbing it or the back/forward history. Home for the
// app-level preferences that were scattered as blind footer toggles.
//
// Two grouped sections: Appearance (theme mode, palette, accent — the
// theming amendment 2026-07-13, all reactive so each click previews live)
// and General (the metrics switch + a jump to the vault's config editor).
import type { ReactNode } from "react";
import { useNavigate } from "react-router";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogTitle,
} from "../components/ui/dialog";
import { setShowMetrics, useMetrics } from "../lib/metrics";
import {
  setAccent,
  setPalette,
  setTheme,
  useAccent,
  usePalette,
  useThemeMode,
  type Accent,
  type Palette,
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

// Preview swatch colours — the light-mode accent value of each option, so
// the picker shows the hue without depending on `data-accent` being live.
// Mirrors the accent tokens in globals.css.
const ACCENTS: { value: Accent; label: string; swatch: string }[] = [
  { value: "blue", label: "Blue", swatch: "oklch(0.55 0.11 250)" },
  { value: "teal", label: "Teal", swatch: "oklch(0.55 0.09 195)" },
  { value: "violet", label: "Violet", swatch: "oklch(0.55 0.12 300)" },
  { value: "green", label: "Green", swatch: "oklch(0.55 0.1 150)" },
  { value: "graphite", label: "Graphite", swatch: "oklch(0.45 0.012 260)" },
  { value: "rose", label: "Rose", swatch: "oklch(0.55 0.12 350)" },
];

export default function SettingsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const navigate = useNavigate();
  const showMetrics = useMetrics();
  // Reactive reads (useSyncExternalStore) — the store, not local state, is
  // the source of truth, so controls stay honest even if appearance is
  // changed elsewhere, and each choice previews the moment it's set.
  const theme = useThemeMode();
  const palette = usePalette();
  const accent = useAccent();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent aria-describedby={undefined}>
        <DialogTitle className="text-base font-semibold text-ink">
          Settings
        </DialogTitle>

        <div className="mt-5 space-y-6">
          <section className="space-y-4">
            <SectionLabel>Appearance</SectionLabel>

            <Row label="Theme" hint="Follows the system unless overridden.">
              <Segmented
                ariaLabel="Theme"
                value={theme}
                options={THEMES}
                onChange={setTheme}
              />
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
              <div
                role="group"
                aria-label="Accent"
                className="flex items-center gap-1.5"
              >
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
          </section>

          <section className="space-y-4">
            <SectionLabel>General</SectionLabel>

            <Row
              label="Show metrics"
              hint="Progress bars and charts, hidden by default."
            >
              <button
                type="button"
                role="switch"
                aria-checked={showMetrics}
                aria-label="Show progress metrics"
                onClick={() => setShowMetrics(!showMetrics)}
                className={`relative h-5 w-9 shrink-0 rounded-full transition-colors ${
                  showMetrics ? "bg-accent-interactive" : "bg-bg-sunken"
                }`}
              >
                <span
                  className={`absolute top-0.5 h-4 w-4 rounded-full bg-bg-surface transition-all ${
                    showMetrics ? "left-4" : "left-0.5"
                  }`}
                />
              </button>
            </Row>

            <Row
              label="Vault config"
              hint="Edit .cuaderno/config.toml — note types and schemas."
            >
              <button
                type="button"
                onClick={() => {
                  onOpenChange(false);
                  navigate("/config");
                }}
                className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
              >
                Edit…
              </button>
            </Row>
          </section>
        </div>

        <div className="mt-6 flex justify-end">
          <DialogClose asChild>
            <button
              type="button"
              className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
            >
              Done
            </button>
          </DialogClose>
        </div>
      </DialogContent>
    </Dialog>
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
