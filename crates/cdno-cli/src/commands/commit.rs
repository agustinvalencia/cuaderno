//! `cdno commit` subcommands: create a standalone commitment note,
//! mark one as completed. Thin clap-to-domain layer over
//! [`cdno_domain::Vault::create_commitment`] and
//! [`cdno_domain::Vault::complete_commitment`].
//!
//! Mirrors the `cdno project` surface (`create` / `done` verbs) for
//! consistency. Promptable fields are clap-optional and follow the
//! gather→confirm→execute pattern from `docs/cli-ergonomics.md`.
//! Done's slug is a plain text prompt for now; a fuzzy picker over
//! active commitments arrives with the broader retrofit.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime};
use clap::Subcommand;

use cdno_domain::Vault;
use cdno_domain::frontmatter::Context as CommitmentContext;

use crate::bootstrap;
use crate::commands::project::parse_iso_date;
use crate::prompt;

#[derive(Debug, Subcommand)]
pub enum CommitCommands {
    /// Create an active commitment at `commitments/<slug>.md`.
    Create {
        /// Commitment title (also used as the body heading; slugged for
        /// the filename).
        #[arg(long)]
        title: Option<String>,
        /// Due date, `YYYY-MM-DD`.
        #[arg(long, value_parser = parse_iso_date)]
        due: Option<NaiveDate>,
        /// Life-domain context (work, personal, home-family, ...).
        #[arg(long)]
        context: Option<CommitmentContext>,
        /// Optional related project slug (bare slug). Lets that project
        /// list this commitment among its related dated items.
        #[arg(long)]
        project: Option<String>,
        /// Optional related stewardship slug (bare slug). Lets that
        /// stewardship list this commitment among its related dated
        /// items.
        #[arg(long)]
        stewardship: Option<String>,
        /// Value for a custom template's prompted variable
        /// (`[variables.prompt]`), repeatable: `--var name=value`.
        #[arg(long = "var", value_parser = crate::prompt::parse_key_val)]
        var: Vec<(String, String)>,
    },

    /// Mark a commitment as completed: stamps `status` and `completed`,
    /// moves the note to `commitments/_done/<year>/<slug>.md`.
    Done {
        /// Slug of the active commitment to complete.
        #[arg(long)]
        slug: Option<String>,
    },
}

pub fn run(
    root: &Path,
    at: NaiveDateTime,
    command: CommitCommands,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    // `--json` implies non-interactive: prompts/confirms print to stdout,
    // which would corrupt the JSON result. Scripted callers pass full args.
    let interactive = prompt::is_interactive(no_interactive || json);
    match command {
        CommitCommands::Create {
            title,
            due,
            context,
            project,
            stewardship,
            var,
        } => create(
            &vault,
            at,
            title,
            due,
            context,
            project,
            stewardship,
            var,
            interactive,
            json,
        ),
        CommitCommands::Done { slug } => done(&vault, at, slug, interactive, json),
    }
}

#[allow(clippy::too_many_arguments)]
fn create(
    vault: &Vault,
    at: NaiveDateTime,
    title: Option<String>,
    due: Option<NaiveDate>,
    context: Option<CommitmentContext>,
    project: Option<String>,
    stewardship: Option<String>,
    var: Vec<(String, String)>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    let title = prompt::gather_or_error(title, "title", interactive, &mut prompted, || {
        prompt::prompt_text("Title")
    })?;
    let due = prompt::gather_or_error(due, "due", interactive, &mut prompted, || {
        prompt::prompt_date("Due date")
    })?;
    let context = prompt::gather_or_error(context, "context", interactive, &mut prompted, || {
        prompt::prompt_context()
    })?;
    let template_vars =
        prompt::gather_template_vars(vault, "commitment", None, &var, interactive, &mut prompted)?;

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to create commitment:\n  title:       {title}\n  due:         {due}\n  context:     {}\n  project:     {}\n  stewardship: {}",
            context.as_str(),
            project.as_deref().unwrap_or("(none)"),
            stewardship.as_deref().unwrap_or("(none)"),
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }
    let path = vault
        .create_commitment_with_vars(
            at,
            &title,
            due,
            context,
            project.as_deref(),
            stewardship.as_deref(),
            &template_vars,
        )
        .context("creating commitment")?;
    crate::output::emit_write_result(json, &path.to_string(), &format!("Created {path}"))?;
    Ok(())
}

fn done(
    vault: &Vault,
    at: NaiveDateTime,
    slug: Option<String>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    // Plain text prompt for the slug — the fuzzy picker over active
    // commitments lands with the rest of the retrofit follow-up. Users
    // can grab slugs from `cdno commitments` output meanwhile.
    let slug = prompt::gather_or_error(slug, "slug", interactive, &mut prompted, || {
        prompt::prompt_text("Commitment slug")
    })?;

    if prompted && !prompt::confirm_preview(&format!("About to mark commitment '{slug}' as done"))?
    {
        println!("Aborted.");
        return Ok(());
    }
    let path = vault
        .complete_commitment(at, &slug)
        .context("completing commitment")?;
    crate::output::emit_write_result(json, &path.to_string(), &format!("Completed at {path}"))?;
    Ok(())
}
