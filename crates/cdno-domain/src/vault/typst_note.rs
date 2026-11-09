//! The Typst note writer's shared parts (RFC 0008 §5.7): the metadata block from the engine's
//! fields, the one check a body passes before it is written, and the assembly of the file.
//!
//! Public so the integration suite can reach it. The verbs that write a note (T19, T23, T26) call
//! it; lint (T15) reads the same facts through the core `typst` primitives, not through here.

use std::collections::HashMap;

use cdno_core::config::FieldType;
use cdno_core::error::StoreError;
use cdno_core::path::VaultPath;
use cdno_core::paths::{NoteFormat, note_address};
use cdno_core::typst::{self, TypstError};
use chrono::NaiveDateTime;
use serde_json::Value;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::set_frontmatter::coerce_value;
use crate::error::DomainError;

/// The metadata block for `ordered` fields: the engine's fields first, in their order, then the
/// type's declared fields in the registry's order. Every value is a string unless the caller
/// supplied a JSON array of strings; a boolean or a number is written as its text. A value the
/// literal writer refuses is an `InvalidFieldValue` on the field `(metadata block)`.
pub fn render_metadata_block(
    note_type: &str,
    ordered: &[(String, Value)],
) -> Result<String, DomainError> {
    let fields: Vec<(String, Value)> = ordered
        .iter()
        .map(|(key, value)| (key.clone(), transport(value)))
        .collect();
    typst::write_block(&fields).map_err(|e| DomainError::InvalidFieldValue {
        note_type: note_type.to_string(),
        field: "(metadata block)".to_string(),
        reason: match e {
            TypstError::Unwritable { what } => what,
            other => other.to_string(),
        },
    })
}

/// A string stays, an array stays, a boolean or number becomes its text; null and objects pass
/// through for the writer to refuse.
fn transport(value: &Value) -> Value {
    match value {
        Value::Bool(_) | Value::Number(_) => Value::String(value.to_string()),
        other => other.clone(),
    }
}

/// Why a Typst body is refused at the door. The three rows of §5.10 that the body alone can trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyRejection {
    CarriesBlock,
    DoesNotParse { line: usize, message: String },
    LinkNeedsEscape { line: usize, leaf: String },
}

impl From<BodyRejection> for DomainError {
    fn from(r: BodyRejection) -> Self {
        match r {
            BodyRejection::CarriesBlock => DomainError::BodyCarriesMetadataBlock,
            BodyRejection::DoesNotParse { line, message } => {
                DomainError::BodyDoesNotParse { line, message }
            }
            BodyRejection::LinkNeedsEscape { line, leaf } => {
                DomainError::LinkNeedsEscape { line, leaf }
            }
        }
    }
}

/// The lines [`assemble_typst_note`] puts between the block and the body.
const SEPARATOR: &str = "\n\n";
const SEPARATOR_LINES: usize = 2;

/// The check a body passes before it is written: no `<cdno>` Label node, it parses, and no link
/// has an interior that is not one inline run. The body is parsed as it will sit, after the
/// separating newlines, so something only valid at the start of a file (a `#!` line) is refused;
/// reported lines are still counted in the body.
pub fn check_typst_body(body: &str) -> Result<(), BodyRejection> {
    let sat = format!("{SEPARATOR}{body}");
    let line_in_body = |line: usize| line.saturating_sub(SEPARATOR_LINES).max(1);
    if typst::has_label_node(&sat) {
        return Err(BodyRejection::CarriesBlock);
    }
    typst::parses(&sat).map_err(|f| BodyRejection::DoesNotParse {
        line: line_in_body(f.line),
        message: f.message,
    })?;
    if let Some(fault) = typst::wikilink_spans(&sat).faults.into_iter().next() {
        return Err(BodyRejection::LinkNeedsEscape {
            line: line_in_body(fault.line),
            leaf: fault.leaf,
        });
    }
    Ok(())
}

/// The block, a blank line, then the body as given, with a trailing newline ensured.
pub fn assemble_typst_note(block: &str, body: &str) -> String {
    let mut out = String::with_capacity(block.len() + body.len() + 2);
    out.push_str(block);
    out.push_str(SEPARATOR);
    out.push_str(body);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// The fields the create path computes itself; a required one of these is satisfied without the
/// caller (as in the Markdown path).
const ENGINE_SUPPLIED_FIELDS: &[&str] = &["title", "slug", "created", "date"];

impl Vault {
    /// Create a custom-type note in `format`: the one entry the CLI and MCP surfaces call.
    /// Markdown goes to [`create_custom_note_with_vars`](Self::create_custom_note_with_vars);
    /// Typst refuses `vars` (they belong to Markdown templates) and goes to
    /// [`create_typst_custom_note`](Self::create_typst_custom_note).
    #[allow(clippy::too_many_arguments)]
    pub fn create_custom_note_in_format(
        &self,
        at: NaiveDateTime,
        format: NoteFormat,
        type_name: &str,
        title: &str,
        fields: &HashMap<String, String>,
        vars: &HashMap<String, String>,
        body: Option<&str>,
        origin: Option<&str>,
        slug: Option<&str>,
    ) -> Result<VaultPath, DomainError> {
        match format {
            NoteFormat::Markdown => self.create_custom_note_with_vars(
                at, type_name, title, fields, vars, body, origin, slug,
            ),
            NoteFormat::Typst => {
                if !vars.is_empty() {
                    return Err(DomainError::VarsUnsupportedForTypst);
                }
                self.create_typst_custom_note(at, type_name, title, fields, body, origin, slug)
            }
        }
    }

    /// Create a Typst note of the config-defined custom type `type_name` (RFC 0008 §5.7), at
    /// `<folder>/<slug>.typ`. The checks mirror the Markdown path (declared fields, required
    /// fields, the explicit-or-derived slug) without a template: the file is the metadata block
    /// from `type`, `title`, `created`, `origin` and the declared fields in the registry's order,
    /// a blank line, and `body` as given (no title heading, no scaffold). A declared `int`,
    /// `float` or `bool` field is coerced to its type before the block is written, so the note
    /// passes the type check on re-read.
    ///
    /// Errors as the Markdown path does, plus [`DomainError::FormatUnsupportedForType`] for a
    /// type that does not admit Typst, [`DomainError::StemTakenByOtherFormat`] when an `.md`
    /// holds the stem, and the body rejections of [`check_typst_body`].
    #[allow(clippy::too_many_arguments)]
    pub fn create_typst_custom_note(
        &self,
        at: NaiveDateTime,
        type_name: &str,
        title: &str,
        fields: &HashMap<String, String>,
        body: Option<&str>,
        origin: Option<&str>,
        slug: Option<&str>,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?;

        let title = title.trim();
        if title.is_empty() {
            return Err(DomainError::EmptyField { field: "title" });
        }

        let registry = self.type_registry();
        let Some(descriptor) = registry.resolve(type_name) else {
            return Err(DomainError::UnknownNoteType {
                note_type: type_name.to_owned(),
            });
        };
        let Some(def) = descriptor.as_custom() else {
            return Err(DomainError::BuiltinTypeNotCustom {
                note_type: type_name.to_owned(),
            });
        };
        if !registry.admits_typst(type_name) {
            return Err(DomainError::FormatUnsupportedForType {
                note_type: type_name.to_owned(),
            });
        }

        if slug.is_some() && fields.get("slug").is_some_and(|v| !v.trim().is_empty()) {
            return Err(DomainError::SlugGivenTwice {
                field: "slug".to_owned(),
            });
        }

        let mut fields = fields.clone();
        if let Some(origin) = origin.map(str::trim).filter(|o| !o.is_empty()) {
            fields.insert("origin".to_owned(), origin.to_owned());
        }
        let fields = &fields;

        for key in fields.keys() {
            if !def.required.contains(key) && !def.optional.contains(key) {
                return Err(DomainError::UnknownField {
                    note_type: type_name.to_owned(),
                    field: key.clone(),
                });
            }
        }
        for req in &def.required {
            let supplied = fields.get(req).is_some_and(|v| !v.trim().is_empty());
            if !supplied && !ENGINE_SUPPLIED_FIELDS.contains(&req.as_str()) {
                return Err(DomainError::MissingRequiredField {
                    note_type: type_name.to_owned(),
                    field: req.clone(),
                });
            }
        }

        let slug = self.explicit_or_derived_slug(slug, title)?;
        let date = at.date().format("%Y-%m-%d").to_string();

        let path = VaultPath::new(format!(
            "{}/{slug}.{}",
            def.folder,
            NoteFormat::Typst.extension()
        ))?;
        if self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                path.to_string(),
            )));
        }
        self.refuse_other_format_stem(&path)?;

        let specs = self
            .config()
            .schemas
            .get(type_name)
            .map(|s| s.declared_fields())
            .unwrap_or_default();
        let value_of = |key: &str| -> Result<Option<Value>, DomainError> {
            let supplied = fields.get(key).filter(|v| !v.trim().is_empty());
            let Some(text) = supplied else {
                return Ok(match key {
                    "title" => Some(Value::String(title.to_owned())),
                    "slug" => Some(Value::String(slug.clone())),
                    "created" | "date" => Some(Value::String(date.clone())),
                    _ => None,
                });
            };
            let Some(spec) = specs.get(key) else {
                return Ok(Some(Value::String(text.clone())));
            };
            let typed = match spec.ty {
                FieldType::Bool | FieldType::Int | FieldType::Float => {
                    coerce_value(key, type_name, spec.ty, text)?
                }
                FieldType::String | FieldType::Date => Value::String(text.clone()),
            };
            if let Some(reason) = spec.check_value(&typed) {
                return Err(DomainError::InvalidFieldValue {
                    note_type: type_name.to_owned(),
                    field: key.to_owned(),
                    reason,
                });
            }
            Ok(Some(typed))
        };

        let order = descriptor
            .custom_frontmatter_order()
            .expect("a custom descriptor always yields an order");
        let mut entries: Vec<(String, Value)> =
            vec![("type".to_owned(), Value::String(type_name.to_owned()))];
        for key in ["title", "created", "origin"]
            .into_iter()
            .map(str::to_owned)
            .chain(
                order
                    .into_iter()
                    .filter(|k| !matches!(k.as_str(), "type" | "title" | "created" | "origin")),
            )
        {
            if let Some(value) = value_of(&key)? {
                entries.push((key, value));
            }
        }

        let body = body.filter(|b| !b.trim().is_empty()).unwrap_or("");
        check_typst_body(body)?;
        let block = write_typed_block(type_name, &entries)?;
        let content = assemble_typst_note(&block, body);

        let entry = build_index_entry_for(&path, &content, type_name)?;
        tx.write_file(path.clone(), content);
        tx.upsert_note(entry);
        self.stage_created_line(&mut tx, at, type_name, &path, title, None)?;
        tx.commit()?;
        Ok(path)
    }

    /// Refuse `path` when a note of the other format already holds its stem in the folder: the
    /// two would make `[[stem]]` ambiguous (RFC 0008 §5.7). Checks the files, so a stem the
    /// index never saw is caught as well.
    pub(super) fn refuse_other_format_stem(&self, path: &VaultPath) -> Result<(), DomainError> {
        let address = note_address(path);
        let own = cdno_core::paths::note_format(path);
        for format in NoteFormat::ALL.into_iter().filter(|f| Some(*f) != own) {
            let Ok(other) = VaultPath::new(format!("{address}.{}", format.extension())) else {
                continue;
            };
            if self.store.exists(&other)? {
                let stem = other
                    .as_path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_owned();
                return Err(DomainError::StemTakenByOtherFormat {
                    stem,
                    holder: other,
                });
            }
        }
        Ok(())
    }
}

/// As [`render_metadata_block`], but each value is written as its own type: the caller has
/// already coerced a declared int, float or bool.
fn write_typed_block(note_type: &str, entries: &[(String, Value)]) -> Result<String, DomainError> {
    typst::write_block(entries).map_err(|e| DomainError::InvalidFieldValue {
        note_type: note_type.to_string(),
        field: "(metadata block)".to_string(),
        reason: match e {
            TypstError::Unwritable { what } => what,
            other => other.to_string(),
        },
    })
}
