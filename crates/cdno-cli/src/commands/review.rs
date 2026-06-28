//! `cdno review weekly` — the guided weekly-review ritual.
//!
//! Distinct from `cdno weekly` (which just *reads* the note) and the
//! low-level `upsert_weekly_section`: this walks the three retrospective
//! sections (Wins, Challenges, One Improvement) into the ending week's
//! note, then composes the forward goal — which lands in *next* week's
//! note as its `This Week's Goal`, the carry-forward hand-off.
//!
//! Non-interactive (or piped) runs print the current weekly note
//! instead of prompting, so the command stays scriptable and TTY-test
//! friendly.
//!
//! `cdno review monthly` is tracked separately — it needs a new monthly
//! note type + seam (the weekly note already exists; the monthly one
//! does not).

use std::path::Path;

use anyhow::{Context, Result};
use cdno_domain::WeeklySection;
use chrono::NaiveDate;
use clap::Subcommand;

use crate::bootstrap;
use crate::prompt;

#[derive(Debug, Subcommand)]
pub enum ReviewCommands {
    /// Walk the retrospective sections into this week's note, then set
    /// next week's goal in next week's note. Reads the current note when
    /// not interactive.
    Weekly,
}

pub fn run(
    root: &Path,
    today: NaiveDate,
    command: ReviewCommands,
    no_interactive: bool,
) -> Result<()> {
    match command {
        ReviewCommands::Weekly => weekly(root, today, no_interactive),
    }
}

/// The three retrospective sections, in ritual order. They reflect on
/// the ending week and land in its own note. The forward goal is handled
/// separately — it belongs in *next* week's note (see `weekly`).
const REVIEW_SECTIONS: [WeeklySection; 3] = [
    WeeklySection::Wins,
    WeeklySection::Challenges,
    WeeklySection::OneImprovement,
];

fn weekly(root: &Path, today: NaiveDate, no_interactive: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let view = vault
        .read_weekly_note(today)
        .context("reading the weekly note")?;

    // No TTY: show the current note rather than prompting into the void.
    // Reuse `cdno weekly`'s renderer so the two read paths match
    // (frontmatter stripped, same no-note placeholder).
    if !prompt::is_interactive(no_interactive) {
        print!("{}", crate::commands::weekly::render(&view, today));
        return Ok(());
    }

    println!("Weekly review. Leave a section blank to skip it.\n");
    if view.exists {
        println!(
            "Current note:\n{}",
            crate::commands::weekly::render(&view, today).trim_end()
        );
    }

    let mut saved_path: Option<String> = None;
    for section in REVIEW_SECTIONS {
        let entry = prompt::prompt_text(section.heading())?;
        if entry.trim().is_empty() {
            continue;
        }
        // `append: false` — compose/overwrite the section for this pass.
        // `upsert_weekly_section` returns the note path, so we don't need
        // a second read to report where it landed.
        let path = vault
            .upsert_weekly_section(today, section, &entry, false)
            .with_context(|| format!("writing the '{}' section", section.heading()))?;
        saved_path = Some(path.to_string());
    }

    match saved_path {
        Some(path) => println!("\nWeekly review saved to {path}."),
        None => println!("\nNothing entered \u{2014} weekly note unchanged."),
    }

    // The forward goal is next week's anchor, so it lands in *next*
    // week's note as its `This Week's Goal` — not this (ending) week's.
    let next_week = today + chrono::Duration::days(7);
    let goal = prompt::prompt_text("Next week's goal (its This Week's Goal)")?;
    if !goal.trim().is_empty() {
        let path = vault
            .upsert_weekly_section(next_week, WeeklySection::ThisWeeksGoal, &goal, false)
            .context("writing next week's goal")?;
        println!("Next week's goal saved to {path}.");
    }
    Ok(())
}
