//! `cdno templates` subcommands: template introspection and editing.
//!
//! Custom templates in `.cuaderno/templates/` can only use the
//! `{{placeholders}}` a note type's create path actually supplies (unknown
//! ones render verbatim). `templates vars <type>` surfaces that set from the
//! CLI so you don't have to read the source or the user guide to know what a
//! custom template may reference (#271).
//!
//! `list` / `show` / `save` / `new` complete a story that was split across
//! two applications (#597, #599). Ejecting a template to customise it worked
//! from the CLI; reading one back, listing them, or saving one did not —
//! those lived only in the desktop app's Templates view, which is being
//! retired. This is not net-new surface so much as the other half of what
//! `eject` already started.
//!
//! ## `save` writes where `eject` would have
//!
//! `Vault::save_template` transparently CREATES the custom override for a
//! built-in type — the desktop's direct edit-and-save model, with no
//! separate eject step. So `templates save --note-type project` and
//! `templates eject project` write the same file, and `show` reads back
//! exactly what either wrote. `new` is the genuinely different one: it
//! scaffolds a starter for a config-defined custom type, which has no
//! built-in to copy and which `eject` explicitly refuses.
//!
//! ## Templates are config, not notes
//!
//! `save_template` is a plain confined `store.write_file` — no
//! `VaultTransaction`, and so no cross-process write lock (CLAUDE.md lists
//! it among the documented raw-write paths). The path is still confined to
//! `.cuaderno/templates/` by `VaultPath`, so a traversal cannot escape the
//! vault; what is absent is the serialisation, not the confinement.

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

    /// List every note type and the state of its template: whether a
    /// custom override exists, which source is in effect, and the path
    /// the override lives (or would live) at.
    List,

    /// Print a template's effective content verbatim — the custom
    /// override when one exists, else the built-in default. A custom type
    /// with no file yet shows the starter `new` would write.
    Show {
        /// Note type to show.
        #[arg(add = ArgValueCompleter::new(completions::complete_note_type))]
        note_type: String,
        /// Show a `<type>-<variant>` template instead of the base one.
        #[arg(long, value_name = "NAME")]
        variant: Option<String>,
    },

    /// Write a template. On a built-in type this creates the custom
    /// override transparently, so it needs no prior `eject`.
    Save {
        /// Note type whose template to write.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_note_type))]
        note_type: Option<String>,
        /// Write the `<type>-<variant>` template instead of the base one.
        #[arg(long, value_name = "NAME")]
        variant: Option<String>,
        /// Read the new content from this file, or from stdin when it is
        /// `-`. Without it, an interactive run opens your editor seeded
        /// with the current template.
        #[arg(long, value_name = "PATH")]
        file: Option<String>,
    },

    /// Scaffold a starter template for a config-defined custom type that
    /// has none yet. Built-in types have a default to edit instead, so
    /// this refuses them and points at `save`.
    New {
        /// Config-defined custom note type to scaffold.
        #[arg(long, add = ArgValueCompleter::new(completions::complete_note_type))]
        note_type: Option<String>,
    },
}

pub fn run(
    root: &Path,
    command: TemplatesCommands,
    json: bool,
    no_interactive: bool,
) -> Result<()> {
    let interactive = crate::prompt::reports_interactively(no_interactive, json);
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
        TemplatesCommands::List => {
            let rows = summaries(root)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&list_rows(&rows))?);
            } else {
                println!("{}", render_list(&rows));
            }
            Ok(())
        }
        TemplatesCommands::Show { note_type, variant } => {
            let content = template_content(root, &note_type, variant.as_deref())?;
            // Verbatim, and via `print!`: `templates show <t>` has to diff
            // clean against the file `eject`/`save` wrote, so an added
            // trailing newline would be a bug rather than a nicety.
            print!("{}", content.content);
            Ok(())
        }
        TemplatesCommands::Save {
            note_type,
            variant,
            file,
        } => save(root, note_type, variant, file, json, interactive),
        TemplatesCommands::New { note_type } => new(root, note_type, json, interactive),
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

/// Refuse `--variant` on a config-defined custom type.
///
/// `Vault::save_template`'s custom-type branch resolves the filename from
/// the type's configured `template` and never consults `variant`, so a
/// variant save did not write `people-meeting.md` — it overwrote
/// `people.md`, the type's ONLY template, and reported success. Silent
/// data loss. `read_template` has the matching blind spot: it returns the
/// base content for any variant, which is also what seeds the editor in
/// the interactive `save` path, so the overwrite looked like an edit of
/// the right file.
///
/// Variants are a built-in-type feature (`<type>-<variant>.md`). A custom
/// type has one template, so asking for a variant of one is a mistake
/// worth naming rather than a request to be quietly reinterpreted.
fn reject_variant_on_custom(vault: &Vault, note_type: &str, variant: Option<&str>) -> Result<()> {
    let Some(variant) = variant else {
        return Ok(());
    };
    if vault
        .type_registry()
        .resolve(note_type)
        .is_some_and(|d| d.is_custom())
    {
        bail!(
            "`{note_type}` is a config-defined custom type, which has a single template — \
             there is no `{note_type}-{variant}` to read or write. Drop `--variant`, or \
             declare a separate note type for it in `.cuaderno/config.toml`."
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

/// Data seam: every note type and the state of its template.
pub fn summaries(root: &Path) -> Result<Vec<cdno_domain::TemplateSummary>> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    Ok(vault.list_templates()?)
}

/// Data seam: a template's effective content plus its source rung.
pub fn template_content(
    root: &Path,
    note_type: &str,
    variant: Option<&str>,
) -> Result<cdno_domain::TemplateContent> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    validate_known_type(&vault, note_type)?;
    reject_variant_on_custom(&vault, note_type, variant)?;
    Ok(vault.read_template(note_type, variant)?)
}

/// Write seam: save a template, returning the written path.
pub fn save_content(
    root: &Path,
    note_type: &str,
    variant: Option<&str>,
    content: &str,
) -> Result<String> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    validate_known_type(&vault, note_type)?;
    reject_variant_on_custom(&vault, note_type, variant)?;
    Ok(vault
        .save_template(note_type, variant, content)?
        .to_string())
}

/// Write seam: scaffold a starter template for a custom type.
///
/// `BuiltinTypeNotCustom` is rephrased rather than surfaced: the domain's
/// own wording for it is about creating a NOTE of a built-in type ("use
/// `cdno project create`"), which is sound advice for `create_note` and
/// misleading here — someone running `templates new project` wants a
/// template, and the answer is `eject` or `save`, not a note.
pub fn create(root: &Path, note_type: &str) -> Result<String> {
    use cdno_domain::error::DomainError;
    let (vault, _report) = bootstrap::open_vault(root)?;
    validate_known_type(&vault, note_type)?;
    match vault.create_template(note_type) {
        Ok(path) => Ok(path.to_string()),
        Err(DomainError::BuiltinTypeNotCustom { .. }) => bail!(
            "`{note_type}` is a built-in type, so it already has a template to start \
             from — there is nothing to scaffold. Use `cdno templates eject {note_type}` \
             to copy the built-in default, or `cdno templates save --note-type \
             {note_type}` to write one directly. `templates new` is for a \
             config-defined custom type, which has no built-in to copy."
        ),
        Err(other) => Err(other.into()),
    }
}

/// The source rung as a stable wire token.
///
/// Separate from the human label because `--json` is read by scripts:
/// `source_label`'s "none (run `templates new`)" would force a caller to
/// match prose, and would break the moment that prose is reworded.
/// `TemplateSourceKind` is a closed enum, so it rides the wire as a token
/// and the label stays free to change.
fn source_token(source: Option<cdno_domain::TemplateSourceKind>) -> &'static str {
    use cdno_domain::TemplateSourceKind::*;
    match source {
        Some(CustomVariant) => "custom_variant",
        Some(CustomBase) => "custom_base",
        Some(BuiltinVariant) => "builtin_variant",
        Some(BuiltinDefault) => "builtin_default",
        None => "none",
    }
}

/// The source rung as a column value.
fn source_label(source: Option<cdno_domain::TemplateSourceKind>) -> &'static str {
    use cdno_domain::TemplateSourceKind::*;
    match source {
        Some(CustomVariant) => "custom variant",
        Some(CustomBase) => "custom",
        Some(BuiltinVariant) => "built-in variant",
        Some(BuiltinDefault) => "built-in",
        // A config custom type whose file does not exist yet: nothing on
        // disk backs it, which is exactly the state `new` is for.
        None => "none (run `templates new`)",
    }
}

/// Text seam: the `list` table.
pub fn render_list(rows: &[cdno_domain::TemplateSummary]) -> String {
    if rows.is_empty() {
        return "No note types.".to_owned();
    }
    let mut table = crate::output::styled_table();
    table.set_header(["Type", "Kind", "Template", "Path"]);
    for row in rows {
        table.add_row([
            row.note_type.clone(),
            if row.is_custom_type {
                "custom".to_owned()
            } else {
                "built-in".to_owned()
            },
            source_label(row.source).to_owned(),
            row.path.clone(),
        ]);
    }
    crate::output::no_wrap_columns(&mut table, &[0, 1, 2]);
    crate::output::render(&table)
}

/// The `--json` rows for `list`.
pub fn list_rows(rows: &[cdno_domain::TemplateSummary]) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|row| {
            serde_json::json!({
                "note_type": row.note_type,
                "display_name": row.display_name,
                "is_custom_type": row.is_custom_type,
                "source": source_token(row.source),
                "has_custom_file": row.has_custom_file,
                "path": row.path,
            })
        })
        .collect()
}

/// Resolve the new content for `save`.
///
/// Three sources, in the order that keeps a script deterministic and an
/// interactive run convenient: an explicit file, stdin when that file is
/// `-`, or — only when there is a human to ask — the editor, seeded with
/// the template as it stands so an edit starts from the current text
/// rather than an empty buffer. Off a terminal the absent flag is an
/// error, never a silent empty template: `save` with no input would
/// otherwise blank the file.
fn gather_content(
    root: &Path,
    note_type: &str,
    variant: Option<&str>,
    file: Option<String>,
    interactive: bool,
    prompted: &mut bool,
) -> Result<String> {
    match file {
        Some(path) if path == "-" => {
            use std::io::Read;
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|e| anyhow::anyhow!("reading the template from stdin: {e}"))?;
            Ok(buffer)
        }
        Some(path) => std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("reading the template from {path}: {e}")),
        None if interactive => {
            *prompted = true;
            let current = template_content(root, note_type, variant)?.content;
            crate::prompt::prompt_editor(&format!("Template for {note_type}"), &current)
        }
        None => Err(crate::prompt::missing_flag("file")),
    }
}

fn save(
    root: &Path,
    note_type: Option<String>,
    variant: Option<String>,
    file: Option<String>,
    json: bool,
    interactive: bool,
) -> Result<()> {
    let mut prompted = false;
    let note_type =
        crate::prompt::gather_or_error(note_type, "note-type", interactive, &mut prompted, || {
            crate::prompt::prompt_text("Note type whose template to write")
        })?;

    let content = gather_content(
        root,
        &note_type,
        variant.as_deref(),
        file,
        interactive,
        &mut prompted,
    )?;

    if prompted
        && !crate::prompt::confirm_preview(&format!(
            "Save the template for '{note_type}' ({} bytes).",
            content.len()
        ))?
    {
        println!("Cancelled — nothing was written.");
        return Ok(());
    }

    let path = save_content(root, &note_type, variant.as_deref(), &content)?;
    crate::output::emit_write_result(json, &path, &format!("Saved template to {path}"))
}

fn new(root: &Path, note_type: Option<String>, json: bool, interactive: bool) -> Result<()> {
    let mut prompted = false;
    let note_type =
        crate::prompt::gather_or_error(note_type, "note-type", interactive, &mut prompted, || {
            crate::prompt::prompt_text("Custom note type to scaffold a template for")
        })?;

    if prompted
        && !crate::prompt::confirm_preview(&format!("Scaffold a template for '{note_type}'."))?
    {
        println!("Cancelled — nothing was written.");
        return Ok(());
    }

    let path = create(root, &note_type)?;
    crate::output::emit_write_result(json, &path, &format!("Created template at {path}"))
}
