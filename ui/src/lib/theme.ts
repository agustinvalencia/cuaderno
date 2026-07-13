// Appearance store (plan §3.10, theming amendment 2026-07-13). Three
// independent axes, each stamped onto <html> and each persisted in
// localStorage like the metrics store — a preference, not data, so a
// broken/absent storage degrades to the calm default, never a crash:
//
//   • mode    — light / dark / system (the luminance axis; toggles the
//               `.dark` class + `color-scheme`, following the OS in
//               `system`).
//   • palette — a curated named surface/ink palette that composes ON TOP
//               of the mode via a `data-palette` attribute (globals.css
//               authors each palette for both light and dark).
//   • accent  — the single interactive colour (wikilinks, focus ring),
//               swapped via `data-accent`. Deliberately does NOT touch
//               the seven context hues, so "clickable" never reads as a
//               context signal.
//
// Reactive like lib/metrics.ts (a listener Set + useSyncExternalStore),
// so every control reflects live state and a change previews instantly.
//
// Forward seam (not built here): editor/reader typography prefs — mono
// font size, reading measure, and a token-driven CodeMirror
// `HighlightStyle` to replace CM's generic `defaultHighlightStyle` — will
// extend this store once the CM6 + KaTeX editor work lands. Kept out of
// this change to avoid conflicting with that file.
import { useSyncExternalStore } from "react";

export type Theme = "light" | "dark" | "system";
export type Palette = "default" | "warm" | "cool" | "high-contrast";
export type Accent =
  | "blue"
  | "teal"
  | "violet"
  | "green"
  | "graphite"
  | "rose";

const THEME_KEY = "cuaderno-theme";
const PALETTE_KEY = "cuaderno-palette";
const ACCENT_KEY = "cuaderno-accent";

// `system` and `default`/`blue` are the absent-key defaults: choosing them
// removes the key rather than storing it, so a fresh install and an
// explicit reset look identical.
const PALETTES: Palette[] = ["default", "warm", "cool", "high-contrast"];
const ACCENTS: Accent[] = [
  "blue",
  "teal",
  "violet",
  "green",
  "graphite",
  "rose",
];

// One listener set for all three axes — React controls read whichever
// slice they need; any setter notifies everyone (cheap, and appearance
// changes are rare).
const listeners = new Set<() => void>();

function notify(): void {
  listeners.forEach((fn) => fn());
}

function readString(key: string): string | null {
  // Defensive, like the metrics store: private mode / exotic webview /
  // tests must fall back to the default, never throw.
  try {
    return globalThis.localStorage?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

function writeString(key: string, value: string | null): void {
  // Best-effort persistence: a broken storage still applies to <html>,
  // it just won't be remembered next launch.
  try {
    if (value === null) {
      globalThis.localStorage?.removeItem(key);
    } else {
      globalThis.localStorage?.setItem(key, value);
    }
  } catch {
    // Preference persistence is best-effort.
  }
}

export function storedTheme(): Theme {
  const raw = readString(THEME_KEY);
  return raw === "light" || raw === "dark" ? raw : "system";
}

export function storedPalette(): Palette {
  const raw = readString(PALETTE_KEY);
  return PALETTES.includes(raw as Palette) ? (raw as Palette) : "default";
}

export function storedAccent(): Accent {
  const raw = readString(ACCENT_KEY);
  return ACCENTS.includes(raw as Accent) ? (raw as Accent) : "blue";
}

/** Stamp the current stored appearance onto <html>: the `.dark` class
 * (mode, OS-resolved in `system`), plus `data-palette` / `data-accent`
 * attributes the CSS override blocks key off. The two defaults
 * (`default` palette, `blue` accent) drop the attribute entirely so the
 * base `@theme` tokens win with no extra specificity. */
export function applyAppearance(): void {
  const root = document.documentElement;

  const theme = storedTheme();
  const dark =
    theme === "dark" ||
    (theme === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);
  root.classList.toggle("dark", dark);

  const palette = storedPalette();
  if (palette === "default") {
    root.removeAttribute("data-palette");
  } else {
    root.setAttribute("data-palette", palette);
  }

  const accent = storedAccent();
  if (accent === "blue") {
    root.removeAttribute("data-accent");
  } else {
    root.setAttribute("data-accent", accent);
  }
}

export function setTheme(theme: Theme): void {
  writeString(THEME_KEY, theme === "system" ? null : theme);
  applyAppearance();
  notify();
}

export function setPalette(palette: Palette): void {
  writeString(PALETTE_KEY, palette === "default" ? null : palette);
  applyAppearance();
  notify();
}

export function setAccent(accent: Accent): void {
  writeString(ACCENT_KEY, accent === "blue" ? null : accent);
  applyAppearance();
  notify();
}

/** Call once at startup: apply the stored appearance and keep it in sync
 * with the OS in `system` mode. */
export function initTheme(): void {
  applyAppearance();
  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", () => applyAppearance());
}

function subscribe(onChange: () => void): () => void {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
}

/** Reactive read of the theme mode. */
export function useThemeMode(): Theme {
  return useSyncExternalStore(subscribe, storedTheme, (): Theme => "system");
}

/** Reactive read of the surface/ink palette. */
export function usePalette(): Palette {
  return useSyncExternalStore(subscribe, storedPalette, (): Palette => "default");
}

/** Reactive read of the interactive accent. */
export function useAccent(): Accent {
  return useSyncExternalStore(subscribe, storedAccent, (): Accent => "blue");
}
