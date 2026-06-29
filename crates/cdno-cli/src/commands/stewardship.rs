//! `cdno stewardship` subcommands: create, list, show, add-periodic.
//!
//! Filing a tracking note is the separate top-level `cdno track`
//! verb because it's a routine action (a user logs activities
//! several times a week), while these subcommands manage the
//! dashboard itself — created at vault setup time and revisited
//! during reviews.

use std::path::Path;

use anyhow::{Context as AnyhowContext, Result};
use chrono::{NaiveDate, NaiveDateTime};
use clap::Subcommand;
use clap_complete::engine::ArgValueCompleter;

use cdno_domain::frontmatter::{Context, StewardshipFrontmatter};
use cdno_domain::recurrence::Recurrence;
use cdno_domain::{StewardshipSummary, StewardshipVariant, Vault};

use crate::bootstrap;
use crate::completions;
use crate::prompt;

#[derive(Debug, Subcommand)]
pub enum StewardshipCommands {
    /// Create a new stewardship dashboard. `--tracking` makes it
    /// expanded (`stewardships/<slug>/_index.md` with room for
    /// `tracking/` and `routines/` siblings); without it the
    /// dashboard is a single flat file at `stewardships/<slug>.md`.
    Create {
        /// Human-readable name. Becomes the body H1; the slug derives
        /// from it.
        #[arg(long)]
        name: Option<String>,
        /// Life context (work, household, personal, …).
        #[arg(long)]
        context: Option<Context>,
        /// Create the expanded variant with room for tracking notes.
        /// Without this flag the dashboard is flat.
        #[arg(long)]
        tracking: bool,
    },

    /// List every stewardship with its variant, tracking count, and
    /// staleness badge.
    List,

    /// Show a stewardship's frontmatter and an excerpt of the
    /// dashboard body.
    Show {
        /// Stewardship slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_stewardship))]
        slug: Option<String>,
    },

    /// Append a periodic commitment line to the dashboard's
    /// `## Periodic Commitments` section. The line becomes one row in
    /// the aggregated `cdno commitments` view.
    AddPeriodic {
        /// Stewardship slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_stewardship))]
        stewardship: Option<String>,
        /// Title of the commitment (e.g. "Dental check-up").
        #[arg(long)]
        title: Option<String>,
        /// Recurrence in canonical wire format: `daily`, `weekly`,
        /// `monthly`, `every N months`, or `yearly`. Quote when it
        /// contains spaces.
        #[arg(long)]
        every: Option<Recurrence>,
        /// Next due date, `YYYY-MM-DD`.
        #[arg(long, value_parser = parse_iso_date)]
        next: Option<NaiveDate>,
    },
}

pub fn run(
    root: &Path,
    at: NaiveDateTime,
    command: StewardshipCommands,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    // `--json` implies non-interactive: prompts/confirms print to stdout,
    // which would corrupt the JSON result. Scripted callers pass full args.
    let interactive = prompt::is_interactive(no_interactive || json);
    match command {
        StewardshipCommands::Create {
            name,
            context,
            tracking,
        } => create(&vault, at, name, context, tracking, interactive, json),
        StewardshipCommands::List => {
            let summaries = vault
                .list_stewardships(at.date())
                .context("listing stewardships")?;
            if json {
                println!("{}", serde_json::to_string_pretty(&summaries)?);
            } else {
                print!("{}", render_list(&summaries));
            }
            Ok(())
        }
        StewardshipCommands::Show { slug } => show(&vault, at, slug, interactive),
        StewardshipCommands::AddPeriodic {
            stewardship,
            title,
            every,
            next,
        } => add_periodic(
            &vault,
            at,
            stewardship,
            title,
            every,
            next,
            interactive,
            json,
        ),
    }
}

fn create(
    vault: &Vault,
    at: NaiveDateTime,
    name: Option<String>,
    context: Option<Context>,
    tracking: bool,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    let name = prompt::gather_or_error(name, "name", interactive, &mut prompted, || {
        prompt::prompt_text("Stewardship name")
    })?;
    let context = prompt::gather_or_error(context, "context", interactive, &mut prompted, || {
        prompt::prompt_context()
    })?;
    // `--tracking` is a CLI bool. In interactive mode (when other
    // fields prompted) we ask explicitly; in non-interactive mode the
    // flag value is taken at face value.
    let tracking = if prompted {
        prompt::prompt_confirm(
            "Track activities under this stewardship? (expanded variant)",
            tracking,
        )?
    } else {
        tracking
    };

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to create stewardship:\n  name:    {name}\n  context: {}\n  variant: {}",
            context.as_str(),
            if tracking { "expanded" } else { "flat" }
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    let path = if tracking {
        vault.create_stewardship_expanded(at, &name, context)
    } else {
        vault.create_stewardship_flat(at, &name, context)
    }
    .context("creating stewardship")?;
    crate::output::emit_write_result(json, &path.to_string(), &format!("Created {path}"))?;
    Ok(())
}

fn show(vault: &Vault, at: NaiveDateTime, slug: Option<String>, interactive: bool) -> Result<()> {
    let slug = match slug {
        Some(s) => s,
        None if interactive => prompt::prompt_stewardship(vault, at.date())?,
        None => return Err(prompt::missing_flag("slug")),
    };
    let (fm, body, summary) = load_for_show(vault, at, &slug)?;
    print!("{}", render_show(&slug, &fm, &body, summary.as_ref()));
    Ok(())
}

#[allow(clippy::too_many_arguments)] // thin CLI gather→confirm→execute passthrough
fn add_periodic(
    vault: &Vault,
    at: NaiveDateTime,
    stewardship: Option<String>,
    title: Option<String>,
    every: Option<Recurrence>,
    next: Option<NaiveDate>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    let stewardship = prompt::gather_or_error(
        stewardship,
        "stewardship",
        interactive,
        &mut prompted,
        || prompt::prompt_stewardship(vault, at.date()),
    )?;
    let title = prompt::gather_or_error(title, "title", interactive, &mut prompted, || {
        prompt::prompt_text("Title (e.g. Dental check-up)")
    })?;
    let every = prompt::gather_or_error(every, "every", interactive, &mut prompted, || {
        prompt::prompt_recurrence()
    })?;
    let next = prompt::gather_or_error(next, "next", interactive, &mut prompted, || {
        prompt::prompt_date("Next due date")
    })?;
    if prompted
        && !prompt::confirm_preview(&format!(
            "About to add periodic commitment to '{stewardship}':\n  title:  {title}\n  every:  {every}\n  next:   {next}"
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }
    let path = vault
        .add_periodic_commitment(at, &stewardship, &title, every, next)
        .context("adding periodic commitment")?;
    crate::output::emit_write_result(json, &path.to_string(), &format!("Updated {path}"))?;
    Ok(())
}

/// Pre-read the dashboard for `show` and locate its summary row
/// from `list_stewardships`. Pulled out so `render_show` stays a
/// pure formatter (testable with stdout-free assertions).
fn load_for_show(
    vault: &Vault,
    at: NaiveDateTime,
    slug: &str,
) -> Result<(StewardshipFrontmatter, String, Option<StewardshipSummary>)> {
    let (fm, body, _variant) = vault
        .get_stewardship(slug)
        .with_context(|| format!("loading stewardship '{slug}'"))?;
    let summaries = vault
        .list_stewardships(at.date())
        .context("listing stewardships")?;
    let summary = summaries.iter().find(|s| s.slug == slug).cloned();
    Ok((fm, body, summary))
}

/// Render `cdno stewardship list` output. Public so tests can
/// assert on the formatted text without capturing stdout.
pub fn render_list(summaries: &[StewardshipSummary]) -> String {
    if summaries.is_empty() {
        return "Stewardships\n  (none \u{2014} create one with `cdno stewardship create`)\n"
            .to_owned();
    }
    // slug / name / variant / tracking-activity columns (#153).
    let mut table = crate::output::styled_table();
    for s in summaries {
        let variant_badge = match s.variant {
            StewardshipVariant::Flat => "[flat]",
            StewardshipVariant::Expanded => "[expanded]",
        };
        let activity_badge = match (s.tracking_count, s.staleness_days) {
            (0, _) => "no tracking yet".to_owned(),
            (n, Some(d)) => format!("{n} tracking, last {d} days ago"),
            (n, None) => format!("{n} tracking"),
        };
        table.add_row(vec![
            s.slug.clone(),
            s.name.clone(),
            variant_badge.to_owned(),
            activity_badge,
        ]);
    }
    // Keep slug, variant, and activity badge whole; only the name reflows.
    crate::output::no_wrap_columns(&mut table, &[0, 2, 3]);
    format!("Stewardships\n{}\n", crate::output::render(&table))
}

/// Render `cdno stewardship show` output. Public for test access.
pub fn render_show(
    slug: &str,
    fm: &StewardshipFrontmatter,
    body: &str,
    summary: Option<&StewardshipSummary>,
) -> String {
    let name = body
        .lines()
        .find_map(|l| l.trim_start().strip_prefix("# "))
        .unwrap_or(slug)
        .trim();
    let variant_label = match summary.map(|s| s.variant) {
        Some(StewardshipVariant::Flat) => "flat",
        Some(StewardshipVariant::Expanded) => "expanded",
        None => "?",
    };
    let mut out = format!("{slug} \u{2014} {name} [{variant_label}]\n");
    out.push_str(&format!("Context: {}\n", fm.context.as_str()));
    if let Some(s) = summary {
        match (s.tracking_count, s.staleness_days) {
            (0, _) => out.push_str("Tracking: (none yet)\n"),
            (n, Some(d)) => out.push_str(&format!("Tracking: {n} notes, last {d} days ago\n")),
            (n, None) => out.push_str(&format!("Tracking: {n} notes\n")),
        }
    }
    out
}

/// Parse a `YYYY-MM-DD` date for clap's `value_parser` on
/// `--next`. Copies the helper used by project/commit verbs.
fn parse_iso_date(s: &str) -> std::result::Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| format!("could not parse `{s}` as a date (expected YYYY-MM-DD)"))
}
