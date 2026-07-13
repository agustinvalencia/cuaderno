import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { attachEventBridge } from "./api/events";
import { initTheme } from "./lib/theme";
import { loadCustomCss } from "./lib/customCss";
import { setWatcherState } from "./lib/watcherStatus";
import { ToastProvider } from "./shell/Toasts";
import App from "./App";
import "./styles/globals.css";

initTheme();

// Apply the user's `.cuaderno/custom.css` (the override escape hatch), then
// re-apply on window focus — the same signal the query cache uses — so an
// edit to the file takes effect when the user Cmd-Tabs back from their
// editor, no relaunch needed.
void loadCustomCss();
window.addEventListener("focus", () => void loadCustomCss());

// Cache posture (plan §2.5): events are the primary invalidation
// source; staleness never expires on its own. refetchOnWindowFocus
// stays ON as the backstop for silently dropped filesystem events —
// it maps exactly onto Cmd-Tabbing back from the editor.
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: Infinity,
      refetchOnWindowFocus: true,
      retry: 1,
    },
  },
});

// Listeners attach BEFORE the first render (and therefore before the
// first fetch); attachEventBridge ends with a global invalidation to
// cover anything emitted earlier. In a plain browser tab (vite dev
// without Tauri) the bridge is absent — render anyway.
// watcher:status lands in the module store; the shell's WatcherPill
// reads it reactively (grey pill + 60s poll fallback while degraded).
attachEventBridge(queryClient, (status) => setWatcherState(status.state))
  .catch((error) => {
    // Absent bridge is normal in a plain browser tab; a failure
    // inside Tauri (capability regression) must at least be loud in
    // the devtools console.
    console.warn("event bridge not attached; relying on focus refetch", error);
  })
  .finally(() => {
    createRoot(document.getElementById("root")!).render(
      <StrictMode>
        <QueryClientProvider client={queryClient}>
          <ToastProvider>
            <BrowserRouter>
              <App />
            </BrowserRouter>
          </ToastProvider>
        </QueryClientProvider>
      </StrictMode>,
    );
  });
