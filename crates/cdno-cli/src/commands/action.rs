//! `cdno action` subcommands: the user-facing surface for the action
//! layer. `add`, `promote`, and `complete` are thin clap-to-domain
//! shims; `list` reads `Vault::list_actions` and formats the bullets
//! with their attached-note status inline.
//!
//! Promptable fields are declared `Option<T>`. In a TTY (and unless
//! `--no-interactive` is set) a missing field is gathered via the
//! `prompt` module; in non-interactive sessions missing fields error
//! with a clear "missing --flag" message. The handler tracks whether
//! anything was prompted and renders a preview-and-confirm step in
//! that case, matching the design's "confirm-on-prompt only" rule.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use clap::Subcommand;
use clap_complete::engine::ArgValueCompleter;

use cdno_domain::frontmatter::{ActionStatus, EnergyLevel};
use cdno_domain::{ActionListEntry, AttachedAction, Vault};

use crate::bootstrap;
use crate::completions;
use crate::prompt;

#[derive(Debug, Subcommand)]
pub enum ActionCommands {
    /// Append a next action to a project. `--note` creates a manifest
    /// note alongside the bullet and wikilinks it.
    Add {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        project: Option<String>,
        /// Action title.
        #[arg(long)]
        title: Option<String>,
        /// Energy bucket: deep, medium, or light.
        #[arg(long)]
        energy: Option<EnergyLevel>,
        /// Promote on creation: also write an action note and wikilink
        /// the bullet to it.
        #[arg(long)]
        note: bool,
    },

    /// Promote an existing plain bullet to a wikilinked manifest note.
    /// Substring-matches the bullet text; energy is inherited.
    Promote {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        project: Option<String>,
        /// Substring matching the bullet to promote.
        #[arg(long)]
        query: Option<String>,
    },

    /// Mark a next action as completed by case-insensitive substring
    /// match. A wikilinked bullet also archives its note to
    /// `actions/_done/<year>/`.
    Complete {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        project: Option<String>,
        /// Substring matching the action to complete.
        #[arg(long)]
        query: Option<String>,
    },

    /// List a project's open action bullets, with the attached-note
    /// status (active / blocked / completed) inline when present.
    List {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        project: Option<String>,
    },
}

pub fn run(
    root: &Path,
    at: NaiveDateTime,
    command: ActionCommands,
    no_interactive: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let interactive = prompt::is_interactive(no_interactive);

    match command {
        ActionCommands::Add {
            project,
            title,
            energy,
            note,
        } => add(&vault, at, project, title, energy, note, interactive),
        ActionCommands::Promote { project, query } => {
            promote(&vault, at, project, query, interactive)
        }
        ActionCommands::Complete { project, query } => {
            complete(&vault, at, project, query, interactive)
        }
        ActionCommands::List { project } => list(&vault, project, interactive),
    }
}

// ---------------------------------------------------------------------
// Per-verb handlers — gather missing fields, confirm-on-prompt, execute.
// ---------------------------------------------------------------------

fn add(
    vault: &Vault,
    at: NaiveDateTime,
    project: Option<String>,
    title: Option<String>,
    energy: Option<EnergyLevel>,
    note_flag: bool,
    interactive: bool,
) -> Result<()> {
    let mut prompted = false;
    let project = prompt::gather_or_error(project, "project", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let title = prompt::gather_or_error(title, "title", interactive, &mut prompted, || {
        prompt::prompt_text("Title")
    })?;
    let energy = prompt::gather_or_error(energy, "energy", interactive, &mut prompted, || {
        prompt::prompt_energy()
    })?;
    // Only ask about --note when we're already in an interactive flow.
    // A user who provided every other flag and omitted --note clearly
    // wants the default (plain bullet).
    let note = if prompted {
        prompt::prompt_confirm("Promote on creation? (writes an action note)", note_flag)?
    } else {
        note_flag
    };

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to add to project '{project}':\n  title:  {title}\n  energy: {}\n  note:   {}",
            energy.as_str(),
            yesno(note),
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    if note {
        let path = vault
            .add_action_with_note(at, &project, &title, energy)
            .context("adding action with note")?;
        println!("Action added to projects/{project}.md with note {path}");
    } else {
        let path = vault
            .add_action(at, &project, &title, energy)
            .context("adding action")?;
        println!("Action added to {path}");
    }
    Ok(())
}

fn promote(
    vault: &Vault,
    at: NaiveDateTime,
    project: Option<String>,
    query: Option<String>,
    interactive: bool,
) -> Result<()> {
    let mut prompted = false;
    let project = prompt::gather_or_error(project, "project", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let query = prompt::gather_or_error(query, "query", interactive, &mut prompted, || {
        let entries = vault
            .list_actions(&project)
            .context("listing actions for the bullet picker")?;
        let labels: Vec<String> = entries.iter().map(|e| e.text.clone()).collect();
        let picked = prompt::prompt_bullet(&project, &labels)?;
        Ok(strip_energy_for_query(&picked))
    })?;

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to promote action on '{project}': '{query}'"
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    let note_path = vault
        .promote_action(at, &project, &query)
        .context("promoting action")?;
    println!("Promoted to {note_path}");
    Ok(())
}

fn complete(
    vault: &Vault,
    at: NaiveDateTime,
    project: Option<String>,
    query: Option<String>,
    interactive: bool,
) -> Result<()> {
    let mut prompted = false;
    let project = prompt::gather_or_error(project, "project", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let query = prompt::gather_or_error(query, "query", interactive, &mut prompted, || {
        let entries = vault
            .list_actions(&project)
            .context("listing actions for the bullet picker")?;
        let labels: Vec<String> = entries.iter().map(|e| e.text.clone()).collect();
        let picked = prompt::prompt_bullet(&project, &labels)?;
        Ok(strip_energy_for_query(&picked))
    })?;

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to complete action on '{project}': '{query}'"
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    let project_path = vault
        .complete_action(at, &project, &query)
        .context("completing action")?;
    println!("Action done on {project_path}");
    Ok(())
}

fn list(vault: &Vault, project: Option<String>, interactive: bool) -> Result<()> {
    // List is read-only — no confirm step even if we prompt for the
    // project, since nothing is being mutated.
    let project = match project {
        Some(p) => p,
        None if interactive => prompt::prompt_project(vault)?,
        None => return Err(prompt::missing_flag("project")),
    };
    let entries = vault.list_actions(&project).context("listing actions")?;
    print!("{}", render_list(&project, &entries));
    Ok(())
}

// ---------------------------------------------------------------------
// Shared gather helper and small utilities.
// ---------------------------------------------------------------------

/// Strip a trailing `(deep|medium|light)` suffix from a bullet label.
/// Used when the interactive bullet picker hands back the full text —
/// the domain's substring matcher strips the same suffix on its end,
/// so the query without the suffix is what resolves uniquely.
fn strip_energy_for_query(text: &str) -> String {
    for suffix in [" (deep)", " (medium)", " (light)"] {
        if let Some(stripped) = text.strip_suffix(suffix) {
            return stripped.to_owned();
        }
    }
    text.to_owned()
}

fn yesno(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// Render `cdno action list` output. Pure so tests can exercise the
/// formatting without going through stdout.
pub fn render_list(project: &str, entries: &[ActionListEntry]) -> String {
    let mut out = format!("Actions for projects/{project}.md\n");
    if entries.is_empty() {
        out.push_str("  (no open actions)\n");
        return out;
    }
    for entry in entries {
        out.push_str("  - ");
        out.push_str(&entry.text);
        if let Some(att) = &entry.attached {
            out.push_str(&format!("  [{}]", status_label(att)));
        }
        out.push('\n');
    }
    out
}

fn status_label(att: &AttachedAction) -> &'static str {
    match att.status {
        ActionStatus::Active => "active",
        ActionStatus::Blocked => "blocked",
        ActionStatus::Completed => "completed",
    }
}
