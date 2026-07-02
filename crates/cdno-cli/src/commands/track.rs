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

use cdno_domain::{StewardshipVariant, Vault};

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

    let path = vault
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

    // Point-of-use nudge (#282): when the note used the built-in generic
    // template (no custom variant/base override in the vault), tell the user
    // the ready-made structured templates exist. Suppressed under `--json`.
    if !json && let Some(hint) = generic_template_hint(root, &activity_variant) {
        println!("{hint}");
    }
    Ok(())
}

/// The hint to show when `cdno track` fell back to the built-in **generic**
/// tracking template — i.e. the vault has no custom `tracking-<slug>.md` (the
/// activity variant) and no custom `tracking.md` (base override), so the create
/// path resolved the plain generic shape. Returns `None` when a custom template
/// applied (mirrors the resolver's precedence: variant → base → built-in).
pub fn generic_template_hint(root: &Path, activity_slug: &str) -> Option<String> {
    let templates = root.join(cdno_core::paths::TEMPLATES_DIR);
    let has_custom = templates
        .join(format!("tracking-{activity_slug}.md"))
        .exists()
        || templates.join("tracking.md").exists();
    if has_custom {
        None
    } else {
        Some(format!(
            "  (generic template — copy an example from examples/templates/tracking/ into \
             .cuaderno/templates/tracking-{activity_slug}.md for a structured layout)"
        ))
    }
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
