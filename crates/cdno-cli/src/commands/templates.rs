//! `cdno templates` subcommands: template introspection.
//!
//! Custom templates in `.cuaderno/templates/` can only use the
//! `{{placeholders}}` a note type's create path actually supplies (unknown
//! ones render verbatim). `templates vars <type>` surfaces that set from the
//! CLI so you don't have to read the source or the user guide to know what a
//! custom template may reference (#271).

use std::path::Path;
use std::str::FromStr;

use anyhow::{Result, bail};
use clap::Subcommand;
use clap_complete::engine::ArgValueCompleter;

use cdno_domain::note_type::NoteType;
use cdno_domain::{PlaceholderSource, TemplatePlaceholder};

use crate::bootstrap;
use crate::completions;

#[derive(Debug, Subcommand)]
pub enum TemplatesCommands {
    /// List the `{{placeholders}}` a note type's template supports —
    /// the keys its create path fills, plus any config `[variables]` /
    /// `[variables.prompt]` names available to every template.
    Vars {
        /// Note type: `project`, `action`, `question`, `portfolio`,
        /// `evidence`, `stewardship`, `tracking`, `commitment`, `daily`,
        /// `weekly`, or `inbox`.
        #[arg(add = ArgValueCompleter::new(completions::complete_note_type))]
        note_type: String,
        /// Template variant — selects a `<type>-<variant>` built-in when
        /// one exists (none ship today), else falls back to the base type.
        #[arg(long)]
        variant: Option<String>,
    },

    /// Copy a built-in template into `.cuaderno/templates/<type>.md` as an
    /// editable starting point for customisation. Refuses to overwrite an
    /// existing custom template unless `--force`.
    Eject {
        /// Note type to eject (same set as `templates vars`).
        #[arg(add = ArgValueCompleter::new(completions::complete_note_type))]
        note_type: String,
        /// Template variant. The variant must have its own built-in — there's
        /// no fallback to the base type. (No variant templates ship today, so
        /// this currently always errors; kept for future built-in variants.)
        #[arg(long)]
        variant: Option<String>,
        /// Overwrite an existing custom template.
        #[arg(long)]
        force: bool,
    },
}

pub fn run(root: &Path, command: TemplatesCommands, json: bool) -> Result<()> {
    match command {
        TemplatesCommands::Vars { note_type, variant } => {
            let placeholders = placeholders(root, &note_type, variant.as_deref())?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json_rows(&placeholders))?
                );
            } else {
                println!("{}", render_table(&placeholders));
            }
            Ok(())
        }
        TemplatesCommands::Eject {
            note_type,
            variant,
            force,
        } => {
            let path = eject(root, &note_type, variant.as_deref(), force)?;
            crate::output::emit_write_result(json, &path, &format!("Ejected template to {path}"))
        }
    }
}

/// Validate `note_type` against the known set, returning a friendly error
/// listing the valid types (richer than the domain's terser variant).
fn validate_note_type(note_type: &str) -> Result<()> {
    if NoteType::from_str(note_type).is_err() {
        let valid = NoteType::ALL
            .iter()
            .map(|t| t.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("unknown note type '{note_type}' — valid types: {valid}");
    }
    Ok(())
}

/// Write seam: eject a built-in template, returning the written path. Its own
/// function so tests assert on the path/side effect (house pattern).
pub fn eject(root: &Path, note_type: &str, variant: Option<&str>, force: bool) -> Result<String> {
    validate_note_type(note_type)?;
    let (vault, _report) = bootstrap::open_vault(root)?;
    let path = vault.eject_template(note_type, variant, force)?;
    Ok(path.to_string())
}

/// Data seam: validate the type, open the vault, and gather the supported
/// placeholders. Tests assert on this `Vec` directly (house pattern, cf.
/// `search::search_hits`).
pub fn placeholders(
    root: &Path,
    note_type: &str,
    variant: Option<&str>,
) -> Result<Vec<TemplatePlaceholder>> {
    // Validate the type here so the user gets the full valid set, rather
    // than the domain's terser `unknown note type` error.
    validate_note_type(note_type)?;
    let (vault, _report) = bootstrap::open_vault(root)?;
    Ok(vault.template_placeholders(note_type, variant)?)
}

/// Text seam: render the placeholder table.
pub fn render_table(placeholders: &[TemplatePlaceholder]) -> String {
    if placeholders.is_empty() {
        return "No template placeholders.".to_owned();
    }
    let mut table = crate::output::styled_table();
    table.set_header(["Placeholder", "Source", "Note"]);
    for p in placeholders {
        let (source, note) = source_columns(&p.source);
        table.add_row([format!("{{{{{}}}}}", p.name), source.to_owned(), note]);
    }
    crate::output::no_wrap_columns(&mut table, &[0, 1]);
    crate::output::render(&table)
}

/// The `--json` rows for a placeholder set: `{ name, source }`, with
/// `message` on prompt entries.
pub fn json_rows(placeholders: &[TemplatePlaceholder]) -> Vec<serde_json::Value> {
    placeholders
        .iter()
        .map(|p| match &p.source {
            PlaceholderSource::Supplied => {
                serde_json::json!({ "name": p.name, "source": "supplied" })
            }
            PlaceholderSource::Config => serde_json::json!({ "name": p.name, "source": "config" }),
            PlaceholderSource::Prompt { message } => {
                serde_json::json!({ "name": p.name, "source": "prompt", "message": message })
            }
        })
        .collect()
}

/// `(source-label, note)` columns for the human table.
fn source_columns(source: &PlaceholderSource) -> (&'static str, String) {
    match source {
        PlaceholderSource::Supplied => ("supplied", "filled automatically on create".to_owned()),
        PlaceholderSource::Config => ("config", "from [variables]".to_owned()),
        PlaceholderSource::Prompt { message } => ("prompt", message.clone()),
    }
}
