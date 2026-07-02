//! `cdno note` — create and list notes of config-defined custom types.
//!
//! Built-in types have their own verbs (`cdno project`, `cdno question`, …);
//! a custom type declared under `[note_types.<type>]` has none, so this one
//! generic command serves them all.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use chrono::NaiveDateTime;
use clap::Subcommand;

use crate::bootstrap;

#[derive(Debug, Subcommand)]
pub enum NoteCommands {
    /// Create a note of a config-defined custom type (declared under
    /// `[note_types.<type>]` in `.cuaderno/config.toml`).
    New {
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
    },

    /// List every note of a config-defined custom type, by path.
    List {
        /// The custom note type.
        note_type: String,
    },
}

pub fn run(root: &Path, at: NaiveDateTime, command: NoteCommands, json: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    match command {
        NoteCommands::New {
            note_type,
            title,
            field,
            var,
        } => {
            let fields: HashMap<String, String> = field.into_iter().collect();
            let vars: HashMap<String, String> = var.into_iter().collect();
            let path =
                vault.create_custom_note_with_vars(at, &note_type, &title, &fields, &vars)?;
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
