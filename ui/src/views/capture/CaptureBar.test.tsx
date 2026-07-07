// Capture-bar behaviour: Enter captures to inbox, Cmd+Enter logs to
// today. The window/event Tauri APIs run over mockIPC (+ mockWindows
// so getCurrentWindow resolves); the assertions pin the command name
// and the argument key marshalled across the bridge.
import { afterEach, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import CaptureBar from "./CaptureBar";

afterEach(() => {
  cleanup();
  clearMocks();
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
