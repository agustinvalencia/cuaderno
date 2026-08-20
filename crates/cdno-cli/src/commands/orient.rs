//! `cdno orient`: the daily-orientation view — commitments due soon,
//! active projects with their top next action, and a suggested
//! starting point. The first command that composes several domain
//! queries (via `Vault::orientation_context`) into one display.
//!
//! Rendering is split from I/O: `build_orientation` returns the text
//! so tests can assert on it without capturing stdout, and `run` just
//! prints what it returns. An optional `--energy` biases the
//! suggestion; the *interactive* energy prompt is deferred to the
//! `cdno-cli::prompt` ergonomics work (#113), which brings `inquire`.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDate;

use cdno_domain::frontmatter::EnergyLevel;
use cdno_domain::{
    CommitmentEntry, CommitmentSource, OrientationContext, ProjectSummary, TopAction,
};

use crate::bootstrap;
use crate::output::card::{Card, render_cards};
use crate::output::style::{Accent, Palette, Role, cell};

/// Render the daily orientation for the vault at `root` as of `today`.
pub fn run(root: &Path, today: NaiveDate, energy: Option<EnergyLevel>, json: bool) -> Result<()> {
    if json {
        let (vault, _report) = bootstrap::open_vault(root)?;
        let ctx = vault
            .orientation_context(today)
            .context("building orientation context")?;
        println!("{}", serde_json::to_string_pretty(&ctx)?);
        return Ok(());
    }
    print!("{}", build_orientation(root, today, energy)?);
    Ok(())
}

/// Open the vault, build the orientation context, and render it to a
/// string. Split from [`run`] so tests can assert on the rendered text
/// without capturing stdout.
pub fn build_orientation(
    root: &Path,
    today: NaiveDate,
    energy: Option<EnergyLevel>,
) -> Result<String> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let ctx = vault
        .orientation_context(today)
        .context("building orientation context")?;
    Ok(render(&ctx, today, energy))
}

fn render(ctx: &OrientationContext, today: NaiveDate, energy: Option<EnergyLevel>) -> String {
    let palette = Palette::active();
    let mut out = format!(
        "{}\n\n",
        palette.paint(
            Role::Heading,
            &format!("Orientation — {}", today.format("%A %-d %B %Y"))
        )
    );

    out.push_str(&heading(
        &palette,
        "Commitments (due within 48h, plus overdue)",
    ));
    if ctx.commitments.is_empty() {
        out.push_str(&format!(
            "  {}\n",
            palette.paint(Role::Muted, "(nothing due)")
        ));
    } else {
        // Genuinely tabular — three short, aligned fields per row — so
        // this stays a table and gains only colour.
        let mut table = crate::output::styled_table();
        for c in &ctx.commitments {
            table.add_row(commitment_row(c));
        }
        // date and source stay whole; the title reflows.
        crate::output::no_wrap_columns(&mut table, &[0, 2]);
        out.push_str(&crate::output::render(&table));
        out.push('\n');
    }
    out.push('\n');

    out.push_str(&heading(&palette, "Active projects"));
    if ctx.projects.is_empty() {
        out.push_str(&format!(
            "  {}\n",
            palette.paint(
                Role::Muted,
                "(none — create one with `cdno project create`)"
            )
        ));
    } else {
        // A project carries prose, so it gets a card rather than a row:
        // the state and the `next:` action read as one item behind a
        // context-coloured gutter instead of as a wrapped table cell.
        let cards: Vec<Card> = ctx
            .projects
            .iter()
            .map(|p| {
                Card::new(&p.slug)
                    .badge(p.context.as_str())
                    .accent(Accent::for_context(p.context))
                    .prose(state_line(p))
                    .meta(format!("next: {}", project_next(p)))
            })
            .collect();
        out.push_str(&render_cards(
            &cards,
            &palette,
            crate::output::render_width(),
        ));
    }
    out.push('\n');

    // Lapsed habits arrive in Phase 3; only render the section once
    // there's something to show.
    if !ctx.lapsed_habits.is_empty() {
        out.push_str(&heading(&palette, "Lapsed habits"));
        let mut table = crate::output::styled_table();
        for h in &ctx.lapsed_habits {
            table.add_row(vec![
                cell(Role::Slug, &h.stewardship),
                cell(Role::Warn, &h.detail),
            ]);
        }
        crate::output::no_wrap_columns(&mut table, &[0]);
        out.push_str(&crate::output::render(&table));
        out.push('\n');
        out.push('\n');
    }

    out.push_str(&heading(&palette, "Suggested start"));
    out.push_str(&format!("  {}\n", suggestion(ctx, energy)));

    out
}

/// A section heading on its own line. Shared so every section of the
/// orientation reads at the same weight.
pub(crate) fn heading(palette: &Palette, text: &str) -> String {
    format!("{}\n", palette.paint(Role::Heading, text))
}

/// [`commitment_cells`] as styled table cells: the date and the overdue
/// marker are what the reader scans for, so they carry the colour.
pub(crate) fn commitment_row(c: &CommitmentEntry) -> Vec<comfy_table::Cell> {
    let cells = commitment_cells(c);
    let source_role = if c.is_overdue { Role::Warn } else { Role::Meta };
    vec![
        cell(Role::Meta, &cells[0]),
        cell(Role::Prose, &cells[1]),
        cell(source_role, &cells[2]),
    ]
}

/// The cells for one commitment row: `date` / `title` / `source`, with
/// the overdue marker appended to the source cell. Shared with the
/// `cdno commitments` list view so both surfaces tabulate identically.
pub(crate) fn commitment_cells(c: &CommitmentEntry) -> Vec<String> {
    let mut source = source_label(&c.source);
    if c.is_overdue {
        source.push_str("  — overdue");
    }
    vec![c.date.to_string(), c.title.clone(), source]
}

pub(crate) fn source_label(source: &CommitmentSource) -> String {
    match source {
        CommitmentSource::ProjectMilestone(slug) => format!("project: {slug}"),
        CommitmentSource::Stewardship(slug) => format!("stewardship: {slug}"),
        CommitmentSource::StandaloneCommitment(_) => "commitment".to_owned(),
        CommitmentSource::ActionNote(slug) => format!("action: {slug}"),
    }
}

/// The project's state snippet collapsed to a single line, or a
/// placeholder when the project has no recorded state.
fn state_line(p: &ProjectSummary) -> String {
    if p.state_snippet.trim().is_empty() {
        "(no state recorded)".to_owned()
    } else {
        p.state_snippet.replace('\n', " ")
    }
}

/// The project's top next action label, or a placeholder when none is
/// open. Shared with `cdno status`.
pub(crate) fn project_next(p: &ProjectSummary) -> String {
    match &p.top_action {
        Some(action) => action_label(action),
        None => "(no open actions)".to_owned(),
    }
}

fn action_label(action: &TopAction) -> String {
    match action.energy {
        Some(energy) => format!("{} ({})", action.text, energy.as_str()),
        None => action.text.clone(),
    }
}

/// Pick a starting point: a project whose top action matches the
/// requested `energy` if given, otherwise the first project with any
/// open action. Falls back to a capture hint when nothing is queued.
fn suggestion(ctx: &OrientationContext, energy: Option<EnergyLevel>) -> String {
    let pick = energy
        .and_then(|want| {
            ctx.projects
                .iter()
                .find(|p| p.top_action.as_ref().and_then(|a| a.energy) == Some(want))
        })
        .or_else(|| ctx.projects.iter().find(|p| p.top_action.is_some()));

    match pick {
        Some(p) => format!("{}: {}", p.slug, project_next(p)),
        None => "nothing queued — capture a next action with `cdno project action`".to_owned(),
    }
}
