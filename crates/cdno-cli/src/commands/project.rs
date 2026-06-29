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
use clap_complete::engine::ArgValueCompleter;

use cdno_domain::frontmatter::{Context as ProjectContext, ProjectStatus};

use crate::bootstrap;
use crate::completions;

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
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        slug: Option<String>,
        /// New state text.
        #[arg(long)]
        text: Option<String>,
    },

    /// Move an active project to projects/_parked/.
    Park {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        slug: Option<String>,
    },

    /// Bring a parked project back, enforcing the active-project cap.
    Activate {
        /// Project slug (parked).
        #[arg(long, add = ArgValueCompleter::new(completions::complete_parked_project))]
        slug: Option<String>,
    },

    /// List active projects with their state snippet.
    List,

    /// Show a compact summary of a single project (any status).
    Show {
        #[arg(add = ArgValueCompleter::new(completions::complete_any_project))]
        slug: String,
    },

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
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        slug: Option<String>,
        /// Milestone title.
        #[arg(long)]
        title: Option<String>,
        /// Target date (YYYY-MM-DD).
        #[arg(long, value_parser = parse_iso_date)]
        date: Option<NaiveDate>,
        /// Treat as a hard deadline (counted in commitments aggregation).
        #[arg(long)]
        hard: bool,
    },
    /// Mark a milestone as done by substring match.
    Done {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        slug: Option<String>,
        /// Substring matching the milestone title.
        #[arg(long)]
        query: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum WaitingCommands {
    /// Add a waiting-on item.
    Add {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        slug: Option<String>,
        /// Description of what's blocking.
        #[arg(long)]
        description: Option<String>,
    },
    /// Resolve (remove) a waiting-on item by substring match.
    Resolve {
        /// Project slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_active_project))]
        slug: Option<String>,
        /// Substring matching the waiting-on item.
        #[arg(long)]
        query: Option<String>,
    },
}

/// Dispatch a parsed `cdno project ...` invocation. `at` is the
/// timestamp used for any daily-log writes the underlying domain
/// operation performs. `no_interactive` is forwarded from the global
/// CLI flag; every mutating verb now follows the ergonomics
/// convention (gather missing fields → confirm-on-prompt → execute).
pub fn run(
    root: &Path,
    at: NaiveDateTime,
    command: ProjectCommands,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    // `--json` implies non-interactive: prompts/confirms print to stdout,
    // which would corrupt the JSON result. Scripted callers pass full args.
    let interactive = crate::prompt::is_interactive(no_interactive || json);
    match command {
        ProjectCommands::Create {
            title,
            context,
            question,
        } => create(&vault, at, title, context, question, interactive, json)?,
        ProjectCommands::State { slug, text } => {
            state(&vault, at, slug, text, interactive, json)?;
        }
        ProjectCommands::Park { slug } => park(&vault, at, slug, interactive, json)?,
        ProjectCommands::Activate { slug } => activate(&vault, at, slug, interactive, json)?,
        ProjectCommands::List => {
            let active = vault.active_projects().context("listing active projects")?;
            if json {
                // Serialise the per-project summaries (the same data the
                // text renderer fetches), not the raw frontmatter tuples.
                let summaries = active
                    .iter()
                    .map(|(path, _fm)| {
                        let slug = path
                            .as_path()
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        vault.project_summary(slug)
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .context("loading project summaries")?;
                println!("{}", serde_json::to_string_pretty(&summaries)?);
            } else {
                print_active_list(&vault, &active)?;
            }
        }
        ProjectCommands::Show { slug } => {
            let summary = vault
                .project_summary(&slug)
                .context("loading project summary")?;
            if json {
                // Same ProjectSummary shape as `project list` elements.
                println!("{}", serde_json::to_string_pretty(&summary)?);
            } else {
                print_summary(&summary);
            }
        }
        ProjectCommands::Milestone { action } => match action {
            MilestoneCommands::Add {
                slug,
                title,
                date,
                hard,
            } => milestone_add(&vault, at, slug, title, date, hard, interactive, json)?,
            MilestoneCommands::Done { slug, query } => {
                milestone_done(&vault, at, slug, query, interactive, json)?
            }
        },
        ProjectCommands::Waiting { action } => match action {
            WaitingCommands::Add { slug, description } => {
                waiting_add(&vault, at, slug, description, interactive, json)?
            }
            WaitingCommands::Resolve { slug, query } => {
                waiting_resolve(&vault, at, slug, query, interactive, json)?
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
    json: bool,
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
    crate::output::emit_write_result(json, &path.to_string(), &format!("Created {path}"))?;
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
    json: bool,
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
    crate::output::emit_write_result(json, &path.to_string(), &format!("Updated {path}"))?;
    Ok(())
}

/// `cdno project park` — fuzzy slug picker over active projects.
fn park(
    vault: &cdno_domain::Vault,
    at: NaiveDateTime,
    slug: Option<String>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    use crate::prompt;
    let mut prompted = false;
    let slug = prompt::gather_or_error(slug, "slug", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    if prompted && !prompt::confirm_preview(&format!("About to park project '{slug}'"))? {
        println!("Aborted.");
        return Ok(());
    }
    let path = vault.park_project(at, &slug).context("parking project")?;
    crate::output::emit_write_result(json, &path.to_string(), &format!("Parked at {path}"))?;
    Ok(())
}

/// `cdno project activate` — fuzzy slug picker over *parked* projects.
fn activate(
    vault: &cdno_domain::Vault,
    at: NaiveDateTime,
    slug: Option<String>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    use crate::prompt;
    let mut prompted = false;
    let slug = prompt::gather_or_error(slug, "slug", interactive, &mut prompted, || {
        prompt::prompt_parked_project(vault)
    })?;
    if prompted && !prompt::confirm_preview(&format!("About to activate project '{slug}'"))? {
        println!("Aborted.");
        return Ok(());
    }
    let path = vault
        .activate_project(at, &slug)
        .context("activating project")?;
    crate::output::emit_write_result(json, &path.to_string(), &format!("Activated at {path}"))?;
    Ok(())
}

/// `cdno project milestone add` — slug picker, title text, calendar
/// date, hard/soft confirm.
#[allow(clippy::too_many_arguments)] // thin CLI gather→confirm→execute passthrough
fn milestone_add(
    vault: &cdno_domain::Vault,
    at: NaiveDateTime,
    slug: Option<String>,
    title: Option<String>,
    date: Option<NaiveDate>,
    hard_flag: bool,
    interactive: bool,
    json: bool,
) -> Result<()> {
    use crate::prompt;
    let mut prompted = false;
    let slug = prompt::gather_or_error(slug, "slug", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let title = prompt::gather_or_error(title, "title", interactive, &mut prompted, || {
        prompt::prompt_text("Milestone title")
    })?;
    let date = prompt::gather_or_error(date, "date", interactive, &mut prompted, || {
        prompt::prompt_date("Target date")
    })?;
    // Only ask about --hard when we're already in an interactive flow.
    let hard = if prompted {
        prompt::prompt_hard_soft()?
    } else {
        hard_flag
    };
    if prompted
        && !prompt::confirm_preview(&format!(
            "About to add milestone to '{slug}':\n  title: {title}\n  date:  {date}\n  hard:  {hard}"
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }
    let path = vault
        .add_milestone(at, &slug, &title, date, hard)
        .context("adding milestone")?;
    crate::output::emit_write_result(
        json,
        &path.to_string(),
        &format!("Milestone added to {path}"),
    )?;
    Ok(())
}

/// `cdno project milestone done` — slug picker, then fuzzy open-
/// milestone picker (via [`Vault::open_milestones`]).
fn milestone_done(
    vault: &cdno_domain::Vault,
    at: NaiveDateTime,
    slug: Option<String>,
    query: Option<String>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    use crate::prompt;
    let mut prompted = false;
    let slug = prompt::gather_or_error(slug, "slug", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let query = prompt::gather_or_error(query, "query", interactive, &mut prompted, || {
        prompt::prompt_open_milestone(&slug, vault)
    })?;
    if prompted
        && !prompt::confirm_preview(&format!(
            "About to mark milestone '{query}' on '{slug}' as done"
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }
    let path = vault
        .complete_milestone(at, &slug, &query)
        .context("completing milestone")?;
    crate::output::emit_write_result(
        json,
        &path.to_string(),
        &format!("Milestone done on {path}"),
    )?;
    Ok(())
}

/// `cdno project waiting add` — slug picker + text input for the
/// blocker description.
fn waiting_add(
    vault: &cdno_domain::Vault,
    at: NaiveDateTime,
    slug: Option<String>,
    description: Option<String>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    use crate::prompt;
    let mut prompted = false;
    let slug = prompt::gather_or_error(slug, "slug", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let description = prompt::gather_or_error(
        description,
        "description",
        interactive,
        &mut prompted,
        || prompt::prompt_text("Waiting on"),
    )?;
    if prompted
        && !prompt::confirm_preview(&format!(
            "About to add waiting-on to '{slug}': {description}"
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }
    let path = vault
        .add_waiting_on(at, &slug, &description)
        .context("adding waiting-on item")?;
    crate::output::emit_write_result(
        json,
        &path.to_string(),
        &format!("Waiting-on added to {path}"),
    )?;
    Ok(())
}

/// `cdno project waiting resolve` — slug picker, then plain text
/// query input. A fuzzy picker over open waiting items lands when its
/// supporting domain query is wanted; for now a substring of the item
/// resolves uniquely via the domain matcher.
fn waiting_resolve(
    vault: &cdno_domain::Vault,
    at: NaiveDateTime,
    slug: Option<String>,
    query: Option<String>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    use crate::prompt;
    let mut prompted = false;
    let slug = prompt::gather_or_error(slug, "slug", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let query = prompt::gather_or_error(query, "query", interactive, &mut prompted, || {
        prompt::prompt_text("Waiting-on substring to resolve")
    })?;
    if prompted
        && !prompt::confirm_preview(&format!(
            "About to resolve waiting-on on '{slug}': '{query}'"
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }
    let path = vault
        .resolve_waiting_on(at, &slug, &query)
        .context("resolving waiting-on item")?;
    crate::output::emit_write_result(
        json,
        &path.to_string(),
        &format!("Waiting-on resolved on {path}"),
    )?;
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
