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

/// Render the daily orientation for the vault at `root` as of `today`.
pub fn run(root: &Path, today: NaiveDate, energy: Option<EnergyLevel>) -> Result<()> {
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
    let mut out = format!("Orientation — {}\n\n", today.format("%A %-d %B %Y"));

    out.push_str("Commitments (due within 48h, plus overdue)\n");
    if ctx.commitments.is_empty() {
        out.push_str("  (nothing due)\n");
    } else {
        for c in &ctx.commitments {
            out.push_str(&format!("  {}\n", commitment_line(c)));
        }
    }
    out.push('\n');

    out.push_str("Active projects\n");
    if ctx.projects.is_empty() {
        out.push_str("  (none — create one with `cdno project create`)\n");
    } else {
        for p in &ctx.projects {
            out.push_str(&format!(
                "  {} — {}\n    next: {}\n",
                p.slug,
                state_line(p),
                project_next(p),
            ));
        }
    }
    out.push('\n');

    // Lapsed habits arrive in Phase 3; only render the section once
    // there's something to show.
    if !ctx.lapsed_habits.is_empty() {
        out.push_str("Lapsed habits\n");
        for h in &ctx.lapsed_habits {
            out.push_str(&format!("  {} — {}\n", h.stewardship, h.detail));
        }
        out.push('\n');
    }

    out.push_str("Suggested start\n");
    out.push_str(&format!("  {}\n", suggestion(ctx, energy)));

    out
}

/// Format one commitment line: `<date>  <title>  (<source>)` with a
/// trailing `— overdue` marker when past due.
fn commitment_line(c: &CommitmentEntry) -> String {
    let overdue = if c.is_overdue { "  — overdue" } else { "" };
    format!(
        "{}  {}  ({}){overdue}",
        c.date,
        c.title,
        source_label(&c.source),
    )
}

fn source_label(source: &CommitmentSource) -> String {
    match source {
        CommitmentSource::ProjectMilestone(slug) => format!("project: {slug}"),
        CommitmentSource::Stewardship(slug) => format!("stewardship: {slug}"),
        CommitmentSource::StandaloneCommitment => "commitment".to_owned(),
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
