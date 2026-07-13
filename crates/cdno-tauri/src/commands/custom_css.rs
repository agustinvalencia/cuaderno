//! User custom stylesheet (`.cuaderno/custom.css`).
//!
//! The curated Appearance/Reading settings expose a fixed set of choices;
//! this is the escape hatch for everything else. The frontend injects this
//! file's contents into a `<style>` after the bundled CSS, so any design
//! token — accent colours, what "narrow" means, heading sizes, the mono
//! face, whole palettes — can be redefined by editing plain CSS in the
//! vault, next to `config.toml`. The vault stays the source of truth, and
//! the customisation is hand-editable, exactly like the rest of the vault.
//!
//! Two commands, both sync (the file is tiny): `read_custom_css` for
//! injection, `open_custom_css` to hand it to the user's editor (seeding a
//! documented template the first time so there's a starting point).

use std::path::{Path, PathBuf};

use tauri_plugin_opener::OpenerExt;

use cdno_core::paths::CUADERNO_DIR;

use crate::error::CmdError;
use crate::state::AppState;

const CUSTOM_CSS_FILE: &str = "custom.css";

fn custom_css_path(root: &Path) -> PathBuf {
    root.join(CUADERNO_DIR).join(CUSTOM_CSS_FILE)
}

/// Create the file (seeding the documented template) if it doesn't exist,
/// so both "Edit in app" and "Edit in editor" open a real starting point
/// rather than nothing. Idempotent — a no-op once the file is present.
fn ensure_seeded(path: &Path) -> Result<(), CmdError> {
    if path.exists() {
        return Ok(());
    }
    // The `.cuaderno` dir exists for any opened vault, but create it
    // defensively rather than fail if it somehow doesn't.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            tracing::warn!(error = %err, "could not create .cuaderno for custom.css");
            CmdError::Internal("could not create the .cuaderno directory".to_owned())
        })?;
    }
    std::fs::write(path, CUSTOM_CSS_TEMPLATE).map_err(|err| {
        tracing::warn!(error = %err, "could not seed custom.css");
        CmdError::Internal("could not create custom.css".to_owned())
    })
}

/// Read `.cuaderno/custom.css`, or return an empty string when it doesn't
/// exist yet (the common case — no custom styling). A read error other
/// than "not found" is surfaced generically; the app is fully usable
/// without a custom stylesheet, so it must never be fatal.
#[tauri::command]
pub fn read_custom_css(state: tauri::State<'_, AppState>) -> Result<String, CmdError> {
    let path = custom_css_path(&state.root);
    match std::fs::read_to_string(&path) {
        Ok(css) => Ok(css),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => {
            tracing::warn!(error = %err, "failed to read custom.css");
            Err(CmdError::Internal("could not read custom.css".to_owned()))
        }
    }
}

/// Ensure `.cuaderno/custom.css` exists (seeding the template the first
/// time), then open it in the user's default editor — the "Edit in editor"
/// action.
#[tauri::command]
pub fn open_custom_css<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<(), CmdError> {
    let path = custom_css_path(&state.root);
    ensure_seeded(&path)?;
    // No confinement dance (unlike open_in_editor): this path is built
    // entirely from state.root plus the CUADERNO_DIR/custom.css constants,
    // with zero frontend input, so there is no traversal vector to guard.
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|err| {
            tracing::error!(error = %err, "failed to open custom.css in an editor");
            CmdError::Internal("could not open custom.css in an editor".to_owned())
        })?;
    Ok(())
}

/// Ensure `.cuaderno/custom.css` exists (seeding the template the first
/// time) and return its contents — the "Edit in app" action's loader, so
/// the in-app editor opens on the documented template rather than blank.
#[tauri::command]
pub fn init_custom_css(state: tauri::State<'_, AppState>) -> Result<String, CmdError> {
    let path = custom_css_path(&state.root);
    ensure_seeded(&path)?;
    std::fs::read_to_string(&path).map_err(|err| {
        tracing::warn!(error = %err, "failed to read custom.css after seeding");
        CmdError::Internal("could not read custom.css".to_owned())
    })
}

/// Overwrite `.cuaderno/custom.css` with `content` — the in-app editor's
/// save. Creates the `.cuaderno` dir defensively; the frontend re-injects
/// the stylesheet after this resolves.
#[tauri::command]
pub fn write_custom_css(
    state: tauri::State<'_, AppState>,
    content: String,
) -> Result<(), CmdError> {
    let path = custom_css_path(&state.root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            tracing::warn!(error = %err, "could not create .cuaderno for custom.css");
            CmdError::Internal("could not create the .cuaderno directory".to_owned())
        })?;
    }
    std::fs::write(&path, content).map_err(|err| {
        tracing::warn!(error = %err, "could not write custom.css");
        CmdError::Internal("could not write custom.css".to_owned())
    })
}

/// The first-run template: a documented map of the overridable tokens, all
/// commented out so a fresh file is a no-op until the user opts in.
const CUSTOM_CSS_TEMPLATE: &str = r#"/* cuaderno custom stylesheet
 *
 * Loaded after the app's own styles, so anything here overrides the
 * built-in theme. Everything is a CSS variable on :root — redefine the
 * ones you care about. The Settings panel (Cmd+,) still drives the named
 * choices; these let you redefine what those choices mean.
 *
 * Changes apply when you refocus the app window (or hit Reload in
 * Settings). Delete a rule to fall back to the default.
 */

:root {
  /* --- Fonts -------------------------------------------------------- */
  /* --font-sans: "Inter", -apple-system, sans-serif; */
  /* --font-serif: "Iowan Old Style", Georgia, serif; */
  /* --font-mono: "JetBrains Mono", ui-monospace, monospace; */

  /* --- Accent colours (the swatches in Settings > Accent) ----------- */
  /* Any CSS colour works; oklch() keeps them tonally even. */
  /* --accent-blue: oklch(0.56 0.14 256); */
  /* --accent-teal: #1a8f8f; */

  /* --- Reader typography -------------------------------------------- */
  /* Heading sizes are relative to the body size (em). */
  /* --reader-heading-1: 1.5em; */
  /* --reader-heading-2: 1.25em; */
  /* --reader-heading-3: 1.1em; */
  /* --reader-line-height: 1.7; */

  /* --- Layout ------------------------------------------------------- */
  /* --sidebar-width: 16rem; */
  /* --titlebar-height: 3rem; */
  /* --sidebar-vibrancy: 60%; */ /* lower = more see-through */
}

/* Redefine what a named setting means. These attribute selectors match
 * the choice you pick in Settings, so e.g. your "Narrow" reading width: */
/* :root[data-reading-width="narrow"] { --reader-measure: 50ch; } */
/* :root[data-text-size="large"]      { --reader-font-size: 1.125rem; } */

/* Override a palette in a specific mode (append .dark for dark mode): */
/* :root[data-palette="warm"]      { --color-bg-base: oklch(0.97 0.02 80); } */
/* :root[data-palette="warm"].dark { --color-bg-base: oklch(0.24 0.02 70); } */
"#;
