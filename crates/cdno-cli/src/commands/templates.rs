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

use cdno_domain::note_type::NoteType;
use cdno_domain::{PlaceholderSource, TemplatePlaceholder};

use crate::bootstrap;

#[derive(Debug, Subcommand)]
pub enum TemplatesCommands {
    /// List the `{{placeholders}}` a note type's template supports —
    /// the keys its create path fills, plus any config `[variables]` /
    /// `[variables.prompt]` names available to every template.
    Vars {
        /// Note type: `project`, `action`, `question`, `portfolio`,
        /// `evidence`, `stewardship`, `tracking`, `commitment`, `daily`,
        /// `weekly`, or `inbox`.
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
            let out = build_vars(root, &note_type, variant.as_deref(), json)?;
            println!("{out}");
            Ok(())
        }
    }
}

/// Build the rendered output of `templates vars` (table, or JSON with
/// `--json`). A seam so tests assert on the text rather than capturing
/// stdout — the house pattern (cf. `search::build_search`).
pub fn build_vars(
    root: &Path,
    note_type: &str,
    variant: Option<&str>,
    json: bool,
) -> Result<String> {
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
    let placeholders = vault.template_placeholders(note_type, variant)?;

    if json {
        let rows: Vec<serde_json::Value> = placeholders.iter().map(placeholder_json).collect();
        return Ok(serde_json::to_string_pretty(&rows)?);
    }

    if placeholders.is_empty() {
        return Ok(format!("{note_type} has no template placeholders."));
    }

    let mut table = crate::output::styled_table();
    table.set_header(["Placeholder", "Source", "Note"]);
    for p in &placeholders {
        let (source, note) = source_columns(&p.source);
        table.add_row([format!("{{{{{}}}}}", p.name), source.to_owned(), note]);
    }
    crate::output::no_wrap_columns(&mut table, &[0, 1]);
    Ok(crate::output::render(&table))
}

/// `(source-label, note)` columns for the human table.
fn source_columns(source: &PlaceholderSource) -> (&'static str, String) {
    match source {
        PlaceholderSource::Supplied => ("supplied", "filled automatically on create".to_owned()),
        PlaceholderSource::Config => ("config", "from [variables]".to_owned()),
        PlaceholderSource::Prompt { message } => ("prompt", message.clone()),
    }
}

fn placeholder_json(p: &TemplatePlaceholder) -> serde_json::Value {
    match &p.source {
        PlaceholderSource::Supplied => serde_json::json!({ "name": p.name, "source": "supplied" }),
        PlaceholderSource::Config => serde_json::json!({ "name": p.name, "source": "config" }),
        PlaceholderSource::Prompt { message } => {
            serde_json::json!({ "name": p.name, "source": "prompt", "message": message })
        }
    }
}
