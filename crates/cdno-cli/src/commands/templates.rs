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
        /// Template variant (e.g. `gym` for `tracking`) — selects the
        /// variant's built-in template when one exists.
        #[arg(long)]
        variant: Option<String>,
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
    }
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
    if NoteType::from_str(note_type).is_err() {
        let valid = NoteType::ALL
            .iter()
            .map(|t| t.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("unknown note type '{note_type}' — valid types: {valid}");
    }

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
