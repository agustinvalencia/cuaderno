//! `cdno note` — create and list notes of config-defined custom types.
//!
//! Built-in types have their own verbs (`cdno project`, `cdno question`, …);
//! a custom type declared under `[note_types.<type>]` has none, so this one
//! generic command serves them all.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use clap::Subcommand;

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
        /// A file whose contents become the note's body: it fills the type
        /// template's `{{body}}` placeholder, or is inserted after the H1
        /// when the template has none. Prompted in an editor when the
        /// template has `{{body}}` and the session is interactive; required
        /// then when it is not.
        #[arg(long = "body-file")]
        body_file: Option<PathBuf>,
        /// Where the note came from: one string of wikilinks, e.g.
        /// `[[journal/2026/daily/2026-09-02#Woodbury identity]]` (promotion,
        /// RFC 0002). The type must declare an `origin` field.
        #[arg(long)]
        origin: Option<String>,
    },

    /// List every note of a config-defined custom type, by path.
    List {
        /// The custom note type.
        note_type: String,
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
            // template has a `{{body}}` slot for it; otherwise it is optional.
            let body = if vault.custom_template_has_body(&note_type)? {
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
