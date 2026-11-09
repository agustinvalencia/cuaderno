//! The Typst note writer's shared parts (RFC 0008 §5.7): the metadata block from the engine's
//! fields, the one check a body passes before it is written, and the assembly of the file.
//!
//! Public so the integration suite can reach it. The verbs that write a note (T19, T23, T26) call
//! it; lint (T15) reads the same facts through the core `typst` primitives, not through here.

use cdno_core::error::StoreError;
use cdno_core::path::VaultPath;
use cdno_core::typst::{self, TypstError};
use chrono::NaiveDateTime;
use serde_json::{Value, json};

use crate::error::DomainError;
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;

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

impl Vault {
    /// Stem exclusivity (RFC 0008 §5.7, P6): refuse a Markdown path whose `.typ` sibling exists,
    /// since `[[stem]]` would name two notes. Reads only; called before anything is staged.
    pub(in crate::vault) fn refuse_if_stem_held_by_typst(
        &self,
        md_path: &VaultPath,
    ) -> Result<(), DomainError> {
        let text = md_path.to_string();
        let Some(stem) = text.strip_suffix(".md") else {
            return Ok(());
        };
        let holder = VaultPath::new(format!("{stem}.typ"))?;
        if self.store.exists(&holder)? {
            return Err(DomainError::StemTakenByOtherFormat {
                stem: stem.to_string(),
                holder,
            });
        }
        Ok(())
    }

    /// File an evidence note written in Typst into `portfolios/<portfolio>/` (RFC 0008 §5.7).
    ///
    /// The same `source` and `origin` rules as [`file_evidence`](Self::file_evidence); the file is
    /// `<YYYY-MM-DD>-<source-slug>.typ`, the metadata block then `body`. Every refusal runs before
    /// anything is staged, so the vault is unchanged.
    ///
    /// Errors, beyond those of the Markdown verb:
    /// - [`DomainError::FormatUnsupportedForType`] — `evidence` is not in `[typst] note_types`.
    /// - [`DomainError::StemTakenByOtherFormat`] — the same stem exists as `.md`.
    /// - [`DomainError::BodyCarriesMetadataBlock`], [`DomainError::BodyDoesNotParse`],
    ///   [`DomainError::LinkNeedsEscape`] — from the body check.
    pub fn file_typst_evidence(
        &self,
        at: NaiveDateTime,
        portfolio: &str,
        source: &str,
        origin: &str,
        body: &str,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?;
        let source = source.trim();
        let origin = origin.trim();
        if !self
            .type_registry()
            .admits_typst(NoteType::Evidence.as_str())
        {
            return Err(DomainError::FormatUnsupportedForType {
                note_type: NoteType::Evidence.as_str().to_string(),
            });
        }
        if source.is_empty() {
            return Err(DomainError::EmptyField { field: "source" });
        }
        if origin.is_empty() {
            return Err(DomainError::EmptyField { field: "origin" });
        }
        if origin.contains("[[") || origin.contains("]]") {
            return Err(DomainError::MalformedWikilink {
                value: origin.to_owned(),
            });
        }

        let portfolio_index = VaultPath::new(format!(
            "{}/{portfolio}/_index.md",
            cdno_core::paths::PORTFOLIOS
        ))?;
        if !self.store.exists(&portfolio_index)? {
            return Err(DomainError::Store(StoreError::NotFound(format!(
                "{portfolio_index}{}",
                self.available_portfolios_hint()
            ))));
        }

        let created = at.date();
        let stem = format!(
            "{}/{portfolio}/{}-{}",
            cdno_core::paths::PORTFOLIOS,
            created.format("%Y-%m-%d"),
            slugify(source)
        );
        let path = VaultPath::new(format!("{stem}.typ"))?;
        if self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                path.to_string(),
            )));
        }
        let holder = VaultPath::new(format!("{stem}.md"))?;
        if self.store.exists(&holder)? {
            return Err(DomainError::StemTakenByOtherFormat { stem, holder });
        }

        check_typst_body(body)?;

        let block = render_metadata_block(
            NoteType::Evidence.as_str(),
            &[
                ("type".to_string(), json!(NoteType::Evidence.as_str())),
                (
                    "created".to_string(),
                    json!(created.format("%Y-%m-%d").to_string()),
                ),
                ("source".to_string(), json!(source)),
                ("portfolio".to_string(), json!(portfolio)),
                ("origin".to_string(), json!(format!("[[{origin}]]"))),
            ],
        )?;
        let content = assemble_typst_note(&block, body);
        let entry = build_index_entry_for(&path, &content, NoteType::Evidence.as_str())?;

        tx.write_file(path.clone(), content);
        tx.upsert_note(entry);
        self.stage_created_line(
            &mut tx,
            at,
            NoteType::Evidence.as_str(),
            &path,
            source,
            None,
        )?;
        tx.commit()?;

        Ok(path)
    }
}
