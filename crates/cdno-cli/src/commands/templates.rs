//! `cdno templates` subcommands: template introspection.
//!
//! Custom templates in `.cuaderno/templates/` can only use the
//! `{{placeholders}}` a note type's create path actually supplies (unknown
//! ones render verbatim). `templates vars <type>` surfaces that set from the
//! CLI so you don't have to read the source or the user guide to know what a
//! custom template may reference (#271).

use std::path::Path;

use anyhow::{Result, bail};
use clap::Subcommand;
use clap_complete::engine::ArgValueCompleter;

use cdno_domain::note_type::NoteType;
use cdno_domain::{PlaceholderSource, TemplatePlaceholder, Vault};

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
    },

    /// Copy a built-in template into `.cuaderno/templates/<type>.md` as an
    /// editable starting point for customisation. Refuses to overwrite an
    /// existing custom template unless `--force`. Pass `--all` to eject every
    /// built-in template at once (skipping ones you've already customised).
    Eject {
        /// Note type to eject (same set as `templates vars`). Omit with `--all`.
        #[arg(
            add = ArgValueCompleter::new(completions::complete_note_type),
            required_unless_present = "all",
            conflicts_with = "all",
        )]
        note_type: Option<String>,
        /// Eject every built-in template into `.cuaderno/templates/`.
        #[arg(long)]
        all: bool,
        /// Overwrite existing custom templates.
        #[arg(long)]
        force: bool,
    },
}

pub fn run(root: &Path, command: TemplatesCommands, json: bool) -> Result<()> {
    match command {
        TemplatesCommands::Vars { note_type } => {
            let placeholders = placeholders(root, &note_type)?;
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
            all,
            force,
        } => {
            if all {
                let report = eject_all(root, force)?;
                emit_eject_all(json, &report)
            } else {
                // `required_unless_present = "all"` guarantees Some here.
                let note_type = note_type.expect("clap requires <type> without --all");
                let path = eject(root, &note_type, force)?;
                crate::output::emit_write_result(
                    json,
                    &path,
                    &format!("Ejected template to {path}"),
                )
            }
        }
    }
}

/// What `templates eject --all` did: the note types written and the ones
/// skipped because a custom template already exists.
pub struct EjectAllReport {
    pub written: Vec<String>,
    pub skipped: Vec<String>,
}

/// Write seam for `--all`: eject every built-in template, skipping types that
/// already have a custom template (unless `force`). Opens the vault once and
/// reuses the per-type `Vault::eject_template`.
pub fn eject_all(root: &Path, force: bool) -> Result<EjectAllReport> {
    use cdno_domain::error::DomainError;
    let (vault, _report) = bootstrap::open_vault(root)?;
    let mut written = Vec::new();
    let mut skipped = Vec::new();
    for note_type in NoteType::ALL {
        match vault.eject_template(note_type.as_str(), None, force) {
            Ok(_path) => written.push(note_type.as_str().to_owned()),
            Err(DomainError::TemplateAlreadyExists { .. }) => {
                skipped.push(note_type.as_str().to_owned())
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(EjectAllReport { written, skipped })
}

/// Render the `--all` result: `{written, skipped}` under `--json`, else a human
/// summary naming what was written and what was skipped.
fn emit_eject_all(json: bool, report: &EjectAllReport) -> Result<()> {
    if json {
        let payload = serde_json::json!({
            "written": report.written,
            "skipped": report.skipped,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }
    if report.written.is_empty() {
        println!(
            "All {} templates already exist in .cuaderno/templates/ — use --force to overwrite.",
            report.skipped.len()
        );
        return Ok(());
    }
    println!(
        "Ejected {} template(s) to .cuaderno/templates/: {}",
        report.written.len(),
        report.written.join(", ")
    );
    if !report.skipped.is_empty() {
        println!(
            "Skipped {} already present: {} — use --force to overwrite.",
            report.skipped.len(),
            report.skipped.join(", ")
        );
    }
    Ok(())
}

/// Validate `note_type` against the vault's known set (built-ins + config
/// custom types), returning a friendly error listing the valid names (richer
/// than the domain's terser variant).
fn validate_known_type(vault: &Vault, note_type: &str) -> Result<()> {
    let registry = vault.type_registry();
    if !registry.is_known(note_type) {
        let valid = registry.all_names().join(", ");
        bail!("unknown note type '{note_type}' — valid types: {valid}");
    }
    Ok(())
}

/// Write seam: eject a built-in template, returning the written path. Its own
/// function so tests assert on the path/side effect (house pattern).
///
/// Only base note-type templates are ejectable: no `<type>-<variant>` template
/// ships built-in, so there is nothing to eject for a variant (a `tracking`
/// variant is authored in the vault, not ejected). The domain
/// `eject_template` still takes a `variant`; the CLI always passes `None`.
pub fn eject(root: &Path, note_type: &str, force: bool) -> Result<String> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    validate_known_type(&vault, note_type)?;
    // A config-defined custom type has no built-in template to materialise —
    // its template is authored by hand.
    if vault
        .type_registry()
        .resolve(note_type)
        .is_some_and(|d| d.is_custom())
    {
        bail!(
            "`{note_type}` is a config-defined custom type — it has no built-in template to \
             eject; author `.cuaderno/templates/{note_type}.md` by hand"
        );
    }
    let path = vault.eject_template(note_type, None, force)?;
    Ok(path.to_string())
}

/// Data seam: validate the type, open the vault, and gather the supported
/// placeholders. Tests assert on this `Vec` directly (house pattern, cf.
/// `search::search_hits`).
pub fn placeholders(root: &Path, note_type: &str) -> Result<Vec<TemplatePlaceholder>> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    // Validate here so the user gets the full valid set (built-ins + custom
    // types) rather than the domain's terser `unknown note type` error.
    validate_known_type(&vault, note_type)?;
    Ok(vault.template_placeholders(note_type)?)
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
            PlaceholderSource::Schema => serde_json::json!({ "name": p.name, "source": "schema" }),
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
        PlaceholderSource::Schema => (
            "schema",
            "declared field, filled from frontmatter".to_owned(),
        ),
        PlaceholderSource::Config => ("config", "from [variables]".to_owned()),
        PlaceholderSource::Prompt { message } => ("prompt", message.clone()),
    }
}
