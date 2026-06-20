//! `cdno triage` — drain the inbox.
//!
//! Capture is one-way (`cdno capture` drops a line into `inbox/`); this
//! is the other half of the loop. For each pending capture, the user
//! keeps it as a project action, discards it, or skips it. Routing to
//! an action reuses `add_action`, then the capture is discarded — the
//! domain has no bespoke "promote" op (see `Vault::discard_inbox_item`).
//!
//! Non-interactive runs (or `--no-interactive`) just list what's
//! pending, so the command stays scriptable and testable without a TTY.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use inquire::Select;

use crate::bootstrap;
use crate::prompt;

pub fn run(root: &Path, at: NaiveDateTime, no_interactive: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let items = vault.list_inbox().context("listing the inbox")?;

    if items.is_empty() {
        println!("Inbox empty \u{2014} nothing to triage.");
        return Ok(());
    }

    // Without a TTY we can't prompt; list what's pending instead.
    if !prompt::is_interactive(no_interactive) {
        println!("{} inbox item(s) pending triage:", items.len());
        for item in &items {
            println!("  {} \u{2014} {}", item.slug, item.text);
        }
        return Ok(());
    }

    for item in items {
        println!("\n{}", item.text);
        let choice = Select::new("Triage", vec!["keep as action", "discard", "skip"]).prompt()?;

        match choice {
            "keep as action" => {
                let project = prompt::prompt_project(&vault)?;
                let energy = prompt::prompt_energy()?;
                vault
                    .add_action(at, &project, &item.text, energy)
                    .with_context(|| format!("adding action to '{project}'"))?;
                vault
                    .discard_inbox_item(at, &item.slug)
                    .context("clearing the capture")?;
                println!("Kept as an action on '{project}'; capture cleared.");
            }
            "discard" => {
                vault
                    .discard_inbox_item(at, &item.slug)
                    .context("discarding the capture")?;
                println!("Discarded.");
            }
            _ => println!("Skipped."),
        }
    }

    Ok(())
}
