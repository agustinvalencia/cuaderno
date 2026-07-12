// The log-order preference store + the orderLogs helper.
import { afterEach, beforeAll, expect, test } from "vitest";
import { orderLogs, setLogOrder, toggleLogOrder } from "./logOrder";
// read() is exercised through set/toggle's persisted effect below.

// jsdom here lacks a working localStorage; give the store a real one.
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
  Object.defineProperty(globalThis, "localStorage", {
    value: local,
    configurable: true,
  });
});

afterEach(() => {
  // The store is stateless — read() hits localStorage fresh — so clearing
  // storage fully resets it between tests.
  localStorage.clear();
});

test("orderLogs keeps chronological input oldest-first, reverses for newest", () => {
  const entries = [1, 2, 3];
  expect(orderLogs(entries, "oldest")).toEqual([1, 2, 3]);
  expect(orderLogs(entries, "newest")).toEqual([3, 2, 1]);
});

test("orderLogs never mutates its input", () => {
  const entries = [1, 2, 3];
  orderLogs(entries, "newest");
  expect(entries).toEqual([1, 2, 3]);
});

test("setLogOrder persists newest and clears back to the default", () => {
  setLogOrder("newest");
  expect(localStorage.getItem("cuaderno-log-order")).toBe("newest");
  setLogOrder("oldest");
  // oldest is the default — stored as absence, not a value.
  expect(localStorage.getItem("cuaderno-log-order")).toBeNull();
});

test("toggleLogOrder flips between the two", () => {
  toggleLogOrder(); // oldest -> newest
  expect(localStorage.getItem("cuaderno-log-order")).toBe("newest");
  toggleLogOrder(); // newest -> oldest
  expect(localStorage.getItem("cuaderno-log-order")).toBeNull();
});
