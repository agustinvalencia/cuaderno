//! `cdno track` — file a tracking note under an expanded
//! stewardship.
//!
//! Top-level rather than a `cdno stewardship` subcommand: logging an
//! activity is a routine action a user takes several times a week,
//! while stewardship setup is meta. Mirrors the `cdno file` / `cdno
//! portfolio` split.
//!
//! `<activity>` is positional and unprompted — typing `cdno track
//! gym` is the shortest path to capture. `--stewardship` defaults to
//! the only expanded stewardship when there's exactly one; otherwise
//! it's required (errored in non-interactive, prompted in a TTY).

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDateTime;

use cdno_domain::{StewardshipVariant, TemplateSource, Vault};

use crate::bootstrap;
use crate::prompt;

#[allow(clippy::too_many_arguments)] // thin CLI gather→confirm→execute passthrough
pub fn run(
    root: &Path,
    at: NaiveDateTime,
    activity: String,
    stewardship: Option<String>,
    routine: Option<String>,
    content: String,
    var: Vec<(String, String)>,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    // `--json` implies non-interactive: prompts/confirms print to stdout,
    // which would corrupt the JSON result. Scripted callers pass full args.
    let interactive = prompt::is_interactive(no_interactive || json);

    // Resolve --stewardship. Three branches: explicit, exactly-one
    // expanded stewardship in the vault (default-to-that ergonomic),
    // or pick / error.
    let mut prompted = false;
    let stewardship = match stewardship {
        Some(s) => s,
        None => match default_expanded_stewardship(&vault, at)? {
            Some(s) => s,
            None => {
                if interactive {
                    prompted = true;
                    prompt::prompt_expanded_stewardship(&vault, at.date())?
                } else {
                    return Err(prompt::missing_flag("stewardship"));
                }
            }
        },
    };
    // routine and content stay genuinely optional.
    // The variant is the activity *slug* — mirror the domain's slugify so
    // `template_prompts` resolves the same variant template `scaffold` will
    // (e.g. `cdno track "weight training"` → `tracking-weight-training`),
    // rather than the generic fallback. Otherwise a variant-only prompt var
    // would be missed here and then hard-error at scaffold time.
    let activity_variant = cdno_domain::slugify(&activity);
    let template_vars = prompt::gather_template_vars(
        &vault,
        "tracking",
        Some(&activity_variant),
        &var,
        interactive,
        &mut prompted,
    )?;

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to file tracking entry:\n  stewardship: {stewardship}\n  activity:    {activity}\n  routine:     {}",
            routine.as_deref().unwrap_or("(none)")
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    let (path, source) = vault
        .add_tracking_entry_with_vars(
            at,
            &stewardship,
            &activity,
            routine.as_deref(),
            &content,
            &template_vars,
        )
        .context("filing tracking entry")?;
    crate::output::emit_write_result(json, &path.to_string(), &format!("Tracked at {path}"))?;

    // Point-of-use nudge (#282): a one-time-ish discovery hint for the
    // ready-made structured templates. Suppressed under `--json`, and printed
    // to stderr so it never pollutes a human-mode capture of the result line.
    //
    // Gated on the domain's actual resolution (#287): the note used the generic
    // built-in iff `source` is `BuiltinDefault`. This replaces re-deriving that
    // from the vault filesystem — airtight if a `tracking-<variant>` default is
    // ever bundled (it'd report `BuiltinVariant`, so no false nudge).
    if !json
        && source == TemplateSource::BuiltinDefault
        && let Some(hint) = newcomer_template_hint(root, &activity_variant)
    {
        eprintln!("{hint}");
    }
    Ok(())
}

/// The newcomer discovery hint for the ready-made structured templates, shown
/// after a `cdno track` that rendered the built-in generic (the caller gates on
/// [`TemplateSource::BuiltinDefault`], so *that* decision is the domain's, not
/// this function's — #287).
///
/// Returns `None` once the vault has *any* tracking template — a user who has
/// authored one already knows the mechanism, so the nudge would just be noise on
/// this high-frequency command; it's only for the newcomer who has never
/// customised tracking. This is a UX gate ("has the user ever customised
/// tracking"), distinct from the per-entry resolution the caller already has.
pub fn newcomer_template_hint(root: &Path, activity_slug: &str) -> Option<String> {
    if vault_has_a_tracking_template(&root.join(cdno_core::paths::TEMPLATES_DIR)) {
        return None;
    }
    Some(format!(
        "  (generic template — copy an example from the cuaderno repo's \
         examples/templates/tracking/ into .cuaderno/templates/tracking-{activity_slug}.md \
         for a structured layout)"
    ))
}

/// Whether the vault has any tracking template — a base `tracking.md` or any
/// `tracking-<variant>.md`.
fn vault_has_a_tracking_template(templates_dir: &Path) -> bool {
    if templates_dir.join("tracking.md").exists() {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(templates_dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name();
        let name = name.to_string_lossy();
        name.starts_with("tracking-") && name.ends_with(".md")
    })
}

/// Return the only expanded stewardship in the vault, when there's
/// exactly one — the ergonomic default for `cdno track`. Returns
/// `None` for zero or more-than-one (caller decides whether to
/// prompt or error).
fn default_expanded_stewardship(vault: &Vault, at: NaiveDateTime) -> Result<Option<String>> {
    let summaries = vault.list_stewardships(at.date())?;
    let mut expanded = summaries
        .into_iter()
        .filter(|s| s.variant == StewardshipVariant::Expanded);
    let first = expanded.next();
    if first.is_some() && expanded.next().is_some() {
        return Ok(None);
    }
    Ok(first.map(|s| s.slug))
}
