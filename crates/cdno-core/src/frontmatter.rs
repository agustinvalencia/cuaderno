use std::collections::HashMap;
use std::ops::Range;

use serde::de::DeserializeOwned;
use serde_yaml::Value;

use crate::error::{ParseError, ValidationError};
use crate::path::VaultPath;
use crate::paths::{NoteFormat, note_format};

/// Parsed YAML frontmatter block with typed field access.
///
/// Constructed via [`Frontmatter::parse`], which takes the entire
/// raw markdown document and returns the frontmatter plus the
/// remaining body. Typed access to individual fields uses
/// [`require_field`](Self::require_field) for mandatory fields and
/// [`optional_field`](Self::optional_field) for optional ones. Both
/// deserialise on demand, so a caller can ask the same field as
/// different types without re-parsing the YAML.
#[derive(Debug, Clone, Default)]
pub struct Frontmatter {
    fields: HashMap<String, Value>,
}

impl Frontmatter {
    /// Parse frontmatter and body from a raw markdown document.
    ///
    /// The input must begin with a `---` line (with `\n` or `\r\n`
    /// line endings), contain a closing `---` line, and have valid
    /// YAML between the two. On success, returns the parsed
    /// frontmatter and a `&str` slice of the body that follows the
    /// closing delimiter.
    pub fn parse(raw: &str) -> Result<(Self, &str), ParseError> {
        let Some(body_after_open) = strip_opening_delim(raw) else {
            return Err(ParseError::MissingFrontmatter(
                "no opening '---' delimiter at start of document".to_owned(),
            ));
        };

        let Some((yaml_block, body)) = split_at_closing_delim(body_after_open) else {
            return Err(ParseError::InvalidFrontmatter(
                "opening '---' delimiter has no matching closing '---'".to_owned(),
            ));
        };

        // Empty block is a valid "no fields" state — skip YAML parsing.
        if yaml_block.trim().is_empty() {
            return Ok((Self::default(), body));
        }

        let value: Value =
            serde_yaml::from_str(yaml_block).map_err(|e| ParseError::Yaml(e.to_string()))?;

        // Top-level frontmatter must be a mapping. A plain list or
        // scalar is syntactically valid YAML but useless as
        // frontmatter — reject it at this boundary so field accessors
        // always see a well-shaped map.
        let mapping = match value {
            Value::Mapping(m) => m,
            Value::Null => serde_yaml::Mapping::new(),
            _ => {
                return Err(ParseError::InvalidFrontmatter(
                    "frontmatter must be a YAML mapping".to_owned(),
                ));
            }
        };

        let mut fields = HashMap::with_capacity(mapping.len());
        for (key, val) in mapping {
            let Value::String(key_str) = key else {
                return Err(ParseError::InvalidFrontmatter(
                    "frontmatter keys must be strings".to_owned(),
                ));
            };
            fields.insert(key_str, val);
        }

        Ok((Self { fields }, body))
    }

    /// The metadata of a Typst note, built into the same type [`Frontmatter::parse`] builds. A
    /// value that is not an object has no fields.
    pub fn from_typst(value: serde_json::Value) -> Self {
        let mut fields = HashMap::new();
        if let serde_json::Value::Object(map) = value {
            fields.reserve(map.len());
            for (key, val) in map {
                let val = serde_yaml::to_value(val).unwrap_or(Value::Null);
                fields.insert(key, val);
            }
        }
        Self { fields }
    }

    /// Read a required field as type `T`. Returns
    /// [`ValidationError::MissingField`] if absent, or
    /// [`ValidationError::InvalidField`] if the stored value cannot
    /// be deserialised as `T`.
    pub fn require_field<T: DeserializeOwned>(&self, name: &str) -> Result<T, ValidationError> {
        let value = self
            .fields
            .get(name)
            .ok_or_else(|| ValidationError::MissingField {
                field: name.to_owned(),
            })?;
        deserialise_field(name, value)
    }

    /// Read an optional field as type `T`. Returns `None` if the
    /// field is absent **or** explicitly set to YAML `null`,
    /// `Some(value)` if present and well-typed, or
    /// [`ValidationError::InvalidField`] if present with a non-null
    /// value that cannot be deserialised as `T`.
    ///
    /// YAML `null` is the canonical way to write "this optional
    /// field has no value" — treating it as absent matches the
    /// `lint` checker (which also collapses `null` into "missing")
    /// and lets writers emit `field: null` placeholders without
    /// the parser flagging a non-existent type mismatch. Genuine
    /// type mismatches (e.g. an integer where a string is expected)
    /// still error, since absent vs malformed remain different bugs.
    pub fn optional_field<T: DeserializeOwned>(
        &self,
        name: &str,
    ) -> Result<Option<T>, ValidationError> {
        match self.fields.get(name) {
            None => Ok(None),
            Some(Value::Null) => Ok(None),
            Some(value) => deserialise_field(name, value).map(Some),
        }
    }

    /// Serialise the full frontmatter to a `serde_json::Value::Object`.
    ///
    /// Used by the index layer to populate the JSON-blob `frontmatter`
    /// column. YAML scalars that don't have JSON equivalents (e.g.
    /// native YAML dates) are stringified via serde's default
    /// conversion.
    pub fn as_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::with_capacity(self.fields.len());
        for (key, value) in &self.fields {
            let json_value = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
            map.insert(key.clone(), json_value);
        }
        serde_json::Value::Object(map)
    }
}

/// A note read through [`parse_note_spanned`]: the metadata, the body, and where the metadata
/// sits in the source.
#[derive(Debug, Clone)]
pub struct Spanned<'a> {
    pub frontmatter: Frontmatter,
    pub body: &'a str,
    /// For Markdown, the frontmatter block from the opening `---` to the end of the closing
    /// line; for Typst, the `#metadata((…)) <cdno>` block from `#` to `>`.
    pub metadata_span: Range<usize>,
}

/// Read a note by its format (P5): Markdown through [`Frontmatter::parse`], Typst through the
/// metadata block, the body being the bytes after the `<cdno>` label. A path with no note
/// extension is a [`ParseError`].
pub fn parse_note<'a>(
    path: &VaultPath,
    raw: &'a str,
) -> Result<(Frontmatter, &'a str), ParseError> {
    parse_note_spanned(path, raw).map(|s| (s.frontmatter, s.body))
}

/// [`parse_note`], also returning the metadata span, for the writers that edit the block. The
/// Typst source is parsed once.
pub fn parse_note_spanned<'a>(path: &VaultPath, raw: &'a str) -> Result<Spanned<'a>, ParseError> {
    match note_format(path) {
        Some(NoteFormat::Markdown) => {
            let (frontmatter, body) = Frontmatter::parse(raw)?;
            Ok(Spanned {
                frontmatter,
                body,
                metadata_span: 0..raw.len() - body.len(),
            })
        }
        Some(NoteFormat::Typst) => {
            let block = crate::typst::metadata_block(raw)?;
            let end = block.span.end;
            Ok(Spanned {
                frontmatter: Frontmatter::from_typst(block.value),
                body: &raw[end..],
                metadata_span: block.span,
            })
        }
        None => Err(ParseError::MissingFrontmatter(format!(
            "{path} is not a note file (expected .md or .typ)"
        ))),
    }
}

/// Split a raw markdown document into its frontmatter YAML block and the
/// body after the closing delimiter, by the same delimiter rules as
/// [`Frontmatter::parse`] but without parsing the YAML. `None` when the
/// document does not open with a `---` line or the block is never closed.
pub fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    split_at_closing_delim(strip_opening_delim(raw)?)
}

/// Whether `text`, as a bare YAML scalar, is read back as a string by the
/// parser frontmatter goes through. `true`, `null`, `~`, `0o17` and `1e-3`
/// are not; a text that fails to parse, or is empty, counts as not a string.
pub fn yaml_scalar_is_string(text: &str) -> bool {
    matches!(serde_yaml::from_str::<Value>(text), Ok(Value::String(_)))
}

fn deserialise_field<T: DeserializeOwned>(name: &str, value: &Value) -> Result<T, ValidationError> {
    serde_yaml::from_value(value.clone()).map_err(|e| ValidationError::InvalidField {
        field: name.to_owned(),
        reason: e.to_string(),
    })
}

/// Strip the opening `---` delimiter line and return the rest, or
/// `None` if the document does not start with `---` followed by a
/// line break. Accepts `\n` and `\r\n`.
fn strip_opening_delim(raw: &str) -> Option<&str> {
    if let Some(rest) = raw.strip_prefix("---\n") {
        return Some(rest);
    }
    if let Some(rest) = raw.strip_prefix("---\r\n") {
        return Some(rest);
    }
    None
}

/// Split at the first closing `---` delimiter line. Returns
/// `(yaml_block, body)` or `None` if no closing delimiter is found.
///
/// A closing delimiter is `---` on its own line — either at the very
/// end of the input, or followed by `\n` / `\r\n`. Any `---` that
/// appears mid-line (e.g. a YAML scalar value) is not a closing
/// delimiter.
fn split_at_closing_delim(after_open: &str) -> Option<(&str, &str)> {
    // Two forms to search for:
    //   "\n---\n"   — delimiter with newline after
    //   "\n---\r\n" — delimiter with CRLF after
    //   "\n---"     — delimiter at end of input (with no trailing newline)
    //
    // Search by walking lines. Byte offsets let us slice cleanly.
    let mut pos = 0usize;
    while pos < after_open.len() {
        let rest = &after_open[pos..];
        let (line, consumed, end_of_line) = match rest.find('\n') {
            Some(nl) => {
                // Trim a \r preceding the \n so we don't count it as part of the line.
                let line_end = if nl > 0 && rest.as_bytes()[nl - 1] == b'\r' {
                    nl - 1
                } else {
                    nl
                };
                (&rest[..line_end], nl + 1, false)
            }
            None => (rest, rest.len(), true),
        };

        if line == "---" {
            // YAML block spans from the start of after_open up to the
            // byte just before this line's first char.
            let yaml_block = &after_open[..pos];
            let body_start = pos + consumed;
            let body = if body_start >= after_open.len() || end_of_line {
                ""
            } else {
                &after_open[body_start..]
            };
            return Some((yaml_block, body));
        }

        pos += consumed;
    }
    None
}
