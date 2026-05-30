//! `cdno questions` — list active questions grouped by domain.
//!
//! Top-level rather than a `cdno question` subcommand: this is the
//! frequently-called orientation surface (multiple times a week,
//! during reviews and at the start of focused work blocks), while
//! `cdno question {park,answer,…}` are infrequent lifecycle ops. The
//! shape mirrors `cdno commitments` for the same reason.

use std::path::Path;

use anyhow::{Context, Result};

use cdno_domain::QuestionSummary;
use cdno_domain::frontmatter::QuestionDomain;

use crate::bootstrap;

pub fn run(root: &Path) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let active = vault
        .active_questions()
        .context("listing active questions")?;
    print!("{}", render(&active));
    Ok(())
}

/// Render the active-questions output, grouped by domain. Public so
/// tests can assert formatted text without capturing stdout (same
/// seam as `cdno orient` / `cdno commitments` / `cdno portfolio
/// list`).
pub fn render(active: &[QuestionSummary]) -> String {
    let mut out = String::from("Active questions\n");
    if active.is_empty() {
        out.push_str(
            "  (none \u{2014} create one with `cdno question create --domain research --text ...`)\n",
        );
        return out;
    }
    // Two passes so the domain headings come out in a stable order
    // (Research, then Life) regardless of how active_questions
    // happened to sort across enums.
    for domain in QuestionDomain::ALL {
        let in_domain: Vec<&QuestionSummary> =
            active.iter().filter(|q| q.domain == domain).collect();
        if in_domain.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{}\n", capitalise_first(domain.as_str())));
        for q in in_domain {
            let text = if q.question_text.is_empty() {
                "(no H1)".to_owned()
            } else {
                q.question_text.clone()
            };
            out.push_str(&format!("  {} \u{2014} {text}\n", q.slug));
        }
    }
    out
}

fn capitalise_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
