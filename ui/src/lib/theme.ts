// Theme: class strategy on <html>. Default follows the OS; a manual
// override persists in localStorage (plan §3.10).
const STORAGE_KEY = "cuaderno-theme";

export type Theme = "light" | "dark" | "system";

export function storedTheme(): Theme {
  const raw = localStorage.getItem(STORAGE_KEY);
  return raw === "light" || raw === "dark" ? raw : "system";
}

export function applyTheme(theme: Theme): void {
  const dark =
    theme === "dark" ||
    (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.classList.toggle("dark", dark);
}

export function setTheme(theme: Theme): void {
  if (theme === "system") {
    localStorage.removeItem(STORAGE_KEY);
  } else {
    localStorage.setItem(STORAGE_KEY, theme);
  }
  applyTheme(theme);
}

export function cycleTheme(): Theme {
  const next: Theme =
    storedTheme() === "system" ? "light" : storedTheme() === "light" ? "dark" : "system";
  setTheme(next);
  return next;
}

/** Call once at startup: apply and track OS changes in system mode. */
export function initTheme(): void {
  applyTheme(storedTheme());
  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", () => applyTheme(storedTheme()));
}
