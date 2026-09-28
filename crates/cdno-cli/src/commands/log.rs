use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{NaiveDate, NaiveDateTime};
use clap::Subcommand;

use cdno_domain::vault::NoteToDailyOutcome;

use crate::bootstrap;
use crate::prompt;

/// Append a log entry to the daily note for `at`. Creates the note
/// with a minimal scaffold if it doesn't exist.
pub fn run(root: &Path, at: NaiveDateTime, message: &str, json: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let path = vault
        .log_to_daily_note(at, message)
        .context("appending log entry to daily note")?;
    crate::output::emit_write_result(json, &path.to_string(), &format!("Logged to {path}"))?;
    Ok(())
}

/// The subcommands of `cdno log`. They coexist with the positional
/// message (`cdno log "text"`): a first argument naming a subcommand
/// selects it, and anything else is the message.
#[derive(Debug, Subcommand)]
pub enum LogCommands {
    /// Write worked-out substance (a derivation, a procedure, a page of
    /// reasoning) to a daily note as one `### <heading>` entry under
    /// `## Notes`; the pointer line `noted [[journal/…#<heading>]]` is
    /// written to `## Logs` for you, in the same write.
    ///
    /// Keep `## Logs` for one-line events (`cdno log <message>`) and put
    /// the substance here. The heading must be unique within the day, must
    /// not reuse a daily section name, and must not contain `[`, `]`, `|`,
    /// `#` or inline markup, nor start with `^`; headings inside the body
    /// must be level 3 or deeper. End an entry that could be reused beyond
    /// today with the tag `#concept` on the body's last line, so the review
    /// can find it as a candidate for promotion to a concept note.
    ///
    /// A missing `--heading` or `--body-file` is prompted in an interactive
    /// session (the body in an editor) and is an error otherwise.
    Note {
        /// The entry's heading, written as `### <heading>` under `## Notes`
        /// and used as the pointer's anchor.
        #[arg(long, value_name = "STRING")]
        heading: Option<String>,
        /// A file holding the entry's body. Leading blank lines and
        /// trailing whitespace are dropped; the first line's indentation
        /// is kept. A blank file is refused.
        #[arg(long = "body-file", value_name = "PATH")]
        body_file: Option<PathBuf>,
        /// The day whose note receives the entry, stamped at the current
        /// time. Defaults to today.
        #[arg(long, value_name = "YYYY-MM-DD")]
        date: Option<NaiveDate>,
    },
}

/// Dispatch a `cdno log` subcommand. `now` is the current local moment;
/// a `--date` keeps its time of day, as the `note_to_daily` MCP tool does.
pub fn run_command(
    root: &Path,
    now: NaiveDateTime,
    command: LogCommands,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    match command {
        LogCommands::Note {
            heading,
            body_file,
            date,
        } => {
            let at = date.map_or(now, |d| d.and_time(now.time()));
            note(root, at, heading, body_file, no_interactive, json)
        }
    }
}

/// `cdno log note`: gather the heading and the body, confirm if anything
/// was prompted, then hand both to [`cdno_domain::Vault::note_to_daily`].
/// Every heading rule is the domain's; this layer only refuses a blank
/// body, which the domain would accept as a bare heading.
fn note(
    root: &Path,
    at: NaiveDateTime,
    heading: Option<String>,
    body_file: Option<PathBuf>,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    // `--json` implies non-interactive: prompts/confirms print to stdout,
    // which would corrupt the JSON result.
    let interactive = prompt::reports_interactively(no_interactive, json);
    // Read the file up front so an unreadable path fails before any prompt.
    let body_from_file = body_file
        .map(|p| {
            std::fs::read_to_string(&p)
                .with_context(|| format!("reading --body-file {}", p.display()))
        })
        .transpose()?;

    let mut prompted = false;
    let heading = prompt::gather_or_error(heading, "heading", interactive, &mut prompted, || {
        prompt::prompt_text("Heading")
    })?;
    let body = prompt::gather_or_error(
        body_from_file,
        "body-file",
        interactive,
        &mut prompted,
        || prompt::prompt_editor("Body", ""),
    )?;
    // Only leading blank lines and trailing whitespace are stripped: the
    // first line's indentation is part of the substance (an entry may open
    // with an indented code block), and the domain writes the body verbatim.
    let body = body.trim_start_matches(['\n', '\r']).trim_end().to_owned();
    if body.trim().is_empty() {
        bail!(
            "--body-file is blank: the entry needs its substance; a one-line event belongs in \
             `cdno log <message>`"
        );
    }

    if prompted
        && !prompt::confirm_preview(&format!(
            "About to add a note to the {} daily note:\n  heading: {}",
            at.date(),
            heading.trim()
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    let outcome = vault.note_to_daily(at, &heading, &body)?;
    emit_note_result(json, &outcome)
}

/// Print `Noted <target>`, or with `--json` the `emit_write_result` shape
/// (`path`, `message`) extended with `target` and `log_line`.
fn emit_note_result(json: bool, outcome: &NoteToDailyOutcome) -> Result<()> {
    let message = format!("Noted {}", outcome.target);
    if json {
        let payload = serde_json::json!({
            "path": outcome.path.to_string(),
            "message": message,
            "target": outcome.target,
            "log_line": outcome.log_line,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{message}");
    }
    Ok(())
}
