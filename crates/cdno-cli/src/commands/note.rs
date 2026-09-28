//! `cdno note` — create, list and revise notes of config-defined custom
//! types.
//!
//! Built-in types have their own verbs (`cdno project`, `cdno question`, …);
//! a custom type declared under `[note_types.<type>]` has none, so this one
//! generic command serves them all.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use clap::Subcommand;

use cdno_domain::Vault;
use cdno_domain::vault::{ReviseOutcome, Revision};

use crate::bootstrap;
use crate::prompt;

#[derive(Debug, Subcommand)]
pub enum NoteCommands {
    /// Create a note of a config-defined custom type (declared under
    /// `[note_types.<type>]` in `.cuaderno/config.toml`).
    Create {
        /// The custom note type, e.g. `person`.
        note_type: String,
        /// The note's title; its slug becomes the filename.
        #[arg(long)]
        title: String,
        /// A frontmatter field, `name=value`. Repeatable. Each key must be a
        /// declared `required`/`optional` field of the type.
        #[arg(long = "field", value_parser = crate::prompt::parse_key_val)]
        field: Vec<(String, String)>,
        /// Value for a prompted template variable (`[variables.prompt]`),
        /// `name=value`. Repeatable.
        #[arg(long = "var", value_parser = crate::prompt::parse_key_val)]
        var: Vec<(String, String)>,
        /// A file whose contents become the note's body, without the title
        /// heading (the engine writes the H1; a leading `# <title>` line is
        /// dropped). It fills the type template's `{{body}}` placeholder, or
        /// is inserted after the H1 when the template has none. When the
        /// template has `{{body}}` and nothing else (`--field body=`,
        /// `--var body=`, a `[variables] body` in the config) fills it, it is
        /// prompted in an editor in an interactive session and required
        /// otherwise.
        #[arg(long = "body-file", value_name = "PATH")]
        body_file: Option<PathBuf>,
        /// Where the note came from: one string of wikilinks, e.g.
        /// `[[journal/2026/daily/2026-09-02#Woodbury identity]]` (promotion,
        /// RFC 0002). The type must declare an `origin` field.
        #[arg(long, value_name = "STRING")]
        origin: Option<String>,
    },

    /// List every note of a config-defined custom type, by path.
    List {
        /// The custom note type.
        note_type: String,
    },

    /// Revise a mutable custom note (such as a concept) in place, logging
    /// `revised [[path]] — reason` to today's daily note. Built-in types and
    /// custom types declared `append_only = true` are refused.
    ///
    /// Give either `--body-file` (the whole body after the frontmatter) or
    /// `--section` with `--content-file` (upsert one section). With neither,
    /// an interactive session opens the current body in an editor.
    Revise {
        /// The note to revise, resolved as `cdno open` resolves a reference:
        /// a path with or without `.md`, a slug, or `type:slug`. Omitted in
        /// an interactive session, a picker offers every note.
        note: Option<String>,
        /// A file whose contents replace the note's whole body (everything
        /// after the frontmatter, which is kept as it is), written verbatim.
        #[arg(long = "body-file", value_name = "PATH", conflicts_with = "section")]
        body_file: Option<PathBuf>,
        /// The heading text of the section to upsert, without the `#`
        /// markers. An existing section is replaced together with its
        /// sub-sections; a missing one is appended as `## <section>`.
        #[arg(long, value_name = "STRING", requires = "content_file")]
        section: Option<String>,
        /// A file holding the section's new text, without its heading.
        #[arg(long = "content-file", value_name = "PATH", requires = "section")]
        content_file: Option<PathBuf>,
        /// Why the note was revised, in a short clause; it becomes the
        /// daily-log line `revised [[path]] — reason`.
        #[arg(long, value_name = "STRING")]
        reason: Option<String>,
    },
}

pub fn run(
    root: &Path,
    at: NaiveDateTime,
    command: NoteCommands,
    no_interactive: bool,
    json: bool,
) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    // `--json` implies non-interactive: prompts/confirms print to stdout,
    // which would corrupt the JSON result.
    let interactive = prompt::reports_interactively(no_interactive, json);
    match command {
        NoteCommands::Create {
            note_type,
            title,
            field,
            var,
            body_file,
            origin,
        } => {
            let fields: HashMap<String, String> = field.into_iter().collect();
            let vars: HashMap<String, String> = var.into_iter().collect();
            let body_from_file = body_file
                .map(|p| {
                    std::fs::read_to_string(&p)
                        .with_context(|| format!("reading --body-file {}", p.display()))
                })
                .transpose()?;
            let mut prompted = false;
            // A body is gathered (prompted or demanded) only when the type's
            // template has a `{{body}}` slot that nothing else (a `body`
            // field, a `[variables] body`, a `--var body=`) fills; otherwise
            // it is optional.
            let body = if vault.custom_note_needs_body(&note_type, &fields, &vars)? {
                Some(prompt::gather_or_error(
                    body_from_file,
                    "body-file",
                    interactive,
                    &mut prompted,
                    || prompt::prompt_editor("Body", ""),
                )?)
            } else {
                body_from_file
            };
            if prompted
                && !prompt::confirm_preview(&format!(
                    "About to create {note_type} note:\n  title: {title}"
                ))?
            {
                println!("Aborted.");
                return Ok(());
            }
            let path = vault.create_custom_note_with_vars(
                at,
                &note_type,
                &title,
                &fields,
                &vars,
                body.as_deref(),
                origin.as_deref(),
            )?;
            crate::output::emit_write_result(json, &path.to_string(), &format!("Created {path}"))
        }
        NoteCommands::Revise {
            note,
            body_file,
            section,
            content_file,
            reason,
        } => revise(
            root,
            &vault,
            at,
            ReviseArgs {
                note,
                body_file,
                section,
                content_file,
                reason,
            },
            interactive,
            json,
        ),
        NoteCommands::List { note_type } => {
            let paths = vault.list_custom_notes(&note_type)?;
            if json {
                let rows: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else if paths.is_empty() {
                println!("No `{note_type}` notes.");
            } else {
                for p in &paths {
                    println!("{p}");
                }
            }
            Ok(())
        }
    }
}

/// The flags of `cdno note revise`, bundled so [`revise`] stays readable.
struct ReviseArgs {
    note: Option<String>,
    body_file: Option<PathBuf>,
    section: Option<String>,
    content_file: Option<PathBuf>,
    reason: Option<String>,
}

/// `cdno note revise`: resolve, read, gather, confirm, revise.
///
/// The note is read first and its `content_hash` kept; that hash goes to
/// [`Vault::revise_note`] as `expected_hash`, so an edit that lands while
/// the editor is open (no lock is held meanwhile) is refused at commit
/// with the domain's `StaleRevision` message. The body or section file is
/// read only after the note, so what it holds is always measured against
/// the read the hash came from.
fn revise(
    root: &Path,
    vault: &Vault,
    at: NaiveDateTime,
    args: ReviseArgs,
    interactive: bool,
    json: bool,
) -> Result<()> {
    let mut prompted = false;
    let path = match args.note {
        Some(reference) => {
            let reference = crate::commands::open::strip_vault_root(&reference, root);
            crate::commands::open::resolve(vault, &reference, at.date(), interactive)?
        }
        None if interactive => {
            prompted = true;
            crate::commands::open::pick_from_all(vault, None)?
        }
        None => return Err(prompt::missing_positional("note")),
    };
    let view = vault.read_note(&path)?;

    let revision = match (args.section, args.content_file) {
        (Some(heading), Some(content_file)) => Revision::Section {
            // Headings are matched as trimmed text, so a padded heading
            // would miss the real section and be appended as a duplicate.
            heading: heading.trim().to_owned(),
            content: read_flag_file(&content_file, "content-file")?,
        },
        // clap's `requires` makes a half-given section pair unreachable.
        _ => {
            let body_from_file = args
                .body_file
                .map(|p| read_flag_file(&p, "body-file"))
                .transpose()?;
            Revision::Body(prompt::gather_or_error(
                body_from_file,
                "body-file",
                interactive,
                &mut prompted,
                || prompt::prompt_editor("Body", &view.body),
            )?)
        }
    };
    let reason =
        prompt::gather_or_error(args.reason, "reason", interactive, &mut prompted, || {
            prompt::prompt_text("Reason")
        })?;

    if prompted {
        let target = match &revision {
            Revision::Section { heading, .. } => format!("{path} (section `{heading}`)"),
            Revision::Body(_) => path.to_string(),
        };
        if !prompt::confirm_preview(&format!("About to revise {target}:\n  reason: {reason}"))? {
            println!("Aborted.");
            return Ok(());
        }
    }

    // The hash goes with a section revision too: the domain accepts it, and
    // refusing an edit that raced the read is the same promise either way.
    let outcome = vault.revise_note(
        &path,
        Some(view.content_hash.as_str()),
        revision,
        &reason,
        at,
    )?;
    emit_revise_result(json, &outcome)
}

/// Read a `--body-file` / `--content-file`, naming the flag on failure.
fn read_flag_file(path: &Path, flag: &str) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading --{flag} {}", path.display()))
}

/// Print a revision's result: `Revised <path>` or `No change to <path>`,
/// or with `--json` the `emit_write_result` shape (`path`, `message`)
/// extended with the [`ReviseOutcome`] fields.
fn emit_revise_result(json: bool, outcome: &ReviseOutcome) -> Result<()> {
    let message = if outcome.changed {
        format!("Revised {}", outcome.path)
    } else {
        format!("No change to {}", outcome.path)
    };
    if json {
        let payload = serde_json::json!({
            "path": outcome.path.to_string(),
            "message": message,
            "changed": outcome.changed,
            "new_hash": outcome.new_hash,
            "log_line": outcome.log_line,
            "section_target": outcome.section_target,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{message}");
    }
    Ok(())
}
