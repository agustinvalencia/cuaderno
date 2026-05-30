//! `cdno` — command-line interface for Cuaderno vaults.
//!
//! Thin shim: parses arguments, resolves the path arg or discovers
//! the vault root from CWD, and delegates to a library handler. All
//! real work lives in [`cdno_cli`].

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use chrono::{Local, NaiveDateTime};
use clap::{Parser, Subcommand};

use cdno_cli::commands::action::ActionCommands;
use cdno_cli::commands::commit::CommitCommands;
use cdno_cli::commands::portfolio::PortfolioCommands;
use cdno_cli::commands::project::ProjectCommands;
use cdno_cli::commands::question::QuestionCommands;
use cdno_cli::commands::stewardship::StewardshipCommands;
use cdno_cli::{bootstrap, commands};
use cdno_domain::frontmatter::EnergyLevel;

#[derive(Debug, Parser)]
#[command(
    name = "cdno",
    about = "Cuaderno: a Research Logbook Method vault manager",
    version
)]
struct Cli {
    /// Disable interactive prompts; missing required args become
    /// errors rather than prompting. Useful for scripts and CI.
    /// Always implicit when stdout is not a TTY.
    #[arg(long, global = true)]
    no_interactive: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Create a new vault: folder tree, .cuaderno/ config, default templates.
    Init {
        /// Target directory. Defaults to the current working directory.
        path: Option<PathBuf>,
    },

    /// Append a log entry to today's daily note (or a chosen moment).
    Log {
        /// The log message. Quote if it contains spaces.
        message: String,

        /// Override the timestamp. Accepts `YYYY-MM-DDTHH:MM:SS` or
        /// `YYYY-MM-DDTHH:MM`. Defaults to now.
        #[arg(long, value_name = "TIMESTAMP")]
        at: Option<String>,
    },

    /// Validate every indexed note and report frontmatter problems.
    Lint,

    /// Capture a quick note into `inbox/` with a slug-based filename.
    Capture {
        /// The note text. Quote if it contains spaces.
        text: String,
    },

    /// Manage project maps: create, update state, add/complete actions
    /// and milestones, park/activate, and list/show.
    Project {
        #[command(subcommand)]
        subcommand: ProjectCommands,
    },

    /// Daily orientation: commitments due soon, active projects, and a
    /// suggested starting point.
    Orient {
        /// Bias the suggested starting point toward this energy level
        /// (deep, medium, or light).
        #[arg(long)]
        energy: Option<EnergyLevel>,
    },

    /// Quick snapshot: active projects and their top next actions.
    Status,

    /// Manage actions: add (with optional --note), promote a bullet to
    /// a manifest note, complete, and list.
    Action {
        #[command(subcommand)]
        subcommand: ActionCommands,
    },

    /// Manage portfolios: create, list, show. Filing evidence into a
    /// portfolio is the separate `cdno file` verb (it's a routine
    /// action; portfolios manage the folder + index).
    Portfolio {
        #[command(subcommand)]
        subcommand: PortfolioCommands,
    },

    /// File a piece of evidence into a portfolio.
    File {
        /// Portfolio slug.
        #[arg(long)]
        portfolio: Option<String>,
        /// Citation, experiment id, conversation reference, …
        #[arg(long)]
        source: Option<String>,
        /// Bare wikilink target to whatever produced this evidence
        /// (e.g. `"projects/foo"`); the CLI wraps it.
        #[arg(long)]
        origin: Option<String>,
        /// Inline body. Optional; defaults to empty so the user can
        /// flesh out the note in their editor after creation.
        #[arg(long, default_value = "")]
        content: String,
    },

    /// Manage question notes: create, then status transitions
    /// (park / answer / retire / activate). Each transition logs to
    /// today's daily note.
    Question {
        #[command(subcommand)]
        subcommand: QuestionCommands,
    },

    /// List active questions grouped by domain (research, life). The
    /// frequently-called orientation surface against the question
    /// system; pair with `cdno question {park,answer,…}` for
    /// lifecycle changes.
    Questions,

    /// Manage stewardship dashboards: create (flat or expanded with
    /// `--tracking`), list, show, and append a periodic commitment
    /// line to the dashboard's `## Periodic Commitments` section.
    Stewardship {
        #[command(subcommand)]
        subcommand: StewardshipCommands,
    },

    /// File a tracking note under an expanded stewardship. Activity
    /// is positional (e.g. `cdno track gym`); built-in templates for
    /// gym/body/swim plus a generic fallback.
    Track {
        /// Activity (gym, body, swim, or a user-defined slug).
        activity: String,
        /// Stewardship slug. Defaults to the only expanded
        /// stewardship if there's exactly one; otherwise required.
        #[arg(long)]
        stewardship: Option<String>,
        /// Bare slug of a routine doc — wrapped into
        /// `[[stewardships/<stewardship>/routines/<routine>]]` and
        /// substituted into the template's `routine:` field.
        #[arg(long)]
        routine: Option<String>,
        /// Inline body. Optional; defaults to empty so the user can
        /// fill in tables or notes after creation.
        #[arg(long, default_value = "")]
        content: String,
    },

    /// Manage standalone commitments: create and complete.
    Commit {
        #[command(subcommand)]
        subcommand: CommitCommands,
    },

    /// List aggregated commitments across the vault — project hard
    /// milestones, standalone commitment notes, and self-imposed
    /// action-note deadlines — sorted by date with overdue flagged.
    Commitments {
        /// Lookahead in weeks. The standing 30-day overdue look-back
        /// from the aggregation query always applies on top.
        #[arg(long, default_value_t = 2)]
        weeks: u32,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { path } => {
            let target = match path {
                Some(p) => p,
                None => std::env::current_dir()
                    .context("could not determine the current working directory")?,
            };
            commands::init::run(&target)
        }
        Commands::Log { message, at } => {
            let root = discover_vault_root_or_error()?;
            let at = match at {
                Some(s) => parse_timestamp(&s)?,
                None => Local::now().naive_local(),
            };
            commands::log::run(&root, at, &message)
        }
        Commands::Lint => {
            let root = discover_vault_root_or_error()?;
            commands::lint::run(&root)
        }
        Commands::Capture { text } => {
            let root = discover_vault_root_or_error()?;
            commands::capture::run(&root, Local::now().naive_local(), &text)
        }
        Commands::Project { subcommand } => {
            let root = discover_vault_root_or_error()?;
            commands::project::run(
                &root,
                Local::now().naive_local(),
                subcommand,
                cli.no_interactive,
            )
        }
        Commands::Orient { energy } => {
            let root = discover_vault_root_or_error()?;
            commands::orient::run(&root, Local::now().date_naive(), energy)
        }
        Commands::Status => {
            let root = discover_vault_root_or_error()?;
            commands::status::run(&root, Local::now().date_naive())
        }
        Commands::Action { subcommand } => {
            let root = discover_vault_root_or_error()?;
            commands::action::run(
                &root,
                Local::now().naive_local(),
                subcommand,
                cli.no_interactive,
            )
        }
        Commands::Portfolio { subcommand } => {
            let root = discover_vault_root_or_error()?;
            commands::portfolio::run(
                &root,
                Local::now().naive_local(),
                subcommand,
                cli.no_interactive,
            )
        }
        Commands::File {
            portfolio,
            source,
            origin,
            content,
        } => {
            let root = discover_vault_root_or_error()?;
            commands::file::run(
                &root,
                Local::now().naive_local(),
                portfolio,
                source,
                origin,
                content,
                cli.no_interactive,
            )
        }
        Commands::Question { subcommand } => {
            let root = discover_vault_root_or_error()?;
            commands::question::run(
                &root,
                Local::now().naive_local(),
                subcommand,
                cli.no_interactive,
            )
        }
        Commands::Questions => {
            let root = discover_vault_root_or_error()?;
            commands::questions::run(&root)
        }
        Commands::Stewardship { subcommand } => {
            let root = discover_vault_root_or_error()?;
            commands::stewardship::run(
                &root,
                Local::now().naive_local(),
                subcommand,
                cli.no_interactive,
            )
        }
        Commands::Track {
            activity,
            stewardship,
            routine,
            content,
        } => {
            let root = discover_vault_root_or_error()?;
            commands::track::run(
                &root,
                Local::now().naive_local(),
                activity,
                stewardship,
                routine,
                content,
                cli.no_interactive,
            )
        }
        Commands::Commit { subcommand } => {
            let root = discover_vault_root_or_error()?;
            commands::commit::run(
                &root,
                Local::now().naive_local(),
                subcommand,
                cli.no_interactive,
            )
        }
        Commands::Commitments { weeks } => {
            let root = discover_vault_root_or_error()?;
            commands::commitments::run(&root, Local::now().date_naive(), weeks)
        }
    }
}

/// Resolve the vault root for commands that operate on an existing
/// vault. Walks ancestors of CWD looking for the `.cuaderno/` marker;
/// errors with a `cdno init` hint when none is found.
fn discover_vault_root_or_error() -> Result<PathBuf> {
    let cwd =
        std::env::current_dir().context("could not determine the current working directory")?;
    bootstrap::discover_vault_root(&cwd).ok_or_else(|| {
        anyhow!(
            "{} is not inside a Cuaderno vault; run `cdno init` to create one",
            cwd.display()
        )
    })
}

/// Permissive timestamp parser for `--at`. Accepts seconds-precision
/// or minutes-precision forms; errors with the offending input
/// preserved so the user sees what they typed.
fn parse_timestamp(s: &str) -> Result<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M"))
        .with_context(|| format!("could not parse `{s}` as a timestamp"))
}
