//! `cdno` — command-line interface for Cuaderno vaults.
//!
//! Thin shim: parses arguments, resolves the path arg or discovers
//! the vault root from CWD, and delegates to a library handler. All
//! real work lives in [`cdno_cli`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{Local, NaiveDate, NaiveDateTime};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::engine::ArgValueCompleter;
use clap_complete::env::CompleteEnv;

use cdno_cli::commands::action::ActionCommands;
use cdno_cli::commands::commit::CommitCommands;
use cdno_cli::commands::portfolio::PortfolioCommands;
use cdno_cli::commands::project::ProjectCommands;
use cdno_cli::commands::question::QuestionCommands;
use cdno_cli::commands::review::ReviewCommands;
use cdno_cli::commands::stewardship::StewardshipCommands;
use cdno_cli::commands::templates::TemplatesCommands;
use cdno_cli::completions;
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

    /// Path to the Cuaderno vault to operate on. Overrides both vault
    /// discovery and the `CUADERNO_VAULT_PATH` environment variable.
    /// When omitted, cdno discovers the vault by walking up from the
    /// current directory, then falls back to `CUADERNO_VAULT_PATH` —
    /// letting `cdno log`, `cdno capture`, etc. run from anywhere.
    #[arg(long, global = true, value_name = "PATH", value_hint = clap::ValueHint::DirPath)]
    vault: Option<PathBuf>,

    /// Emit machine-readable JSON instead of the formatted table.
    /// Read verbs (`commitments`, `questions`, `status`, `orient`,
    /// `search`, and the `list`/`show` verbs of `project`, `portfolio`,
    /// `stewardship`, plus `action list`) emit their listing/detail;
    /// write verbs (`log`, `capture`, `file`, `track`, and the
    /// create/update verbs of `project`, `action`, `portfolio`,
    /// `stewardship`, `question`, `commit`) emit a `{path, message}`
    /// result and run non-interactively. Ignored by maintenance/
    /// interactive/bootstrap commands (`init`, `lint`, `reindex`,
    /// `normalise`, `triage`, `review`, `weekly`).
    #[arg(long, global = true)]
    json: bool,

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
    /// Errors fail the command; warnings (e.g. broken wikilinks) are
    /// non-fatal unless `--strict` is given.
    Lint {
        /// Treat warnings as failures too (exit non-zero on any issue).
        #[arg(long)]
        strict: bool,
    },

    /// Rebuild the SQLite index from scratch off the markdown source of
    /// truth. The recovery path for a corrupt or stale index.
    Reindex,

    /// Reorder note frontmatter into the canonical key order of each
    /// note's template (a custom `.cuaderno/templates/` override if
    /// present, else the built-in). Notes cdno creates are already
    /// canonical; this fixes hand-authored or migrated ones. `--check`
    /// reports without writing (non-zero exit if any are out of order).
    Normalise {
        /// Report out-of-order notes without rewriting them.
        #[arg(long)]
        check: bool,
    },

    /// Capture a quick note into `inbox/` with a slug-based filename.
    Capture {
        /// The note text. Quote if it contains spaces.
        text: String,
    },

    /// Triage uncategorised `inbox/` captures: for each, keep it as a
    /// project action, discard it, or skip. Non-interactive runs just
    /// list what's pending.
    Triage,

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

    /// Guided review rituals. `review weekly` walks the retrospective
    /// sections (Wins, Challenges, One Improvement) into this week's note
    /// and sets next week's goal (its This Week's Goal) in next week's.
    Review {
        #[command(subcommand)]
        subcommand: ReviewCommands,
    },

    /// Show the weekly review/plan note: Wins, Challenges, One
    /// Improvement, and This Week's Goal. Defaults to this ISO week.
    Weekly {
        /// Any day in the target ISO week (YYYY-MM-DD). Defaults to this week.
        #[arg(long)]
        date: Option<NaiveDate>,
    },

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

    /// File a piece of evidence into a portfolio. With `--attach`, files
    /// a non-markdown artefact (PDF, image, video, …): the file is copied
    /// into the portfolio and a linked evidence stub is scaffolded beside
    /// it. Without `--attach`, writes a plain markdown evidence note.
    File {
        /// Portfolio slug.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_portfolio))]
        portfolio: Option<String>,
        /// Citation, experiment id, conversation reference, …
        #[arg(long)]
        source: Option<String>,
        /// Bare wikilink target to whatever produced this evidence
        /// (e.g. `"projects/foo"`); the CLI wraps it.
        #[arg(long)]
        origin: Option<String>,
        /// Inline body. For a plain note it's the content; with `--attach`
        /// it's the abstract. Optional; defaults to empty.
        #[arg(long, default_value = "")]
        content: String,
        /// Path to a non-markdown artefact to file as evidence. The file
        /// is copied into `portfolios/<slug>/<evidence-slug>/` and an
        /// evidence stub links to it.
        #[arg(long)]
        attach: Option<PathBuf>,
        /// With `--attach`, remove the source file after a successful
        /// copy (move instead of copy).
        #[arg(long)]
        r#move: bool,
        /// Value for a custom evidence template's prompted variable
        /// (`[variables.prompt]`), repeatable: `--var name=value`. Ignored
        /// with `--attach` (attachment stubs aren't templated).
        #[arg(long = "var", value_parser = cdno_cli::prompt::parse_key_val)]
        var: Vec<(String, String)>,
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

    /// Inspect note templates. `templates vars <type>` lists the
    /// `{{placeholders}}` a type's template supports, for writing a
    /// custom `.cuaderno/templates/` override.
    Templates {
        #[command(subcommand)]
        subcommand: TemplatesCommands,
    },

    /// Create and list notes of config-defined custom types (declared under
    /// `[note_types.<type>]`). `note create <type> --title … --field k=v` and
    /// `note list <type>`.
    Note {
        #[command(subcommand)]
        subcommand: cdno_cli::commands::note::NoteCommands,
    },

    /// Manage stewardship dashboards: create (flat or expanded with
    /// `--tracking`), list, show, and append a periodic commitment
    /// line to the dashboard's `## Periodic Commitments` section.
    Stewardship {
        #[command(subcommand)]
        subcommand: StewardshipCommands,
    },

    /// File a tracking note under an expanded stewardship. Activity
    /// is positional (e.g. `cdno track gym`). Uses a vault template
    /// `.cuaderno/templates/tracking-<activity>.md` if present, else a
    /// generic fallback (see `examples/templates/tracking/`).
    Track {
        /// Activity slug (e.g. gym, swim, reading — anything you track).
        activity: String,
        /// Stewardship slug. Defaults to the only expanded
        /// stewardship if there's exactly one; otherwise required.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_stewardship))]
        stewardship: Option<String>,
        /// Bare slug of a routine doc — wrapped into
        /// `[[stewardships/<stewardship>/routines/<routine>]]` and
        /// substituted into the template's `routine:` field. Only takes
        /// effect on a template that has a `routine:` field; the generic
        /// default has none, so this no-ops unless you use a variant template.
        #[arg(long)]
        routine: Option<String>,
        /// Inline body. Optional; defaults to empty so the user can
        /// fill in tables or notes after creation.
        #[arg(long, default_value = "")]
        content: String,
        /// Value for a custom tracking template's prompted variable
        /// (`[variables.prompt]`), repeatable: `--var name=value`.
        #[arg(long = "var", value_parser = cdno_cli::prompt::parse_key_val)]
        var: Vec<(String, String)>,
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

    /// Full-text search across all notes, ranked best-first. Free-text
    /// query with optional filters by note type, date window, and
    /// portfolio.
    Search {
        /// Search text. Matched case-insensitively; terms are ANDed.
        /// Quotes and operators are treated as literal words.
        query: String,
        /// Restrict to one note type (e.g. `daily`, `project`, `evidence`).
        #[arg(
            long = "type",
            value_name = "TYPE",
            add = ArgValueCompleter::new(completions::complete_note_type)
        )]
        note_type: Option<String>,
        /// Inclusive earliest note date (YYYY-MM-DD).
        #[arg(long, value_parser = cdno_cli::commands::project::parse_iso_date)]
        from: Option<NaiveDate>,
        /// Inclusive latest note date (YYYY-MM-DD).
        #[arg(long, value_parser = cdno_cli::commands::project::parse_iso_date)]
        to: Option<NaiveDate>,
        /// Restrict to notes in this portfolio.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_portfolio))]
        portfolio: Option<String>,
        /// Maximum results to return.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },

    /// Print a shell-completion script. Source it in your shell's
    /// rc file. Dynamic vault-aware suggestions for `--project`,
    /// `--portfolio`, `--stewardship`, `--slug` etc. are wired into
    /// the same script — the binary is re-invoked by the shell when
    /// you press TAB.
    Completions {
        /// Target shell (`bash`, `zsh`, `fish`, `elvish`, or
        /// `powershell`).
        shell: clap_complete::Shell,
    },
}

fn main() -> Result<()> {
    // Runtime completion intercept: when the shell invokes us with
    // the completion env var set, this returns candidates and exits
    // before `Cli::parse()` runs (so an unset vault is harmless).
    CompleteEnv::with_factory(Cli::command).complete();

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
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            let at = match at {
                Some(s) => parse_timestamp(&s)?,
                None => Local::now().naive_local(),
            };
            commands::log::run(&root, at, &message, cli.json)
        }
        Commands::Lint { strict } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::lint::run(&root, strict)
        }
        Commands::Reindex => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::reindex::run(&root)
        }
        Commands::Normalise { check } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::normalise::run(&root, check)
        }
        Commands::Capture { text } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::capture::run(&root, Local::now().naive_local(), &text, cli.json)
        }
        Commands::Triage => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::triage::run(&root, Local::now().naive_local(), cli.no_interactive)
        }
        Commands::Project { subcommand } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::project::run(
                &root,
                Local::now().naive_local(),
                subcommand,
                cli.no_interactive,
                cli.json,
            )
        }
        Commands::Orient { energy } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::orient::run(&root, Local::now().date_naive(), energy, cli.json)
        }
        Commands::Status => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::status::run(&root, Local::now().date_naive(), cli.json)
        }
        Commands::Review { subcommand } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::review::run(
                &root,
                Local::now().date_naive(),
                subcommand,
                cli.no_interactive,
            )
        }
        Commands::Weekly { date } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::weekly::run(&root, Local::now().date_naive(), date)
        }
        Commands::Action { subcommand } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::action::run(
                &root,
                Local::now().naive_local(),
                subcommand,
                cli.no_interactive,
                cli.json,
            )
        }
        Commands::Portfolio { subcommand } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::portfolio::run(
                &root,
                Local::now().naive_local(),
                subcommand,
                cli.no_interactive,
                cli.json,
            )
        }
        Commands::File {
            portfolio,
            source,
            origin,
            content,
            attach,
            r#move,
            var,
        } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::file::run(
                &root,
                Local::now().naive_local(),
                portfolio,
                source,
                origin,
                content,
                attach,
                r#move,
                var,
                cli.no_interactive,
                cli.json,
            )
        }
        Commands::Question { subcommand } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::question::run(
                &root,
                Local::now().naive_local(),
                subcommand,
                cli.no_interactive,
                cli.json,
            )
        }
        Commands::Questions => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::questions::run(&root, cli.json)
        }
        Commands::Templates { subcommand } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::templates::run(&root, subcommand, cli.json)
        }
        Commands::Note { subcommand } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::note::run(&root, Local::now().naive_local(), subcommand, cli.json)
        }
        Commands::Stewardship { subcommand } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::stewardship::run(
                &root,
                Local::now().naive_local(),
                subcommand,
                cli.no_interactive,
                cli.json,
            )
        }
        Commands::Track {
            activity,
            stewardship,
            routine,
            content,
            var,
        } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::track::run(
                &root,
                Local::now().naive_local(),
                activity,
                stewardship,
                routine,
                content,
                var,
                cli.no_interactive,
                cli.json,
            )
        }
        Commands::Commit { subcommand } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::commit::run(
                &root,
                Local::now().naive_local(),
                subcommand,
                cli.no_interactive,
                cli.json,
            )
        }
        Commands::Commitments { weeks } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::commitments::run(&root, Local::now().date_naive(), weeks, cli.json)
        }
        Commands::Search {
            query,
            note_type,
            from,
            to,
            portfolio,
            limit,
        } => {
            let root = resolve_vault_root_or_error(cli.vault.as_deref())?;
            commands::search::run(
                &root, &query, note_type, from, to, portfolio, limit, cli.json,
            )
        }
        Commands::Completions { shell } => {
            // Script emission needs no vault — sourcing is a shell-rc-time
            // operation; the vault may not exist yet (and the dynamic
            // engine's vault opens happen later, on TAB).
            completions::print_script(shell).context("emitting completion script")
        }
    }
}

/// Environment variable naming the vault to operate on when none is
/// discovered from the current directory. Shared with the MCP server,
/// which already honours the same name.
const ENV_VAULT_PATH: &str = "CUADERNO_VAULT_PATH";

/// Resolve the vault root for commands that operate on an existing
/// vault, reading the real CWD and environment and delegating the
/// precedence policy to [`bootstrap::resolve_vault_root`]. Errors with
/// a hint naming all three mechanisms when none resolves.
fn resolve_vault_root_or_error(vault_flag: Option<&Path>) -> Result<PathBuf> {
    let cwd =
        std::env::current_dir().context("could not determine the current working directory")?;
    let env_value = std::env::var(ENV_VAULT_PATH).ok();

    bootstrap::resolve_vault_root(vault_flag, &cwd, env_value.as_deref()).ok_or_else(|| {
        anyhow!(
            "{} is not inside a Cuaderno vault.\n\
             Point cdno at one with `--vault <path>`, set ${} to your vault, \
             or run `cdno init` to create one.",
            cwd.display(),
            ENV_VAULT_PATH,
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
