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

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDateTime;

use cdno_domain::Vault;

use crate::bootstrap;
use crate::prompt;

#[allow(clippy::too_many_arguments)]
pub fn run(
    root: &Path,
    at: NaiveDateTime,
    portfolio: Option<String>,
    source: Option<String>,
    origin: Option<String>,
    content: String,
    attach: Option<PathBuf>,
    move_after: bool,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    // `--json` implies non-interactive: prompts/confirms print to stdout,
    // which would corrupt the JSON result. Scripted callers pass full args.
    let interactive = prompt::is_interactive(no_interactive || json);

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
    // `content` stays optional — no prompt. For a plain note it's the
    // body; with `--attach` it's the abstract.

    if prompted
        && !prompt::confirm_preview(&summarise_filing(
            &portfolio,
            &source,
            &origin,
            &content,
            attach.as_deref(),
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    let path = match &attach {
        Some(artefact) => {
            let stub = vault
                .file_attachment(at, &portfolio, artefact, &source, &origin, &content)
                .context("filing attachment")?;
            if move_after {
                std::fs::remove_file(artefact).with_context(|| {
                    format!(
                        "copied into the vault, but failed to remove the source: {}",
                        artefact.display()
                    )
                })?;
            }
            stub
        }
        None => file_via_vault(&vault, at, &portfolio, &source, &origin, &content)?,
    };
    crate::output::emit_write_result(json, &path.to_string(), &format!("Filed at {path}"))?;
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

fn summarise_filing(
    portfolio: &str,
    source: &str,
    origin: &str,
    content: &str,
    attach: Option<&Path>,
) -> String {
    let body_preview = if content.trim().is_empty() {
        "(empty \u{2014} edit the file after creation)".to_owned()
    } else {
        let first_line = content.lines().next().unwrap_or("");
        format!("\"{first_line}\"")
    };
    let mut out = format!(
        "About to file evidence:\n  portfolio: {portfolio}\n  source:    {source}\n  origin:    {origin}"
    );
    if let Some(path) = attach {
        out.push_str(&format!("\n  attach:    {}", path.display()));
    }
    out.push_str(&format!("\n  content:   {body_preview}"));
    out
}
