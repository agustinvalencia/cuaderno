//! `cdno question` subcommands: create, park, answer, retire,
//! activate. The top-level list lives as its own verb (`cdno
//! questions`) because it's the daily-frequency call against the
//! question system, while these subcommands are infrequent
//! lifecycle ops a researcher reaches for during reviews.
//!
//! Each status transition is its own named verb (rather than a
//! generic `set-status`) to give the user a clear sentence: "park
//! this", "answer this", "retire this". Mirrors the
//! `cdno project park` / `activate` shape.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use clap::Subcommand;
use clap_complete::engine::ArgValueCompleter;

use cdno_domain::Vault;
use cdno_domain::frontmatter::{QuestionDomain, QuestionStatus};

use crate::bootstrap;
use crate::completions;
use crate::prompt;

#[derive(Debug, Subcommand)]
pub enum QuestionCommands {
    /// Create a new question note under `questions/<domain>/<slug>.md`.
    /// The slug is derived from the question text.
    Create {
        /// Which folder the question lives under (`research` or `life`).
        #[arg(long)]
        domain: Option<QuestionDomain>,
        /// The question text. Becomes the body H1.
        #[arg(long)]
        text: Option<String>,
    },

    /// Park a question: set status to `parked`. Pick from active
    /// questions.
    Park {
        /// Question slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_question))]
        slug: Option<String>,
    },

    /// Answer a question: set status to `answered`. Pick from active
    /// questions.
    Answer {
        /// Question slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_question))]
        slug: Option<String>,
    },

    /// Retire a question: set status to `retired`. Pick from anything
    /// not already retired.
    Retire {
        /// Question slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_question))]
        slug: Option<String>,
    },

    /// Activate a question: set status to `active`. Pick from anything
    /// not already active.
    Activate {
        /// Question slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_question))]
        slug: Option<String>,
    },
}

pub fn run(
    root: &Path,
    at: NaiveDateTime,
    command: QuestionCommands,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    // `--json` implies non-interactive: prompts/confirms print to stdout,
    // which would corrupt the JSON result. Scripted callers pass full args.
    let interactive = prompt::is_interactive(no_interactive || json);
    match command {
        QuestionCommands::Create { domain, text } => {
            create(&vault, at, domain, text, interactive, json)
        }
        QuestionCommands::Park { slug } => {
            transition(&vault, at, slug, QuestionStatus::Parked, interactive, json)
        }
        QuestionCommands::Answer { slug } => transition(
            &vault,
            at,
            slug,
            QuestionStatus::Answered,
            interactive,
            json,
        ),
        QuestionCommands::Retire { slug } => {
            transition(&vault, at, slug, QuestionStatus::Retired, interactive, json)
        }
        QuestionCommands::Activate { slug } => {
            transition(&vault, at, slug, QuestionStatus::Active, interactive, json)
        }
    }
}

fn create(
    vault: &Vault,
    at: NaiveDateTime,
    domain: Option<QuestionDomain>,
    text: Option<String>,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    let domain = prompt::gather_or_error(domain, "domain", interactive, &mut prompted, || {
        prompt::prompt_question_domain()
    })?;
    let text = prompt::gather_or_error(text, "text", interactive, &mut prompted, || {
        prompt::prompt_text("Question")
    })?;
    if prompted
        && !prompt::confirm_preview(&format!(
            "About to create question:\n  domain: {}\n  text:   {text}",
            domain.as_str()
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }
    let path = vault
        .create_question(at, domain, &text)
        .context("creating question")?;
    crate::output::emit_write_result(json, &path.to_string(), &format!("Created {path}"))?;
    Ok(())
}

/// Shared body of park / answer / retire / activate. The verb chooses
/// the eligibility filter (which statuses can pick `target`) and the
/// label shown in the picker; everything else is identical.
fn transition(
    vault: &Vault,
    at: NaiveDateTime,
    slug: Option<String>,
    target: QuestionStatus,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let (allowed, verb, picker_label) = match target {
        QuestionStatus::Parked => (vec![QuestionStatus::Active], "park", "Question to park"),
        QuestionStatus::Answered => (vec![QuestionStatus::Active], "answer", "Question to answer"),
        QuestionStatus::Retired => (
            // Anything not already retired.
            vec![
                QuestionStatus::Active,
                QuestionStatus::Parked,
                QuestionStatus::Answered,
            ],
            "retire",
            "Question to retire",
        ),
        QuestionStatus::Active => (
            // Anything not already active.
            vec![
                QuestionStatus::Parked,
                QuestionStatus::Answered,
                QuestionStatus::Retired,
            ],
            "activate",
            "Question to activate",
        ),
    };

    let mut prompted = false;
    let slug = prompt::gather_or_error(slug, "slug", interactive, &mut prompted, || {
        prompt::prompt_question(vault, &allowed, picker_label)
    })?;
    if prompted && !prompt::confirm_preview(&format!("About to {verb} question '{slug}'"))? {
        println!("Aborted.");
        return Ok(());
    }
    let path = vault
        .set_question_status(at, &slug, target)
        .with_context(|| format!("{verb}ing question"))?;
    let message = format!("{} {path}", capitalise_first(&past_tense(verb)));
    crate::output::emit_write_result(json, &path.to_string(), &message)?;
    Ok(())
}

/// `park` → `Parked`, `answer` → `Answered`, etc. Keeps the success
/// line idiomatic ("Parked at ...") without a per-verb match arm.
fn past_tense(verb: &str) -> String {
    if verb.ends_with('e') {
        format!("{verb}d")
    } else {
        format!("{verb}ed")
    }
}

fn capitalise_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
