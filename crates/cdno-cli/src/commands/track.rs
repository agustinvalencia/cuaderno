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

pub fn run(
    root: &Path,
    at: NaiveDateTime,
    activity: String,
    stewardship: Option<String>,
    routine: Option<String>,
    content: String,
    no_interactive: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let interactive = prompt::is_interactive(no_interactive);

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
        .add_tracking_entry(at, &stewardship, &activity, routine.as_deref(), &content)
        .context("filing tracking entry")?;
    println!("Tracked at {path}");
    Ok(())
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
