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

use cdno_domain::frontmatter::{EvidenceFrontmatter, PortfolioFrontmatter, QuestionStatus};
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
        /// against. If a question note already exists for the same
        /// text, the two are linked both ways (the question's `##
        /// Related Portfolios` and this portfolio's `## Related
        /// Questions`).
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

    /// Link an existing portfolio to an existing question, writing
    /// both ends: the question's `## Related Portfolios` and the
    /// portfolio's `## Related Questions`. The retrofit path for
    /// portfolios created before their question, or whose slug differs
    /// from it.
    Link {
        /// Portfolio slug (the `portfolios/<slug>/` folder name).
        #[arg(long, add = ArgValueCompleter::new(completions::complete_portfolio))]
        portfolio: Option<String>,
        /// Question slug, resolved across the `research` and `life`
        /// domains.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_question))]
        question: Option<String>,
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
        PortfolioCommands::Link {
            portfolio,
            question,
        } => link_to_question(&vault, at, portfolio, question, interactive),
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

fn link_to_question(
    vault: &Vault,
    at: NaiveDateTime,
    portfolio: Option<String>,
    question: Option<String>,
    interactive: bool,
) -> Result<()> {
    // Same gather -> confirm -> execute shape as the other write verbs.
    let mut prompted = false;
    let portfolio =
        prompt::gather_or_error(portfolio, "portfolio", interactive, &mut prompted, || {
            prompt::prompt_portfolio(vault, at.date())
        })?;
    // Any status is a valid link target — a question stays worth
    // collecting evidence against whether it's active, parked, or
    // already answered.
    let question =
        prompt::gather_or_error(question, "question", interactive, &mut prompted, || {
            prompt::prompt_question(vault, &QuestionStatus::ALL, "Question to link")
        })?;

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to link portfolio '{portfolio}' to question '{question}'"
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    let path = vault
        .link_portfolio_to_question(&portfolio, &question)
        .with_context(|| format!("linking portfolio '{portfolio}' to question '{question}'"))?;
    println!("Linked portfolio '{portfolio}' to question '{question}' ({path})");
    Ok(())
}

/// Render `cdno portfolio list` output. Public so tests can assert on
/// the formatted text without capturing stdout.
pub fn render_list(summaries: &[PortfolioSummary]) -> String {
    if summaries.is_empty() {
        return "Portfolios\n  (no portfolios \u{2014} create one with `cdno portfolio create`)\n"
            .to_owned();
    }
    // slug / question / staleness columns; the question wraps to the
    // terminal rather than running off the edge (#153).
    let mut table = crate::output::styled_table();
    for p in summaries {
        let badge = match p.staleness_days {
            Some(days) if p.evidence_count > 0 => {
                format!("{} evidence, last {} days ago", p.evidence_count, days)
            }
            _ => "no evidence yet".to_owned(),
        };
        table.add_row(vec![p.slug.clone(), p.question.clone(), badge]);
    }
    // Keep the slug and staleness badge whole; only the question reflows.
    crate::output::no_wrap_columns(&mut table, &[0, 2]);
    format!("Portfolios\n{}\n", crate::output::render(&table))
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
    if !entries.is_empty() {
        // created / source / origin columns; date and origin stay whole,
        // the source reflows (#153). Attachment stubs carry a media `kind`
        // (#154) — tag the source cell so a non-markdown artefact stays
        // visually distinct from prose evidence.
        let mut table = crate::output::styled_table();
        for (_path, ev) in entries {
            let tag = ev
                .kind
                .as_deref()
                .map(|k| format!("[{k}] "))
                .unwrap_or_default();
            table.add_row(vec![
                ev.created.to_string(),
                format!("{tag}{}", ev.source),
                ev.origin.clone(),
            ]);
        }
        crate::output::no_wrap_columns(&mut table, &[0, 2]);
        out.push_str(&crate::output::render(&table));
        out.push('\n');
    }
    out
}
