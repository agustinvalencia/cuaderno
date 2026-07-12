// Theme: class strategy on <html>. Default follows the OS; a manual
// override persists in localStorage (plan §3.10).
const STORAGE_KEY = "cuaderno-theme";

export type Theme = "light" | "dark" | "system";

export function storedTheme(): Theme {
  // Defensive, like the metrics store: a broken/absent localStorage
  // (private mode, exotic webview, tests) means "follow the system",
  // never a crash — this is a preference, not data.
  try {
    const raw = globalThis.localStorage?.getItem(STORAGE_KEY);
    return raw === "light" || raw === "dark" ? raw : "system";
  } catch {
    return "system";
  }
}

export function applyTheme(theme: Theme): void {
  const dark =
    theme === "dark" ||
    (theme === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.classList.toggle("dark", dark);
}

export function setTheme(theme: Theme): void {
  // Best-effort persistence, like the metrics store: a broken/absent
  // localStorage still applies the theme to <html>, just doesn't remember it.
  try {
    if (theme === "system") {
      globalThis.localStorage?.removeItem(STORAGE_KEY);
    } else {
      globalThis.localStorage?.setItem(STORAGE_KEY, theme);
    }
  } catch {
    // Preference persistence is best-effort.
  }
  applyTheme(theme);
}

/** Call once at startup: apply and track OS changes in system mode. */
export function initTheme(): void {
  applyTheme(storedTheme());
  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", () => applyTheme(storedTheme()));
}
