import { resolve } from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Multi-page build: the main shell plus a deliberately tiny capture
// window entry (design §2.5 — the trust-critical capture path must
// not load, or fault-couple to, the full SPA).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        capture: resolve(__dirname, "capture.html"),
      },
    },
  },
  // Tauri dev expects a fixed port and no mid-run clears.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  test: {
    environment: "jsdom",
  },
});
