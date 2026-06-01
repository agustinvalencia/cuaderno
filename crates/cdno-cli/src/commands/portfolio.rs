//! `cdno portfolio` subcommands: create, list, show.
//!
//! Filing evidence is its own top-level verb (`cdno file`) per the
//! design — portfolios manage the folder + index, while filing a piece
//! of evidence is a separate routine action the researcher does much
//! more often.
//!
//! Renderers (`render_list`, `render_show`) are `pub` so tests can
//! assert on the formatted output without capturing stdout, matching
//! the seam used by `cdno orient` / `cdno commitments`.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use clap::Subcommand;
use clap_complete::engine::ArgValueCompleter;

use cdno_domain::frontmatter::{EvidenceFrontmatter, PortfolioFrontmatter};
use cdno_domain::{PortfolioSummary, Vault};

use cdno_core::path::VaultPath;

use crate::bootstrap;
use crate::completions;
use crate::prompt;

#[derive(Debug, Subcommand)]
pub enum PortfolioCommands {
    /// Create a new portfolio under `portfolios/<slug>/`. The slug is
    /// derived from the question.
    Create {
        /// The unifying question the portfolio collects evidence
        /// against.
        #[arg(long)]
        question: Option<String>,
        /// Bare wikilink target to a parent project (e.g.
        /// `"projects/surrogate-model"`). Optional — portfolios can
        /// stand alone.
        #[arg(long)]
        project: Option<String>,
    },

    /// List every portfolio with its evidence count and staleness.
    List,

    /// Show a portfolio's frontmatter and its evidence inventory.
    Show {
        /// Portfolio slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_portfolio))]
        portfolio: Option<String>,
    },
}

pub fn run(
    root: &Path,
    at: NaiveDateTime,
    command: PortfolioCommands,
    no_interactive: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let interactive = prompt::is_interactive(no_interactive);
    match command {
        PortfolioCommands::Create { question, project } => {
            create(&vault, at, question, project, interactive)
        }
        PortfolioCommands::List => {
            let summaries = vault
                .list_portfolios(at.date())
                .context("listing portfolios")?;
            print!("{}", render_list(&summaries));
            Ok(())
        }
        PortfolioCommands::Show { portfolio } => show(&vault, at, portfolio, interactive),
    }
}

fn create(
    vault: &Vault,
    at: NaiveDateTime,
    question: Option<String>,
    project: Option<String>,
    interactive: bool,
) -> Result<()> {
    let mut prompted = false;
    let question =
        prompt::gather_or_error(question, "question", interactive, &mut prompted, || {
            prompt::prompt_text("Question")
        })?;
    // `project` is genuinely optional — no prompt, no error if absent.

    if prompted {
        let project_label = project.as_deref().unwrap_or("(none)");
        if !prompt::confirm_preview(&format!(
            "About to create portfolio:\n  question: {question}\n  project:  {project_label}"
        ))? {
            println!("Aborted.");
            return Ok(());
        }
    }
    let path = vault
        .create_portfolio(at, &question, project.as_deref())
        .context("creating portfolio")?;
    println!("Created {path}");
    Ok(())
}

fn show(
    vault: &Vault,
    at: NaiveDateTime,
    portfolio: Option<String>,
    interactive: bool,
) -> Result<()> {
    // show is read-only — no confirm step, just gather the slug.
    let slug = match portfolio {
        Some(s) => s,
        None if interactive => prompt::prompt_portfolio(vault, at.date())?,
        None => return Err(prompt::missing_flag("portfolio")),
    };

    let fm = vault
        .get_portfolio(&slug)
        .with_context(|| format!("loading portfolio '{slug}'"))?;
    let entries = vault
        .get_portfolio_contents(&slug)
        .context("listing portfolio contents")?;
    let summaries = vault.list_portfolios(at.date())?;
    let summary = summaries.iter().find(|s| s.slug == slug);

    print!("{}", render_show(&slug, &fm, summary, &entries));
    Ok(())
}

/// Render `cdno portfolio list` output. Public so tests can assert on
/// the formatted text without capturing stdout.
pub fn render_list(summaries: &[PortfolioSummary]) -> String {
    let mut out = String::from("Portfolios\n");
    if summaries.is_empty() {
        out.push_str("  (no portfolios \u{2014} create one with `cdno portfolio create`)\n");
        return out;
    }
    for p in summaries {
        let badge = match p.staleness_days {
            Some(days) if p.evidence_count > 0 => {
                format!("{} evidence, last {} days ago", p.evidence_count, days)
            }
            _ => "no evidence yet".to_owned(),
        };
        out.push_str(&format!("  {} \u{2014} {} ({badge})\n", p.slug, p.question));
    }
    out
}

/// Render `cdno portfolio show` output. Public for test access.
pub fn render_show(
    slug: &str,
    fm: &PortfolioFrontmatter,
    summary: Option<&PortfolioSummary>,
    entries: &[(VaultPath, EvidenceFrontmatter)],
) -> String {
    let mut out = format!("{slug} \u{2014} {}\n", fm.question);
    out.push_str(&format!("Created: {}\n", fm.created));
    let project_label = fm.project.as_deref().unwrap_or("(none)");
    out.push_str(&format!("Project: {project_label}\n"));

    match summary {
        Some(s) if s.evidence_count > 0 => {
            let last = s
                .last_updated
                .map(|d| d.to_string())
                .unwrap_or_else(|| "?".to_owned());
            out.push_str(&format!(
                "\nEvidence ({} notes, last {last}):\n",
                s.evidence_count
            ));
        }
        _ => out.push_str("\nEvidence (none yet)\n"),
    }
    for (_path, ev) in entries {
        out.push_str(&format!(
            "  {}  {}  (origin: {})\n",
            ev.created, ev.source, ev.origin,
        ));
    }
    out
}
