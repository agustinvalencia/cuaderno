use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde_yaml::Value;

use crate::error::{ParseError, ValidationError};

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

    /// Read an optional field as type `T`. Returns `None` if absent,
    /// `Some(value)` if present and well-typed, or
    /// [`ValidationError::InvalidField`] if present but the stored
    /// value cannot be deserialised as `T`. Type mismatches intentionally
    /// error rather than silently returning `None` — a missing field and
    /// a malformed field are different bugs.
    pub fn optional_field<T: DeserializeOwned>(
        &self,
        name: &str,
    ) -> Result<Option<T>, ValidationError> {
        match self.fields.get(name) {
            None => Ok(None),
            Some(value) => deserialise_field(name, value).map(Some),
        }
    }
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
