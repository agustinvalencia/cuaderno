use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use cdno_core::paths;

/// Default `config.toml` content written by `cdno init`. Embedded at
/// compile time so the binary needs no companion files at runtime.
const DEFAULT_CONFIG_TOML: &str = include_str!("../../templates/default_config.toml");

/// The `[note_types.concept]` declaration `cdno init` installs into a new
/// vault's config (RFC 0002 §6.1). `include_str!`ed from
/// `examples/note-types/concept/config.toml`, as the bundled registry's copy
/// is, so the example and the binary are one file.
pub const CONCEPT_TYPE_BLOCK: &str =
    include_str!("../../../../examples/note-types/concept/config.toml");

/// The concept template `cdno init` writes to `.cuaderno/templates/concept.md`
/// (RFC 0002 §5.2, §6.1), the file `CONCEPT_TYPE_BLOCK` names; likewise
/// `include_str!`ed from `examples/note-types/concept/concept.md`.
///
/// The frontmatter deliberately carries no `title` and no `origin`. The title
/// is the body H1 (§6.1), and a `title: {{title}}` line would break the YAML
/// for any title containing `: `. `origin` is added, quoted and in declared
/// order, by creation's frontmatter reconciliation only when one is supplied;
/// an `origin: "{{origin}}"` line would leave the placeholder as literal text
/// in every note created without one.
pub const CONCEPT_TEMPLATE: &str =
    include_str!("../../../../examples/note-types/concept/concept.md");

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

/// Initialise a Cuaderno vault rooted at `target`. The directory
/// itself is created if needed; refuses if `.cuaderno/` already
/// exists, since re-init is destructive.
///
/// Creates the layout, writes the default config and `daily.md`, then
/// installs every bundled note type through
/// [`crate::bundled::install_bundled`]. The config is therefore written
/// twice, once verbatim and once through the config gate's append; the
/// result is byte-identical to writing the default and the block together.
///
/// CWD-as-default lives in `main.rs`: this function takes whatever
/// path the caller resolved, so it never touches process-global
/// state and unit tests can run in parallel.
pub fn run(target: &Path) -> Result<()> {
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

    // The bundled types (RFC 0002's `concept`) go in through the same
    // function `cdno config note-type install` uses (RFC 0003 §4.4), so a new
    // vault and an upgraded one are identical by construction. The config is
    // therefore written twice: the default above through `fs::write`, then
    // the appended declaration through the validate-first config gate. That
    // is harmless. The default ends in a blank line, so the append adds no
    // separator and the bytes are exactly the default followed by the block.
    for bundled in crate::bundled::BUNDLED {
        crate::bundled::install_bundled(target, bundled.name)
            .with_context(|| format!("installing the bundled `{}` note type", bundled.name))?;
    }

    // Canonicalise for clarity in the success message; fall back to
    // the original path if the canonical form is unavailable (e.g. the
    // user supplied a path that points at a freshly created dir on a
    // case-insensitive filesystem).
    let display = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());
    println!("Initialised Cuaderno vault at {}", display.display());

    Ok(())
}
