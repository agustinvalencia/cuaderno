//! Interactive prompts for CLI ergonomics — fuzzy selectors, text
//! input, and y/n confirms applied when a required arg is missing and
//! `stdout` is a TTY.
//!
//! Convention (see `docs/cli-ergonomics.md`):
//!
//! 1. Promptable args are declared `Option<T>` in clap.
//! 2. The handler folds each `Option` with [`is_interactive`] and
//!    [`missing_flag`]: `Some` → use; `None` + interactive → prompt;
//!    `None` + non-interactive → error.
//! 3. If any field was prompted, the handler renders a preview and
//!    calls [`confirm_preview`] before executing; all-Some runs
//!    straight through, matching the agentic (MCP / Tauri) shape.
//!
//! Only the helpers the action commands use ship in this PR; date /
//! status / milestone prompts arrive with the project / commit
//! retrofit (#114).

use std::io::IsTerminal;

use anyhow::{Result, anyhow};
use inquire::{Confirm, Select, Text};

use cdno_domain::Vault;
use cdno_domain::frontmatter::EnergyLevel;

/// `true` when interactive prompts are available — `stdout` is a TTY
/// **and** the user hasn't opted out via `--no-interactive`. Handlers
/// use this to decide whether a missing `Option` argument turns into a
/// prompt or an error.
pub fn is_interactive(no_interactive: bool) -> bool {
    !no_interactive && std::io::stdout().is_terminal()
}

/// Build a clear "missing flag" error for the non-interactive path so
/// piped / scripted invocations fail fast rather than hanging.
pub fn missing_flag(flag: &str) -> anyhow::Error {
    anyhow!("missing required flag: --{flag} (provide it explicitly or run interactively in a TTY)")
}

/// Fuzzy-pick an active project. Returns the project slug.
///
/// Errors if there are no active projects — the user can't pick
/// nothing, and silently bailing would mask the real problem.
pub fn prompt_project(vault: &Vault) -> Result<String> {
    let active = vault.active_projects()?;
    if active.is_empty() {
        return Err(anyhow!(
            "no active projects — create one with `cdno project create`",
        ));
    }
    let labels: Vec<String> = active
        .iter()
        .map(|(path, fm)| {
            let slug = path
                .as_path()
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            format!("{slug} ({})", fm.context.as_str())
        })
        .collect();
    let pick = Select::new("Project", labels.clone()).prompt()?;
    let idx = labels
        .iter()
        .position(|l| l == &pick)
        .expect("picked label was in the offered list");
    let (path, _) = &active[idx];
    Ok(path
        .as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_owned())
}

/// Plain text input with `prompt` as the displayed label.
pub fn prompt_text(prompt: &str) -> Result<String> {
    Ok(Text::new(prompt).prompt()?)
}

/// Three-option energy selector matching the bullet suffix vocabulary.
pub fn prompt_energy() -> Result<EnergyLevel> {
    let choice = Select::new("Energy", vec!["deep", "medium", "light"]).prompt()?;
    Ok(match choice {
        "deep" => EnergyLevel::Deep,
        "medium" => EnergyLevel::Medium,
        "light" => EnergyLevel::Light,
        _ => unreachable!("Select only offers the three listed labels"),
    })
}

/// Y/N confirm with a default.
pub fn prompt_confirm(prompt: &str, default: bool) -> Result<bool> {
    Ok(Confirm::new(prompt).with_default(default).prompt()?)
}

/// Fuzzy-pick a bullet from a project's `## Next Actions` list. The
/// selected entry's `text` is returned so the domain's substring
/// matching resolves it uniquely on the next call.
///
/// `labels` should already include the energy suffix and any wikilink
/// — typically the `text` field of each [`cdno_domain::ActionListEntry`].
pub fn prompt_bullet(project: &str, labels: &[String]) -> Result<String> {
    if labels.is_empty() {
        return Err(anyhow!("no open actions on project '{project}'"));
    }
    Ok(Select::new("Bullet", labels.to_vec()).prompt()?)
}

/// Print a preview block and confirm before committing. Returns
/// `false` when the user declines (the caller should abort cleanly,
/// not error). Wraps inquire's confirm so the call site stays compact.
pub fn confirm_preview(preview: &str) -> Result<bool> {
    println!("{preview}");
    Ok(Confirm::new("Proceed?").with_default(true).prompt()?)
}
