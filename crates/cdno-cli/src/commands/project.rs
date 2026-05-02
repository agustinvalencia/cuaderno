//! `cdno project` subcommands: thin clap-to-domain layer for the
//! project surface (create, state, action/done, milestone, waiting,
//! park/activate, list, show).
//!
//! The clap argument types live here so `main.rs` stays a flat
//! dispatcher; each subcommand maps directly onto a method on
//! [`cdno_domain::Vault`].

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime};
use clap::Subcommand;

use cdno_domain::frontmatter::{Context as ProjectContext, EnergyLevel, ProjectStatus};

use crate::bootstrap;

#[derive(Debug, Subcommand)]
pub enum ProjectCommands {
    /// Create a new project map. Seeds parked when at the active cap.
    Create {
        /// Project title.
        title: String,
        /// Life-domain context: work, side-project, university,
        /// family, household, legal, or personal.
        #[arg(long)]
        context: ProjectContext,
        /// Vault-relative target for the core question wikilink
        /// (e.g. `questions/research/foo`). Optional.
        #[arg(long)]
        question: Option<String>,
    },

    /// Update the Current State section, auto-logging the previous body.
    State {
        slug: String,
        /// New state text.
        text: String,
    },

    /// Append a next action with an energy tag.
    Action {
        slug: String,
        /// Action description.
        text: String,
        /// Energy bucket: deep, medium, or light.
        #[arg(long)]
        energy: EnergyLevel,
    },

    /// Mark a next action as done by case-insensitive substring match.
    Done {
        slug: String,
        /// Substring matching the action to complete.
        query: String,
    },

    /// Move an active project to projects/_parked/.
    Park { slug: String },

    /// Bring a parked project back, enforcing the active-project cap.
    Activate { slug: String },

    /// List active projects with their state snippet.
    List,

    /// Show a compact summary of a single project (any status).
    Show { slug: String },

    /// Manage project milestones.
    Milestone {
        #[command(subcommand)]
        action: MilestoneCommands,
    },

    /// Manage waiting-on items.
    Waiting {
        #[command(subcommand)]
        action: WaitingCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum MilestoneCommands {
    /// Add a milestone with a target or hard date.
    Add {
        slug: String,
        /// Milestone title.
        title: String,
        /// Target date (YYYY-MM-DD).
        #[arg(long, value_parser = parse_iso_date)]
        date: NaiveDate,
        /// Treat as a hard deadline (counted in commitments aggregation).
        #[arg(long)]
        hard: bool,
    },
    /// Mark a milestone as done by substring match.
    Done {
        slug: String,
        /// Substring matching the milestone title.
        query: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum WaitingCommands {
    /// Add a waiting-on item.
    Add {
        slug: String,
        /// Description of what's blocking.
        description: String,
    },
    /// Resolve (remove) a waiting-on item by substring match.
    Resolve {
        slug: String,
        /// Substring matching the waiting-on item.
        query: String,
    },
}

/// Dispatch a parsed `cdno project ...` invocation. `at` is the
/// timestamp used for any daily-log writes the underlying domain
/// operation performs.
pub fn run(root: &Path, at: NaiveDateTime, command: ProjectCommands) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    match command {
        ProjectCommands::Create {
            title,
            context,
            question,
        } => {
            let path = vault
                .create_project(at.date(), &title, context, question.as_deref())
                .context("creating project")?;
            println!("Created {path}");
        }
        ProjectCommands::State { slug, text } => {
            let path = vault
                .update_project_state(at, &slug, &text)
                .context("updating project state")?;
            println!("Updated {path}");
        }
        ProjectCommands::Action { slug, text, energy } => {
            let path = vault
                .add_action(at, &slug, &text, energy)
                .context("adding next action")?;
            println!("Action added to {path}");
        }
        ProjectCommands::Done { slug, query } => {
            let path = vault
                .complete_action(at, &slug, &query)
                .context("completing action")?;
            println!("Action done on {path}");
        }
        ProjectCommands::Park { slug } => {
            let path = vault.park_project(at, &slug).context("parking project")?;
            println!("Parked at {path}");
        }
        ProjectCommands::Activate { slug } => {
            let path = vault
                .activate_project(at, &slug)
                .context("activating project")?;
            println!("Activated at {path}");
        }
        ProjectCommands::List => {
            let active = vault.active_projects().context("listing active projects")?;
            print_active_list(&vault, &active)?;
        }
        ProjectCommands::Show { slug } => {
            let summary = vault
                .project_summary(&slug)
                .context("loading project summary")?;
            print_summary(&summary);
        }
        ProjectCommands::Milestone { action } => match action {
            MilestoneCommands::Add {
                slug,
                title,
                date,
                hard,
            } => {
                let path = vault
                    .add_milestone(at, &slug, &title, date, hard)
                    .context("adding milestone")?;
                println!("Milestone added to {path}");
            }
            MilestoneCommands::Done { slug, query } => {
                let path = vault
                    .complete_milestone(at, &slug, &query)
                    .context("completing milestone")?;
                println!("Milestone done on {path}");
            }
        },
        ProjectCommands::Waiting { action } => match action {
            WaitingCommands::Add { slug, description } => {
                let path = vault
                    .add_waiting_on(at, &slug, &description)
                    .context("adding waiting-on item")?;
                println!("Waiting-on added to {path}");
            }
            WaitingCommands::Resolve { slug, query } => {
                let path = vault
                    .resolve_waiting_on(at, &slug, &query)
                    .context("resolving waiting-on item")?;
                println!("Waiting-on resolved on {path}");
            }
        },
    }
    Ok(())
}

fn parse_iso_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| format!("expected YYYY-MM-DD, got `{s}`: {e}"))
}

/// Render `cdno project list` output. Iterates the active projects
/// and calls `project_summary` per slug to surface a one-line state
/// hint alongside the path.
fn print_active_list(
    vault: &cdno_domain::Vault,
    active: &[(cdno_core::path::VaultPath, cdno_domain::ProjectFrontmatter)],
) -> Result<()> {
    if active.is_empty() {
        println!("No active projects.");
        return Ok(());
    }
    println!("{} active project(s):", active.len());
    for (path, fm) in active {
        let slug = path
            .as_path()
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?");
        let summary = vault
            .project_summary(slug)
            .with_context(|| format!("loading summary for {slug}"))?;
        let state_first_line = summary.state_snippet.lines().next().unwrap_or("(no state)");
        println!("  {slug} [{}] — {state_first_line}", fm.context.as_str());
    }
    Ok(())
}

/// Render `cdno project show <slug>` output: a compact block with
/// the project's status, current state snippet (up to 2 lines), and
/// top action.
fn print_summary(summary: &cdno_domain::ProjectSummary) {
    let status_word = match summary.status {
        ProjectStatus::Active => "active",
        ProjectStatus::Parked => "parked",
        ProjectStatus::Completed => "completed",
    };
    println!("[{}] ({status_word})", summary.slug);
    if summary.state_snippet.is_empty() {
        println!("  State: (none)");
    } else {
        println!("  State:");
        for line in summary.state_snippet.lines() {
            println!("    {line}");
        }
    }
    match &summary.top_action {
        Some(action) => match action.energy {
            Some(energy) => println!("  Top: {} ({})", action.text, energy.as_str()),
            None => println!("  Top: {}", action.text),
        },
        None => println!("  Top: (no open actions)"),
    }
}
