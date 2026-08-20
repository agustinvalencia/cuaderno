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
use crate::output::card::{Card, render_cards};
use crate::output::style::{Accent, Palette, Role};

pub fn run(root: &Path, json: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let active = vault
        .active_questions()
        .context("listing active questions")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&active)?);
    } else {
        print!("{}", render(&active));
    }
    Ok(())
}

/// Render the active-questions output, grouped by domain. Public so
/// tests can assert formatted text without capturing stdout (same
/// seam as `cdno orient` / `cdno commitments` / `cdno portfolio
/// list`).
pub fn render(active: &[QuestionSummary]) -> String {
    let palette = Palette::active();
    let mut out = format!("{}\n", palette.paint(Role::Heading, "Active questions"));
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
        // The heading hugs its section, matching the table sections in
        // `cdno orient`; the blank line is what separates one card from
        // the next, so spending one here too would blur the two.
        out.push_str(&format!(
            "\n{}\n",
            palette.paint(Role::Heading, &capitalise_first(domain.as_str()))
        ));
        // A research question is a sentence, not a field, so each one is
        // a card: the slug reads as a title and the question wraps below
        // it, rather than the two competing for width in one row.
        let cards: Vec<Card> = in_domain
            .iter()
            .map(|q| {
                let card = Card::new(&q.slug).accent(Accent::for_question(q.domain));
                if q.question_text.is_empty() {
                    card.muted("(no H1)")
                } else {
                    card.prose(&q.question_text)
                }
            })
            .collect();
        out.push_str(&render_cards(
            &cards,
            &palette,
            crate::output::render_width(),
        ));
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
