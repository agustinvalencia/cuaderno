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

use cdno_domain::frontmatter::{Context as ProjectContext, ProjectStatus};

use crate::bootstrap;

#[derive(Debug, Subcommand)]
pub enum ProjectCommands {
    /// Create a new project map. Seeds parked when at the active cap.
    /// Title and context are clap-optional so they can be prompted for
    /// interactively; in non-interactive sessions, missing flags
    /// error with the standard "missing required flag" message.
    Create {
        /// Project title.
        #[arg(long)]
        title: Option<String>,
        /// Life-domain context: work, side-project, university,
        /// family, household, legal, or personal.
        #[arg(long)]
        context: Option<ProjectContext>,
        /// Vault-relative target for the core question wikilink
        /// (e.g. `questions/research/foo`). Always optional.
        #[arg(long)]
        question: Option<String>,
    },

    /// Update the Current State section, auto-logging the previous body.
    State {
        /// Project slug.
        #[arg(long)]
        slug: Option<String>,
        /// New state text.
        #[arg(long)]
        text: Option<String>,
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
/// operation performs. `no_interactive` is forwarded from the global
/// CLI flag and used by the retrofitted verbs (Create, State); the
/// pre-retrofit verbs (Park / Activate / Milestone / Waiting) still
/// take required positionals and ignore the flag.
pub fn run(
    root: &Path,
    at: NaiveDateTime,
    command: ProjectCommands,
    no_interactive: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let interactive = crate::prompt::is_interactive(no_interactive);
    match command {
        ProjectCommands::Create {
            title,
            context,
            question,
        } => create(&vault, at, title, context, question, interactive)?,
        ProjectCommands::State { slug, text } => {
            state(&vault, at, slug, text, interactive)?;
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

/// `cdno project create` — gather title / context / question with the
/// ergonomics convention, then call `Vault::create_project`. Confirm
/// when anything was prompted.
fn create(
    vault: &cdno_domain::Vault,
    at: NaiveDateTime,
    title: Option<String>,
    context: Option<ProjectContext>,
    question: Option<String>,
    interactive: bool,
) -> Result<()> {
    use crate::prompt;
    let mut prompted = false;
    let title = prompt::gather_or_error(title, "title", interactive, &mut prompted, || {
        prompt::prompt_text("Title")
    })?;
    let context = prompt::gather_or_error(context, "context", interactive, &mut prompted, || {
        prompt::prompt_context()
    })?;
    // `question` is genuinely optional — no prompt, no error if absent.

    if prompted {
        let q = question.as_deref().unwrap_or("(none)");
        if !prompt::confirm_preview(&format!(
            "About to create project:\n  title:    {title}\n  context:  {}\n  question: {q}",
            context.as_str(),
        ))? {
            println!("Aborted.");
            return Ok(());
        }
    }
    let path = vault
        .create_project(at.date(), &title, context, question.as_deref())
        .context("creating project")?;
    println!("Created {path}");
    Ok(())
}

/// `cdno project state` — gather slug / text and call
/// `Vault::update_project_state`. The slug picker is over active
/// projects only (state updates error on parked); a free-text input
/// for the new state body keeps things simple.
fn state(
    vault: &cdno_domain::Vault,
    at: NaiveDateTime,
    slug: Option<String>,
    text: Option<String>,
    interactive: bool,
) -> Result<()> {
    use crate::prompt;
    let mut prompted = false;
    let slug = prompt::gather_or_error(slug, "slug", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let text = prompt::gather_or_error(text, "text", interactive, &mut prompted, || {
        prompt::prompt_text("New state")
    })?;

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to update state of project '{slug}':\n  {text}"
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }
    let path = vault
        .update_project_state(at, &slug, &text)
        .context("updating project state")?;
    println!("Updated {path}");
    Ok(())
}

/// Parse a `YYYY-MM-DD` date for clap's `value_parser` on
/// `--date`. Public so the integration test in `tests/project.rs`
/// can hit it directly: clap's path runs in a subprocess that
/// Linux tarpaulin can't instrument, so a public surface is the
/// cheapest route to honest coverage. Callers other than tests
/// shouldn't depend on this — it's a CLI argument-parsing helper,
/// not a domain primitive.
pub fn parse_iso_date(s: &str) -> Result<NaiveDate, String> {
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
