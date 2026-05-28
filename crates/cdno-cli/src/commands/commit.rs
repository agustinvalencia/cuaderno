//! `cdno commit` subcommands: create a standalone commitment note,
//! mark one as completed. Thin clap-to-domain layer over
//! [`cdno_domain::Vault::create_commitment`] and
//! [`cdno_domain::Vault::complete_commitment`].
//!
//! Mirrors the `cdno project` surface (`create` / `done` verbs) for
//! consistency. The issue's literal syntax was
//! `cdno commit "title" --due ...` (positional title at the verb
//! level), but a verb-explicit shape keeps the dispatch flat and
//! matches every other subcommand group.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime};
use clap::Subcommand;

use cdno_domain::frontmatter::Context as CommitmentContext;

use crate::bootstrap;
use crate::commands::project::parse_iso_date;

#[derive(Debug, Subcommand)]
pub enum CommitCommands {
    /// Create an active commitment at `commitments/<slug>.md`.
    Create {
        /// Commitment title (also used as the body heading; slugged for
        /// the filename).
        title: String,
        /// Due date, `YYYY-MM-DD`.
        #[arg(long, value_parser = parse_iso_date)]
        due: NaiveDate,
        /// Life-domain context (work, personal, home-family, ...).
        #[arg(long)]
        context: CommitmentContext,
    },

    /// Mark a commitment as completed: stamps `status` and `completed`,
    /// moves the note to `commitments/_done/<year>/<slug>.md`.
    Done {
        /// Slug of the active commitment to complete.
        slug: String,
    },
}

pub fn run(root: &Path, at: NaiveDateTime, command: CommitCommands) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    match command {
        CommitCommands::Create {
            title,
            due,
            context,
        } => {
            let path = vault
                .create_commitment(at, &title, due, context)
                .context("creating commitment")?;
            println!("Created {path}");
        }
        CommitCommands::Done { slug } => {
            let path = vault
                .complete_commitment(at, &slug)
                .context("completing commitment")?;
            println!("Completed at {path}");
        }
    }
    Ok(())
}
