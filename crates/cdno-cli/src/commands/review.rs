//! `cdno review weekly` — the guided weekly-review ritual.
//!
//! Distinct from `cdno weekly` (which just *reads* the note) and the
//! low-level `upsert_weekly_section`: this walks the three retrospective
//! sections (Wins, Challenges, One Improvement) into the ending week's
//! note, then composes the forward goal — which lands in *next* week's
//! note as its `This Week's Goal`, the carry-forward hand-off.
//!
//! The prose sections (Wins, Challenges) open in `$EDITOR`, pre-seeded
//! with their current content so the user edits in place (#230); the
//! single-line `One Improvement` and the forward goal stay plain text
//! prompts.
//!
//! Non-interactive (or piped) runs print the current note instead of
//! prompting, so the command stays scriptable and TTY-test friendly.
//!
//! `cdno review monthly` is the calendar-month analogue: it walks the
//! monthly note's three sections (Wins, Themes, Next Month's Focus) into
//! the current month's note. Unlike the weekly ritual there is no
//! cross-note carry-forward — the monthly note links (never copies) its
//! weeks via the scaffolded `## Weeks` block, so every section it
//! composes lands in the same month's note.

use std::path::Path;

use anyhow::{Context, Result};
use cdno_domain::{MonthlySection, WeeklySection};
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
    /// Walk the monthly sections (Wins, Themes, Next Month's Focus) into
    /// this month's note. Reads the current note when not interactive.
    Monthly,
}

pub fn run(
    root: &Path,
    today: NaiveDate,
    command: ReviewCommands,
    no_interactive: bool,
) -> Result<()> {
    match command {
        ReviewCommands::Weekly => weekly(root, today, no_interactive),
        ReviewCommands::Monthly => monthly(root, today, no_interactive),
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

    println!(
        "Weekly review. Wins and Challenges open in your editor, pre-filled with the \
         current content — edit in place and save. One Improvement and next week's goal \
         are single-line prompts. Leaving a section blank (or unchanged) skips it.\n"
    );
    if view.exists {
        println!(
            "Current note:\n{}",
            crate::commands::weekly::render(&view, today).trim_end()
        );
    }

    // Pre-seed the prose editors with each section's current content (if
    // the note exists) so the user edits in place. Parse the note once.
    let current = view
        .exists
        .then(|| cdno_core::markdown::MarkdownDocument::parse(&view.markdown).ok())
        .flatten();

    let mut saved_path: Option<String> = None;
    for section in REVIEW_SECTIONS {
        // Prose sections open in `$EDITOR`, pre-seeded with their current
        // content (#230); the single-line `One Improvement` stays a plain
        // text prompt.
        let entry = match section {
            WeeklySection::Wins | WeeklySection::Challenges => {
                let prefill = current
                    .as_ref()
                    .and_then(|d| d.section(section.heading()).ok())
                    .unwrap_or("")
                    .trim();
                prompt::prompt_editor(section.heading(), prefill)?
            }
            _ => prompt::prompt_text(section.heading())?,
        };
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

/// The monthly sections, in ritual order. Wins and Themes reflect on the
/// month; Next Month's Focus is the forward anchor. Unlike the weekly
/// goal, all three land in the *same* (current) month's note — the
/// monthly note has no cross-note carry-forward.
const MONTHLY_SECTIONS: [MonthlySection; 3] = [
    MonthlySection::Wins,
    MonthlySection::Themes,
    MonthlySection::NextMonthsFocus,
];

fn monthly(root: &Path, today: NaiveDate, no_interactive: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let view = vault
        .read_monthly_note(today)
        .context("reading the monthly note")?;

    // No TTY: show the current note rather than prompting into the void.
    // Reuse `cdno monthly`'s renderer so the two read paths match
    // (frontmatter stripped, same no-note placeholder).
    if !prompt::is_interactive(no_interactive) {
        print!("{}", crate::commands::monthly::render(&view, today));
        return Ok(());
    }

    println!(
        "Monthly review. Wins and Themes open in your editor, pre-filled with the \
         current content — edit in place and save. Next Month's Focus is a single-line \
         prompt. Leaving a section blank (or unchanged) skips it.\n"
    );
    if view.exists {
        println!(
            "Current note:\n{}",
            crate::commands::monthly::render(&view, today).trim_end()
        );
    }

    // Pre-seed the prose editors with each section's current content (if
    // the note exists) so the user edits in place. Parse the note once.
    let current = view
        .exists
        .then(|| cdno_core::markdown::MarkdownDocument::parse(&view.markdown).ok())
        .flatten();

    let mut saved_path: Option<String> = None;
    for section in MONTHLY_SECTIONS {
        // Prose sections open in `$EDITOR`, pre-seeded with their current
        // content; the forward-looking Next Month's Focus stays a plain
        // single-line text prompt (mirrors the weekly goal).
        let entry = match section {
            MonthlySection::Wins | MonthlySection::Themes => {
                let prefill = current
                    .as_ref()
                    .and_then(|d| d.section(section.heading()).ok())
                    .unwrap_or("")
                    .trim();
                prompt::prompt_editor(section.heading(), prefill)?
            }
            _ => prompt::prompt_text(section.heading())?,
        };
        if entry.trim().is_empty() {
            continue;
        }
        // `append: false` — compose/overwrite the section for this pass.
        // `upsert_monthly_section` returns the note path, so we don't need
        // a second read to report where it landed.
        let path = vault
            .upsert_monthly_section(today, section, &entry, false)
            .with_context(|| format!("writing the '{}' section", section.heading()))?;
        saved_path = Some(path.to_string());
    }

    match saved_path {
        Some(path) => println!("\nMonthly review saved to {path}."),
        None => println!("\nNothing entered \u{2014} monthly note unchanged."),
    }
    Ok(())
}
