// The app Settings panel (user request 2026-07-12), summoned with ⌘, — the
// macOS Preferences convention. A calm centred modal (the shared Dialog
// primitive) rather than a second window: it floats over the current view
// without disturbing it or the back/forward history. Home for the
// app-level preferences that were scattered as blind footer toggles —
// theme (now a segmented control showing the current choice) and the
// metrics switch — plus a jump to the vault's config editor.
import { useState } from "react";
import type { ReactNode } from "react";
import { useNavigate } from "react-router";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogTitle,
} from "../components/ui/dialog";
import { setShowMetrics, useMetrics } from "../lib/metrics";
import { setTheme, storedTheme, type Theme } from "../lib/theme";

const THEMES: { value: Theme; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
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
  // Theme has no reactive store (it just stamps <html>); seed local state
  // once from the stored choice (lazy initializer — no read per render), and
  // this dialog is the only control that sets it.
  const [theme, setThemeState] = useState(() => storedTheme());

  function chooseTheme(next: Theme) {
    setTheme(next);
    setThemeState(next);
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent aria-describedby={undefined}>
        <DialogTitle className="text-base font-semibold text-ink">
          Settings
        </DialogTitle>

        <div className="mt-5 space-y-5">
          <Row label="Theme" hint="Follows the system unless overridden.">
            {/* A segmented toggle group, not a form radio set: each option
                applies immediately on click, so `aria-pressed` buttons are
                honest about the interaction — no roving-tabindex/arrow-key
                contract a `radiogroup` would imply but not deliver. */}
            <div
              role="group"
              aria-label="Theme"
              className="flex gap-0.5 rounded-md bg-bg-sunken p-0.5"
            >
              {THEMES.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  aria-pressed={theme === option.value}
                  onClick={() => chooseTheme(option.value)}
                  className={`rounded px-2.5 py-1 text-xs ${
                    theme === option.value
                      ? "bg-bg-surface font-medium text-ink shadow-sm"
                      : "text-ink-muted hover:text-ink"
                  }`}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </Row>

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
