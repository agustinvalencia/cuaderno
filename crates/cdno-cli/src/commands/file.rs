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

/// The file format of the evidence note `cdno file` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum FileFormat {
    #[default]
    Markdown,
    Typst,
}

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
    var: Vec<(String, String)>,
    format: FileFormat,
    body_file: Option<PathBuf>,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    let typst = format == FileFormat::Typst;
    let adopting = typst && attach.is_some();
    if adopting && (body_file.is_some() || !content.trim().is_empty()) {
        anyhow::bail!(
            "--attach with --format typst adopts the file as the note; drop --body-file and --content"
        );
    }
    if !typst && body_file.is_some() {
        anyhow::bail!("--body-file applies to --format typst; use --content for a Markdown note");
    }
    if typst && !var.is_empty() {
        return Err(crate::commands::rejection::typst_refusal(
            cdno_domain::error::DomainError::VarsUnsupportedForTypst.into(),
            json,
        ));
    }
    let (vault, _report) = bootstrap::open_vault(root)?;
    // `--json` implies non-interactive: prompts/confirms print to stdout,
    // which would corrupt the JSON result. Scripted callers pass full args.
    let interactive = prompt::reports_interactively(no_interactive, json);

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

    // A Typst note is not templated and has no body default: the body is
    // demanded (or prompted), and `--var` is refused with the domain's text.
    let content = if adopting {
        String::new()
    } else if typst {
        let from_flag = match body_file {
            Some(p) => Some(
                std::fs::read_to_string(&p)
                    .with_context(|| format!("reading --body-file {}", p.display()))?,
            ),
            None if !content.trim().is_empty() => Some(content),
            None => None,
        };
        // A blank body is no body: refused as if the flag were missing.
        let from_flag = from_flag.filter(|b| !b.trim().is_empty());
        let body =
            prompt::gather_or_error(from_flag, "body-file", interactive, &mut prompted, || {
                prompt::prompt_editor("Body", "")
            })?;
        if body.trim().is_empty() {
            return Err(prompt::missing_flag("body-file"));
        }
        body
    } else {
        content
    };

    // Prompted template variables apply only to the templated (plain-note)
    // path; attachment stubs aren't templated, so `--var` is ignored there.
    let template_vars = if attach.is_none() && !typst {
        prompt::gather_template_vars(&vault, "evidence", None, &var, interactive, &mut prompted)?
    } else {
        std::collections::HashMap::new()
    };

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

    let mut rewrites = Vec::new();
    let path = match &attach {
        Some(artefact) if typst => {
            let adoption = cdno_core::adoption::read_typst_adoption(artefact, root)
                .map_err(cdno_domain::error::DomainError::from)
                .map_err(anyhow::Error::from)
                .map_err(|e| crate::commands::rejection::typst_refusal(e, json))?;
            let adopted = vault
                .adopt_typst_evidence(
                    at,
                    &portfolio,
                    &source,
                    &origin,
                    &adoption.contents,
                    &adoption.file_name,
                    adoption.assets,
                )
                .map_err(anyhow::Error::from)
                .map_err(|e| crate::commands::rejection::typst_refusal(e, json))?;
            if move_after {
                std::fs::remove_file(artefact).with_context(|| {
                    format!(
                        "adopted into the vault, but failed to remove the source: {}",
                        artefact.display()
                    )
                })?;
            }
            rewrites = adopted.rewrites;
            adopted.path
        }
        Some(artefact) => {
            let stub = vault
                .file_attachment(at, &portfolio, artefact, &source, &origin, &content)
                .context("filing attachment")
                .map_err(|e| crate::commands::rejection::typst_refusal(e, json))?;
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
        None if typst => vault
            .file_typst_evidence(at, &portfolio, &source, &origin, &content)
            .map_err(anyhow::Error::from)
            .map_err(|e| crate::commands::rejection::typst_refusal(e, json))?,
        None => file_via_vault(
            &vault,
            at,
            &portfolio,
            &source,
            &origin,
            &content,
            &template_vars,
        )
        .map_err(|e| crate::commands::rejection::typst_refusal(e, json))?,
    };
    if !adopting {
        crate::output::emit_write_result(json, &path.to_string(), &format!("Filed at {path}"))?;
    } else if json {
        let rewrites: Vec<_> = rewrites
            .iter()
            .map(|(was, now)| serde_json::json!({ "was": was, "now": now }))
            .collect();
        let payload = serde_json::json!({
            "path": path.to_string(),
            "message": format!("Filed at {path}"),
            "rewrites": rewrites,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("Filed at {path}");
        for (was, now) in &rewrites {
            println!("  rewrote {was} -> {now}");
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)] // thin gather→create passthrough
fn file_via_vault(
    vault: &Vault,
    at: NaiveDateTime,
    portfolio: &str,
    source: &str,
    origin: &str,
    content: &str,
    prompted: &std::collections::HashMap<String, String>,
) -> Result<cdno_core::path::VaultPath> {
    vault
        .file_evidence_with_vars(at, portfolio, source, origin, content, prompted)
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
