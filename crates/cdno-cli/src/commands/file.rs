//! `cdno file` — file a piece of evidence into a portfolio.
//!
//! Top-level rather than a `cdno portfolio` subcommand: filing is a
//! routine action a researcher does several times a week, while
//! portfolio create / list / show are infrequent meta-operations.
//!
//! Required fields (`portfolio`, `source`, `origin`) are clap-optional
//! and gathered interactively when missing in a TTY. `content` is
//! genuinely optional and defaults to an empty body — the user can
//! flesh out the note in their editor after creation, which matches
//! how the design (§5.5) describes the typical CLI workflow.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDateTime;

use cdno_domain::Vault;

use crate::bootstrap;
use crate::prompt;

pub fn run(
    root: &Path,
    at: NaiveDateTime,
    portfolio: Option<String>,
    source: Option<String>,
    origin: Option<String>,
    content: String,
    no_interactive: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let interactive = prompt::is_interactive(no_interactive);

    let mut prompted = false;
    let portfolio =
        prompt::gather_or_error(portfolio, "portfolio", interactive, &mut prompted, || {
            prompt::prompt_portfolio(&vault, at.date())
        })?;
    let source = prompt::gather_or_error(source, "source", interactive, &mut prompted, || {
        prompt::prompt_text("Source (citation, experiment id, conversation, ...)")
    })?;
    let origin = prompt::gather_or_error(origin, "origin", interactive, &mut prompted, || {
        prompt::prompt_text("Origin wikilink target (e.g. projects/foo)")
    })?;
    // `content` stays optional — no prompt. The CLI prints the path
    // and the user opens the file to write the body.

    if prompted
        && !prompt::confirm_preview(&summarise_filing(&portfolio, &source, &origin, &content))?
    {
        println!("Aborted.");
        return Ok(());
    }

    let path = file_via_vault(&vault, at, &portfolio, &source, &origin, &content)?;
    println!("Filed at {path}");
    Ok(())
}

fn file_via_vault(
    vault: &Vault,
    at: NaiveDateTime,
    portfolio: &str,
    source: &str,
    origin: &str,
    content: &str,
) -> Result<cdno_core::path::VaultPath> {
    vault
        .file_evidence(at, portfolio, source, origin, content)
        .context("filing evidence")
}

fn summarise_filing(portfolio: &str, source: &str, origin: &str, content: &str) -> String {
    let body_preview = if content.trim().is_empty() {
        "(empty \u{2014} edit the file after creation)".to_owned()
    } else {
        let first_line = content.lines().next().unwrap_or("");
        format!("\"{first_line}\"")
    };
    format!(
        "About to file evidence:\n  portfolio: {portfolio}\n  source:    {source}\n  origin:    {origin}\n  content:   {body_preview}"
    )
}
