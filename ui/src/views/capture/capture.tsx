// Minimal entry for the capture window (plan §2.5): mounts only the
// capture bar and the token CSS — never the SPA, router, or TanStack.
// The trust-critical capture path must not load, or fault-couple to,
// the full app.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { initTheme } from "../../lib/theme";
import CaptureBar from "./CaptureBar";
import "../../styles/globals.css";

initTheme();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <CaptureBar />
  </StrictMode>,
);
