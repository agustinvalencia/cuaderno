// The floating capture bar (plan §3.6). A single input: Enter captures
// to inbox, Cmd/Ctrl+Enter logs to today, Escape (or losing focus)
// hides the window. Deliberately imports only the command seam, the
// Tauri window/event APIs, and token CSS — never the SPA, router, or
// TanStack — so the trust-critical capture path can't fault-couple to
// the main app (design §2.5).
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { captureQuick, errorMessage, logQuick } from "../../api/commands";

// The "captured"/"logged" flash lingers this long, then the window
// auto-hides — long enough to register, short enough to stay out of
// the way.
const CONFIRM_MS = 900;
// Grace period after a blur before hiding, so the programmatic refocus
// on `capture:show` (which briefly blurs the input) can't slam the
// window shut mid-summon.
const BLUR_HIDE_MS = 150;

type Status =
  | { kind: "idle" }
  | { kind: "done"; message: string }
  | { kind: "error"; message: string };

export default function CaptureBar() {
  const inputRef = useRef<HTMLInputElement>(null);
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  // Pending timers, cleared on unmount or when superseded.
  const blurTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const confirmTimer = useRef<ReturnType<typeof setTimeout>>(undefined);

  const focusInput = useCallback(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const hide = useCallback(() => {
    void getCurrentWindow().hide();
  }, []);

  // Autofocus on mount, and re-focus (from a clean slate) every time
  // the global hotkey re-summons an already-open window.
  useEffect(() => {
    focusInput();
    let unlisten: (() => void) | undefined;
    void listen("capture:show", () => {
      setStatus({ kind: "idle" });
      if (inputRef.current) inputRef.current.value = "";
      focusInput();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
      clearTimeout(blurTimer.current);
      clearTimeout(confirmTimer.current);
    };
  }, [focusInput]);

  const send = useCallback(
    async (verb: "capture" | "log") => {
      const input = inputRef.current;
      const text = input?.value.trim() ?? "";
      if (!text) return;
      try {
        await (verb === "log" ? logQuick(text) : captureQuick(text));
        if (input) input.value = "";
        setStatus({ kind: "done", message: verb === "log" ? "logged" : "captured" });
        clearTimeout(confirmTimer.current);
        confirmTimer.current = setTimeout(() => {
          setStatus({ kind: "idle" });
          hide();
        }, CONFIRM_MS);
      } catch (error) {
        // The write failed — keep the text so it isn't lost, and show
        // the reason inline (muted, never alarming).
        setStatus({ kind: "error", message: errorMessage(error) });
      }
    },
    [hide],
  );

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      hide();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      void send(event.metaKey || event.ctrlKey ? "log" : "capture");
    }
  };

  return (
    <div className="flex h-screen flex-col justify-center rounded-xl border border-line bg-bg-surface px-4 py-3 shadow-sm">
      <input
        ref={inputRef}
        type="text"
        aria-label="Quick capture"
        placeholder="Capture a thought…"
        onKeyDown={onKeyDown}
        onFocus={() => clearTimeout(blurTimer.current)}
        onBlur={() => {
          clearTimeout(blurTimer.current);
          blurTimer.current = setTimeout(hide, BLUR_HIDE_MS);
        }}
        className="w-full bg-transparent text-base text-ink outline-none placeholder:text-ink-faint"
      />
      <div className="mt-1 flex items-center justify-between text-xs">
        <span className="text-ink-faint">Enter captures to inbox · Cmd+Enter logs to today</span>
        {/* Stable live region so the flash / error is announced. */}
        <span aria-live="polite" className="text-ink-muted">
          {status.kind === "idle" ? "" : status.message}
        </span>
      </div>
    </div>
  );
}
