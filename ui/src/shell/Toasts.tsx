// Minimal toast surface for command errors and confirmations. Calm
// by design law: no red, no shake — ink on a bordered surface, amber
// left-edge only for attention-tier messages, announced politely.
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";

export interface Toast {
  id: number;
  message: string;
  tone: "info" | "attention";
}

interface ToastApi {
  toast: (message: string, tone?: Toast["tone"]) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

const AUTO_DISMISS_MS = 6000;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(1);
  // Track pending auto-dismiss timers so an unmount mid-flight clears
  // them — otherwise the callback fires setToasts on a gone component.
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);

  const toast = useCallback((message: string, tone: Toast["tone"] = "info") => {
    const id = nextId.current++;
    setToasts((current) => [...current, { id, message, tone }]);
    const timer = setTimeout(() => {
      setToasts((current) => current.filter((t) => t.id !== id));
    }, AUTO_DISMISS_MS);
    timers.current.push(timer);
  }, []);

  useEffect(() => {
    const pending = timers.current;
    return () => {
      for (const timer of pending) clearTimeout(timer);
    };
  }, []);

  const api = useMemo(() => ({ toast }), [toast]);

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div
        aria-live="polite"
        className="pointer-events-none fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2"
      >
        {toasts.map(({ id, message, tone }) => (
          <div
            key={id}
            className={`pointer-events-auto rounded border border-line bg-bg-surface px-3 py-2 text-sm text-ink shadow-sm ${
              tone === "attention" ? "border-l-2 border-l-attention" : ""
            }`}
          >
            {message}
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastApi {
  const api = useContext(ToastContext);
  if (!api) {
    throw new Error("useToast requires a ToastProvider above it");
  }
  return api;
}
