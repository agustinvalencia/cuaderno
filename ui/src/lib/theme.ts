// Appearance store (plan §3.10, theming amendments 2026-07-13). Several
// independent axes, each stamped onto <html> and each persisted in
// localStorage like the metrics store — a preference, not data, so a
// broken/absent storage degrades to the calm default, never a crash.
//
// Colour / surface axes:
//   • mode    — light / dark / system (the luminance axis; toggles the
//               `.dark` class + `color-scheme`, following the OS in system).
//   • palette — a curated named surface/ink palette (`data-palette`), each
//               authored for both light and dark in globals.css.
//   • accent  — the single interactive colour (`data-accent`). Never
//               touches the seven context hues.
//
// Layout / typography axes (all map to CSS variables in globals.css, so
// the store only stamps an attribute and the stylesheet owns the values):
//   • textSize          — reader + editor body scale (`data-text-size`).
//   • readingWidth      — the reader column measure (`data-reading-width`).
//   • readingFont       — reader body face (`data-reading-font`).
//   • lineSpacing       — reader line-height (`data-line-spacing`).
//   • sidebarWidth      — the shell sidebar width (`data-sidebar-width`).
//   • reduceTransparency — drops the sidebar vibrancy to opaque
//                          (`data-reduce-transparency`), the macOS a11y option.
//
// Reactive like lib/metrics.ts (a listener Set + useSyncExternalStore),
// so every control reflects live state and a change previews instantly.
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
export type TextSize = "small" | "medium" | "large";
export type ReadingWidth = "narrow" | "comfortable" | "wide";
export type ReadingFont = "system" | "serif" | "mono";
export type LineSpacing = "compact" | "normal" | "relaxed";
export type SidebarWidth = "narrow" | "default" | "wide";

const THEME_KEY = "cuaderno-theme";

// One listener set across all axes — React controls read whichever slice
// they need; any setter notifies everyone (cheap, and appearance changes
// are rare).
const listeners = new Set<() => void>();

function notify(): void {
  listeners.forEach((fn) => fn());
}

function subscribe(onChange: () => void): () => void {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
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

// --- Theme mode (special: drives the `.dark` class, not an attribute) ---

export function storedTheme(): Theme {
  const raw = readString(THEME_KEY);
  return raw === "light" || raw === "dark" ? raw : "system";
}

export function setTheme(theme: Theme): void {
  writeString(THEME_KEY, theme === "system" ? null : theme);
  applyAppearance();
  notify();
}

export function useThemeMode(): Theme {
  return useSyncExternalStore(subscribe, storedTheme, (): Theme => "system");
}

// --- Generic enum axis: a `data-*` attribute whose default value removes
// the attribute (so a fresh install and an explicit reset look identical,
// and the base @theme tokens win with no extra specificity). ---

interface EnumAxis<T extends string> {
  stored(): T;
  set(value: T): void;
  use(): T;
  apply(root: HTMLElement): void;
}

function enumAxis<T extends string>(
  storageKey: string,
  attr: string,
  values: readonly T[],
  fallback: T,
): EnumAxis<T> {
  function stored(): T {
    const raw = readString(storageKey);
    return values.includes(raw as T) ? (raw as T) : fallback;
  }
  return {
    stored,
    set(value: T): void {
      writeString(storageKey, value === fallback ? null : value);
      applyAppearance();
      notify();
    },
    use(): T {
      return useSyncExternalStore(subscribe, stored, (): T => fallback);
    },
    apply(root: HTMLElement): void {
      const value = stored();
      if (value === fallback) root.removeAttribute(attr);
      else root.setAttribute(attr, value);
    },
  };
}

// --- Generic boolean axis: a marker attribute, present iff true. ---

interface BoolAxis {
  stored(): boolean;
  set(on: boolean): void;
  use(): boolean;
  apply(root: HTMLElement): void;
}

function boolAxis(storageKey: string, attr: string): BoolAxis {
  function stored(): boolean {
    return readString(storageKey) === "true";
  }
  return {
    stored,
    set(on: boolean): void {
      writeString(storageKey, on ? "true" : null);
      applyAppearance();
      notify();
    },
    use(): boolean {
      return useSyncExternalStore(subscribe, stored, () => false);
    },
    apply(root: HTMLElement): void {
      if (stored()) root.setAttribute(attr, "");
      else root.removeAttribute(attr);
    },
  };
}

const palette = enumAxis<Palette>(
  "cuaderno-palette",
  "data-palette",
  ["default", "warm", "cool", "high-contrast"],
  "default",
);
const accent = enumAxis<Accent>(
  "cuaderno-accent",
  "data-accent",
  ["blue", "teal", "violet", "green", "graphite", "rose"],
  "blue",
);
const textSize = enumAxis<TextSize>(
  "cuaderno-text-size",
  "data-text-size",
  ["small", "medium", "large"],
  "medium",
);
const readingWidth = enumAxis<ReadingWidth>(
  "cuaderno-reading-width",
  "data-reading-width",
  ["narrow", "comfortable", "wide"],
  "comfortable",
);
const readingFont = enumAxis<ReadingFont>(
  "cuaderno-reading-font",
  "data-reading-font",
  ["system", "serif", "mono"],
  "system",
);
const lineSpacing = enumAxis<LineSpacing>(
  "cuaderno-line-spacing",
  "data-line-spacing",
  ["compact", "normal", "relaxed"],
  "normal",
);
const sidebarWidth = enumAxis<SidebarWidth>(
  "cuaderno-sidebar-width",
  "data-sidebar-width",
  ["narrow", "default", "wide"],
  "default",
);
const reduceTransparency = boolAxis(
  "cuaderno-reduce-transparency",
  "data-reduce-transparency",
);

// Public API per axis — named wrappers keep call sites readable and the
// store's internals private.
export const storedPalette = palette.stored;
export const setPalette = palette.set;
export const usePalette = palette.use;

export const storedAccent = accent.stored;
export const setAccent = accent.set;
export const useAccent = accent.use;

export const setTextSize = textSize.set;
export const useTextSize = textSize.use;

export const setReadingWidth = readingWidth.set;
export const useReadingWidth = readingWidth.use;

export const setReadingFont = readingFont.set;
export const useReadingFont = readingFont.use;

export const setLineSpacing = lineSpacing.set;
export const useLineSpacing = lineSpacing.use;

export const setSidebarWidth = sidebarWidth.set;
export const useSidebarWidth = sidebarWidth.use;

export const setReduceTransparency = reduceTransparency.set;
export const useReduceTransparency = reduceTransparency.use;

/** Stamp the full stored appearance onto <html>: the `.dark` class (mode,
 * OS-resolved in system) plus every axis's `data-*` attribute. */
export function applyAppearance(): void {
  const root = document.documentElement;

  const theme = storedTheme();
  const dark =
    theme === "dark" ||
    (theme === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);
  root.classList.toggle("dark", dark);

  palette.apply(root);
  accent.apply(root);
  textSize.apply(root);
  readingWidth.apply(root);
  readingFont.apply(root);
  lineSpacing.apply(root);
  sidebarWidth.apply(root);
  reduceTransparency.apply(root);
}

/** Call once at startup: apply the stored appearance and keep it in sync
 * with the OS in system mode. */
export function initTheme(): void {
  applyAppearance();
  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", () => applyAppearance());
}
