//! `cdno review weekly` — the guided weekly-review ritual.
//!
//! Distinct from `cdno weekly` (which just *reads* the note) and the
//! low-level `upsert_weekly_section`: this walks the four review
//! sections (Wins, Challenges, One Improvement, Next Week's Focus) and
//! composes each interactively, writing them to the week's note.
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
    /// Walk the weekly review sections and compose each into this
    /// week's note. Reads the current note when not interactive.
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

/// The four review sections, in ritual order.
const WEEKLY_SECTIONS: [WeeklySection; 4] = [
    WeeklySection::Wins,
    WeeklySection::Challenges,
    WeeklySection::OneImprovement,
    WeeklySection::NextWeeksFocus,
];

fn weekly(root: &Path, today: NaiveDate, no_interactive: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let view = vault
        .read_weekly_note(today)
        .context("reading the weekly note")?;

    // No TTY: show the current note rather than prompting into the void.
    if !prompt::is_interactive(no_interactive) {
        if view.exists {
            print!("{}", view.markdown);
        } else {
            println!(
                "No weekly note yet for this week. Run `cdno review weekly` in a terminal to compose one."
            );
        }
        return Ok(());
    }

    println!("Weekly review. Leave a section blank to skip it.\n");
    if view.exists {
        println!("Current note:\n{}\n", view.markdown.trim_end());
    }

    let mut wrote_any = false;
    for section in WEEKLY_SECTIONS {
        let entry = prompt::prompt_text(section.heading())?;
        if entry.trim().is_empty() {
            continue;
        }
        // `append: false` — compose/overwrite the section for this pass.
        vault
            .upsert_weekly_section(today, section, &entry, false)
            .with_context(|| format!("writing the '{}' section", section.heading()))?;
        wrote_any = true;
    }

    if wrote_any {
        let path = vault
            .read_weekly_note(today)
            .map(|v| v.path.to_string())
            .unwrap_or_default();
        println!("\nWeekly review saved to {path}.");
    } else {
        println!("\nNothing entered \u{2014} weekly note unchanged.");
    }
    Ok(())
}
