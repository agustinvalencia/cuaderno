//! `cdno action` subcommands: the user-facing surface for the action
//! layer. `add`, `promote`, and `complete` are thin clap-to-domain
//! shims; `list` reads `Vault::list_actions` and formats the bullets
//! with their attached-note status inline.
//!
//! Replaces the earlier `cdno project action` / `cdno project done`
//! entry points — straight rename, no deprecation, since the CLI is
//! unshipped.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use clap::Subcommand;

use cdno_domain::frontmatter::{ActionStatus, EnergyLevel};
use cdno_domain::{ActionListEntry, AttachedAction};

use crate::bootstrap;

#[derive(Debug, Subcommand)]
pub enum ActionCommands {
    /// Append a next action to a project. `--note` creates a manifest
    /// note alongside the bullet and wikilinks it.
    Add {
        /// Project slug.
        project: String,
        /// Action title.
        title: String,
        /// Energy bucket: deep, medium, or light.
        #[arg(long)]
        energy: EnergyLevel,
        /// Promote on creation: also write an action note and wikilink
        /// the bullet to it.
        #[arg(long)]
        note: bool,
    },

    /// Promote an existing plain bullet to a wikilinked manifest note.
    /// Substring-matches the bullet text; energy is inherited.
    Promote {
        /// Project slug.
        project: String,
        /// Substring matching the bullet to promote.
        query: String,
    },

    /// Mark a next action as completed by case-insensitive substring
    /// match. A wikilinked bullet also archives its note to
    /// `actions/_done/<year>/`.
    Complete {
        /// Project slug.
        project: String,
        /// Substring matching the action to complete.
        query: String,
    },

    /// List a project's open action bullets, with the attached-note
    /// status (active / blocked / completed) inline when present.
    List {
        /// Project slug.
        project: String,
    },
}

pub fn run(root: &Path, at: NaiveDateTime, command: ActionCommands) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    match command {
        ActionCommands::Add {
            project,
            title,
            energy,
            note,
        } => {
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
        }
        ActionCommands::Promote { project, query } => {
            let note_path = vault
                .promote_action(at, &project, &query)
                .context("promoting action")?;
            println!("Promoted to {note_path}");
        }
        ActionCommands::Complete { project, query } => {
            let project_path = vault
                .complete_action(at, &project, &query)
                .context("completing action")?;
            println!("Action done on {project_path}");
        }
        ActionCommands::List { project } => {
            let entries = vault.list_actions(&project).context("listing actions")?;
            print!("{}", render_list(&project, &entries));
        }
    }
    Ok(())
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
