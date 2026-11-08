//! The Typst note writer's shared parts (RFC 0008 §5.7): the metadata block from the engine's
//! fields, the one check a body passes before it is written, and the assembly of the file.
//!
//! Public so the integration suite can reach it; the verbs that call it (T19, T23, T26) live in
//! sibling files.

use cdno_core::typst::{self, TypstError};
use serde_json::Value;

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

/// The pure check shared with lint: the body must not carry `<cdno>`, must parse, and must hold
/// no link whose interior is not one inline run. Lines count in the body.
pub fn check_typst_body(body: &str) -> Result<(), BodyRejection> {
    if typst::has_label(body) {
        return Err(BodyRejection::CarriesBlock);
    }
    typst::parses(body).map_err(|f| BodyRejection::DoesNotParse {
        line: f.line,
        message: f.message,
    })?;
    if let Some(fault) = typst::wikilink_spans(body).faults.into_iter().next() {
        return Err(BodyRejection::LinkNeedsEscape {
            line: fault.line,
            leaf: fault.leaf,
        });
    }
    Ok(())
}

/// The block, a blank line, then the body as given, with a trailing newline ensured.
pub fn assemble_typst_note(block: &str, body: &str) -> String {
    let mut out = String::with_capacity(block.len() + body.len() + 2);
    out.push_str(block);
    out.push_str("\n\n");
    out.push_str(body);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}
