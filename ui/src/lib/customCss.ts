// User custom stylesheet (`.cuaderno/custom.css`). The curated Appearance
// / Reading settings cover the common cases; this is the escape hatch for
// everything else. The file's contents are injected into a <style> element
// appended to <head> AFTER the app's bundled CSS, so any design token
// (accents, palettes, fonts, what "narrow" means, heading sizes, …) can be
// redefined in plain, hand-editable CSS living in the vault.
//
// Inline <style> is permitted by the app CSP (`style-src 'self'
// 'unsafe-inline'`). Reading happens through a Tauri command because the
// webview has no filesystem access by design.
import { readCustomCss } from "../api/commands";

const STYLE_ID = "cuaderno-custom-css";

/** Fetch `.cuaderno/custom.css` and (re-)apply it. Appended last so it
 * wins the cascade over the bundled styles. Best-effort: before the vault
 * is open (first-launch picker) the command errors — swallow it and leave
 * whatever is already applied, exactly like the other startup reads. */
export async function loadCustomCss(): Promise<void> {
  let css: string;
  try {
    css = await readCustomCss();
  } catch {
    return;
  }
  let style = document.getElementById(STYLE_ID) as HTMLStyleElement | null;
  if (!style) {
    style = document.createElement("style");
    style.id = STYLE_ID;
    // Append to <head> so it comes after Vite's injected styles (dev) and
    // the bundled stylesheet link (prod) — later in source order wins on
    // equal specificity.
    document.head.appendChild(style);
  }
  style.textContent = css;
}
