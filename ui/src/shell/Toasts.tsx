// Minimal toast surface for command errors and confirmations. Calm
// by design law: no red, no shake — ink on a bordered surface, amber
// left-edge only for attention-tier messages, announced politely.
import { createContext, useCallback, useContext, useMemo, useRef, useState } from "react";
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

  const toast = useCallback((message: string, tone: Toast["tone"] = "info") => {
    const id = nextId.current++;
    setToasts((current) => [...current, { id, message, tone }]);
    setTimeout(() => {
      setToasts((current) => current.filter((t) => t.id !== id));
    }, AUTO_DISMISS_MS);
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
