// The appearance store: three independent axes (mode / palette / accent),
// each persisted to localStorage and stamped onto <html>. The load-bearing
// invariants are (1) the two defaults — `system`, `default`, `blue` —
// remove their key/attribute rather than storing them, so a fresh install
// and an explicit reset are indistinguishable, and (2) a bogus stored
// value degrades to the default, never throws.
import { beforeEach, expect, test, vi } from "vitest";
import {
  applyAppearance,
  setAccent,
  setPalette,
  setTheme,
  storedAccent,
  storedPalette,
  storedTheme,
} from "./theme";

// A working in-memory Storage — this jsdom's built-in localStorage is a
// non-functional shim (no setItem/removeItem), so we stub our own.
function memoryStorage(): Storage {
  const map = new Map<string, string>();
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key: string) => map.get(key) ?? null,
    key: (index: number) => [...map.keys()][index] ?? null,
    removeItem: (key: string) => void map.delete(key),
    setItem: (key: string, value: string) => void map.set(key, String(value)),
  } as Storage;
}

beforeEach(() => {
  vi.stubGlobal("localStorage", memoryStorage());
  const root = document.documentElement;
  root.className = "";
  root.removeAttribute("data-palette");
  root.removeAttribute("data-accent");
  // jsdom has no matchMedia — stub a "prefers light" OS so `system` mode is
  // deterministic and applyAppearance never throws.
  vi.stubGlobal("matchMedia", () =>
    ({
      matches: false,
      addEventListener: () => {},
      removeEventListener: () => {},
    }) as unknown as MediaQueryList,
  );
});

test("theme mode: dark stamps the class and persists; system clears both", () => {
  setTheme("dark");
  expect(document.documentElement.classList.contains("dark")).toBe(true);
  expect(localStorage.getItem("cuaderno-theme")).toBe("dark");
  expect(storedTheme()).toBe("dark");

  setTheme("system");
  // matchMedia stub reports light, so system resolves to not-dark.
  expect(document.documentElement.classList.contains("dark")).toBe(false);
  expect(localStorage.getItem("cuaderno-theme")).toBeNull();
  expect(storedTheme()).toBe("system");
});

test("palette: a named palette sets the attribute; default removes it", () => {
  setPalette("warm");
  expect(document.documentElement.getAttribute("data-palette")).toBe("warm");
  expect(localStorage.getItem("cuaderno-palette")).toBe("warm");
  expect(storedPalette()).toBe("warm");

  setPalette("default");
  expect(document.documentElement.hasAttribute("data-palette")).toBe(false);
  expect(localStorage.getItem("cuaderno-palette")).toBeNull();
  expect(storedPalette()).toBe("default");
});

test("accent: a named accent sets the attribute; blue removes it", () => {
  setAccent("teal");
  expect(document.documentElement.getAttribute("data-accent")).toBe("teal");
  expect(localStorage.getItem("cuaderno-accent")).toBe("teal");
  expect(storedAccent()).toBe("teal");

  setAccent("blue");
  expect(document.documentElement.hasAttribute("data-accent")).toBe(false);
  expect(localStorage.getItem("cuaderno-accent")).toBeNull();
  expect(storedAccent()).toBe("blue");
});

test("a bogus stored value falls back to the default", () => {
  localStorage.setItem("cuaderno-theme", "chartreuse");
  localStorage.setItem("cuaderno-palette", "neon");
  localStorage.setItem("cuaderno-accent", "ultraviolet");
  expect(storedTheme()).toBe("system");
  expect(storedPalette()).toBe("default");
  expect(storedAccent()).toBe("blue");
});

test("applyAppearance reflects the current stored appearance", () => {
  localStorage.setItem("cuaderno-theme", "dark");
  localStorage.setItem("cuaderno-palette", "cool");
  localStorage.setItem("cuaderno-accent", "violet");

  applyAppearance();

  const root = document.documentElement;
  expect(root.classList.contains("dark")).toBe(true);
  expect(root.getAttribute("data-palette")).toBe("cool");
  expect(root.getAttribute("data-accent")).toBe("violet");
});
