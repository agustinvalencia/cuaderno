// Separate minimal entry for the capture window (plan §2.5): imports
// only the command seam and the token CSS — never the SPA. The real
// capture UI lands in M3; this placeholder keeps the multi-page build
// and the window wiring honest from day one.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { initTheme } from "../../lib/theme";
import "../../styles/globals.css";

initTheme();

function CaptureBar() {
  return (
    <div className="flex h-screen items-center rounded-xl border border-line bg-bg-surface px-4">
      <p className="text-sm text-ink-muted">Quick capture arrives in M3.</p>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <CaptureBar />
  </StrictMode>,
);
