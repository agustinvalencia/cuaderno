// Capture-bar behaviour: Enter captures to inbox, Cmd+Enter logs to
// today. The window/event Tauri APIs run over mockIPC (+ mockWindows
// so getCurrentWindow resolves); the assertions pin the command name
// and the argument key marshalled across the bridge.
import { afterEach, expect, test, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import CaptureBar from "./CaptureBar";

// The confirm flash auto-hides after this long — kept in sync with the
// component's CONFIRM_MS (not exported, so mirrored here).
const CONFIRM_MS = 900;

// Mock the window and event JS modules (not the core invoke seam, which
// stays on mockIPC): this makes `hide` a spy we can assert on, and lets
// a test reach the `capture:show` handler directly rather than plumbing
// a real event through the mocked IPC internals. `vi.hoisted` because
// `vi.mock` factories are hoisted above the imports.
const { hide, captureShowHandlers } = vi.hoisted(() => ({
  hide: vi.fn(),
  captureShowHandlers: [] as Array<(event: unknown) => void>,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ hide }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: (event: unknown) => void) => {
    if (event === "capture:show") captureShowHandlers.push(handler);
    return Promise.resolve(() => {
      const i = captureShowHandlers.indexOf(handler);
      if (i >= 0) captureShowHandlers.splice(i, 1);
    });
  },
}));

/** Fire the global-hotkey re-summon the component subscribes to. */
function emitCaptureShow() {
  for (const handler of [...captureShowHandlers]) {
    handler({ event: "capture:show", id: 0, payload: null });
  }
}

afterEach(() => {
  cleanup();
  clearMocks();
  hide.mockClear();
  captureShowHandlers.length = 0;
});

test("Enter captures the typed text to the inbox", async () => {
  mockWindows("capture");
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    // capture_quick and the plugin:event/window calls all resolve void.
    return undefined;
  });

  render(<CaptureBar />);
  const input = await screen.findByLabelText("Quick capture");
  fireEvent.change(input, { target: { value: "buy milk" } });
  fireEvent.keyDown(input, { key: "Enter" });

  expect(await screen.findByText("captured")).toBeDefined();
  const captured = calls.find((c) => c.cmd === "capture_quick");
  expect(captured?.args).toMatchObject({ text: "buy milk" });
});

test("Cmd+Enter logs the typed text to today", async () => {
  mockWindows("capture");
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return undefined;
  });

  render(<CaptureBar />);
  const input = await screen.findByLabelText("Quick capture");
  fireEvent.change(input, { target: { value: "a passing thought" } });
  fireEvent.keyDown(input, { key: "Enter", metaKey: true });

  expect(await screen.findByText("logged")).toBeDefined();
  const logged = calls.find((c) => c.cmd === "log_quick");
  expect(logged?.args).toMatchObject({ text: "a passing thought" });
});

test("blank input does not fire a capture", async () => {
  mockWindows("capture");
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    return undefined;
  });

  render(<CaptureBar />);
  const input = await screen.findByLabelText("Quick capture");
  fireEvent.keyDown(input, { key: "Enter" });

  expect(calls).not.toContain("capture_quick");
});

test("a rejected capture keeps the text and shows the error, without hiding", async () => {
  mockWindows("capture");
  mockIPC((cmd) => {
    // A rejected promise (not a thrown value) so the mocked invoke
    // rejects rather than throwing synchronously.
    if (cmd === "capture_quick") return Promise.reject(new Error("disk full"));
    return undefined;
  });

  render(<CaptureBar />);
  const input = (await screen.findByLabelText("Quick capture")) as HTMLInputElement;
  fireEvent.change(input, { target: { value: "do not lose me" } });
  fireEvent.keyDown(input, { key: "Enter" });

  // The failure is announced inline, the words are preserved, and the
  // window is never hidden out from under the unsaved text.
  expect(await screen.findByText("disk full")).toBeDefined();
  expect(input.value).toBe("do not lose me");
  expect(hide).not.toHaveBeenCalled();
});

test("re-summoning clears a stale confirm timer so the next thought isn't hidden", async () => {
  vi.useFakeTimers();
  try {
    mockWindows("capture");
    mockIPC(() => undefined);

    render(<CaptureBar />);
    const input = screen.getByLabelText("Quick capture") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "first" } });
    fireEvent.keyDown(input, { key: "Enter" });
    // Flush the capture_quick microtask so the 900ms confirm timer arms.
    await vi.advanceTimersByTimeAsync(0);
    expect(screen.getByText("captured")).toBeDefined();

    // The hotkey re-summons the window mid-flash; the user keeps typing.
    act(() => emitCaptureShow());
    fireEvent.change(input, { target: { value: "second thought" } });

    // Past the old confirm horizon: the superseded timer must have been
    // cleared, so the window was not hidden mid-thought.
    await vi.advanceTimersByTimeAsync(CONFIRM_MS + 50);
    expect(hide).not.toHaveBeenCalled();
    expect(input.value).toBe("second thought");
  } finally {
    vi.useRealTimers();
  }
});

test("two rapid Enters capture the text only once", async () => {
  mockWindows("capture");
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    return undefined;
  });

  render(<CaptureBar />);
  const input = await screen.findByLabelText("Quick capture");
  fireEvent.change(input, { target: { value: "only once" } });
  // Two keydowns before the first write settles: the in-flight guard
  // must collapse them to a single capture.
  fireEvent.keyDown(input, { key: "Enter" });
  fireEvent.keyDown(input, { key: "Enter" });

  await screen.findByText("captured");
  expect(calls.filter((c) => c === "capture_quick")).toHaveLength(1);
});
