use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use cdno_core::paths;

/// Default `config.toml` content written by `cdno init`. Embedded at
/// compile time so the binary needs no companion files at runtime.
const DEFAULT_CONFIG_TOML: &str = include_str!("../../templates/default_config.toml");

/// Default note templates dumped into `.cuaderno/templates/` at init.
///
/// Each entry is `(filename, content)`. The user can edit or delete
/// them — `TemplateEngine` only loads from `.cuaderno/templates/`, so
/// edits take effect immediately and deletions surface as
/// `TemplateError::NotFound` until either the user supplies their own
/// or the type's domain code adds an in-memory fallback.
///
/// Add to this list as Phase 2/3 note types gain concrete schemas.
const DEFAULT_TEMPLATES: &[(&str, &str)] =
    &[("daily.md", include_str!("../../templates/daily.md"))];

pub fn run(path: Option<&Path>) -> Result<()> {
    let target: PathBuf = match path {
        Some(p) => p.to_path_buf(),
        None => {
            std::env::current_dir().context("could not determine the current working directory")?
        }
    };

    // Refuse loudly rather than silently overwriting. Re-init is an
    // explicit destructive action — the user must remove `.cuaderno/`
    // by hand to opt in.
    let cuaderno_dir = target.join(paths::CUADERNO_DIR);
    if cuaderno_dir.exists() {
        bail!(
            "{} already exists; refusing to re-initialise. Remove it manually to start over.",
            cuaderno_dir.display()
        );
    }

    // Journal and `_done` subfolders are year-partitioned. Pre-create
    // the current year so the layout is visible on day one; later
    // years self-create on first write via `create_dir_all`.
    let today = chrono::Local::now().date_naive();
    for rel in paths::init_dirs(today) {
        let dir = target.join(&rel);
        fs::create_dir_all(&dir)
            .with_context(|| format!("creating directory {}", dir.display()))?;
    }

    let config_path = target.join(paths::CONFIG_FILE);
    fs::write(&config_path, DEFAULT_CONFIG_TOML)
        .with_context(|| format!("writing default config to {}", config_path.display()))?;

    let templates_dir = target.join(paths::TEMPLATES_DIR);
    for (filename, content) in DEFAULT_TEMPLATES {
        let dest = templates_dir.join(filename);
        fs::write(&dest, content)
            .with_context(|| format!("writing default template {}", dest.display()))?;
    }

    // Canonicalise for clarity in the success message; fall back to
    // the original path if the canonical form is unavailable (e.g. the
    // user supplied a path that points at a freshly created dir on a
    // case-insensitive filesystem).
    let display = target.canonicalize().unwrap_or(target);
    println!("Initialised Cuaderno vault at {}", display.display());

    Ok(())
}
