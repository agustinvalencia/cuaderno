//! Merging structured fields into an existing frontmatter block (#481).
//!
//! Line-based and value-preserving, in the same spirit as
//! [`rewrite_field_in_frontmatter`](super::rewrite_field_in_frontmatter) and
//! the `normalise` pass: it never re-emits the whole block from a parsed
//! model, so every key the caller did not name — its quoting, its `null`s, its
//! comments, its order — survives byte-for-byte. Only the named keys are
//! rendered, and only the lines they occupy change.
//!
//! Unlike the single-scalar rewriter this accepts *nested* values, because a
//! tracking entry's metrics can be a sequence of records
//! (RFC 0001 §5.1). A nested value is emitted as an indented block under its
//! key, which `normalise`'s `top_level_key` reads as continuation lines and
//! therefore moves as one group.

use serde_json::Value;

use crate::error::DomainError;

/// Merge `fields` into `raw`'s frontmatter block, returning the new document.
///
/// A key already present as a **single-line** scalar (`routine: null`, the
/// shape a template scaffolds) is replaced where it stands, keeping its
/// position in the block. A key not present is appended just before the
/// closing `---`.
///
/// Key order — both the appended keys and the keys inside a record — is
/// `serde_json::Map`'s, which is alphabetical (the workspace does not enable
/// `preserve_order`). Deterministic, and cosmetic: a series groups on a named
/// field and orders on the *sequence*, never on key position.
///
/// Errors:
/// - [`DomainError::MissingSection`] — `raw` has no frontmatter block.
/// - [`DomainError::MultilineFrontmatterField`] — the key is present and
///   already carries a nested block. Replacing it means consuming an unknown
///   number of continuation lines, and guessing wrong would silently drop or
///   duplicate data; a freshly scaffolded note never has one, so this errors
///   rather than guesses.
pub fn merge_fields_into_frontmatter(
    raw: &str,
    fields: &serde_json::Map<String, Value>,
) -> Result<String, DomainError> {
    if fields.is_empty() {
        return Ok(raw.to_owned());
    }

    // Locate the frontmatter region: the opening `---\n` must be at the very
    // start, and the closing `\n---` marks the end. Same bounds as
    // `rewrite_field_in_frontmatter`, so the two agree on what "the block" is.
    let opening = "---\n";
    if !raw.starts_with(opening) {
        return Err(DomainError::MissingSection("frontmatter"));
    }
    let body_after_open = opening.len();
    let closing_offset = raw[body_after_open..]
        .find("\n---")
        .ok_or(DomainError::MissingSection("frontmatter"))?;
    let yaml_end = body_after_open + closing_offset + 1; // include the trailing \n
    let yaml = &raw[body_after_open..yaml_end];

    let mut new_yaml = String::with_capacity(yaml.len() + 128);
    let mut appended = Vec::new();

    // First pass: replace in place any key the block already declares on one
    // line. A key whose existing value spans lines is a hard error, so scan
    // for that before writing anything.
    let lines: Vec<&str> = yaml.split_inclusive('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        match declared_key(line) {
            Some(key) if fields.contains_key(key) => {
                if continues_onto_next_line(line, lines.get(i + 1).copied()) {
                    return Err(DomainError::MultilineFrontmatterField(key.to_owned()));
                }
                let value = &fields[key];
                new_yaml.push_str(&render_field(key, value)?);
                appended.push(key.to_owned());
            }
            _ => new_yaml.push_str(line),
        }
    }

    // Second pass: everything the block did not already declare goes at the
    // end, in the map's key order.
    for (key, value) in fields {
        if !appended.iter().any(|k| k == key) {
            new_yaml.push_str(&render_field(key, value)?);
        }
    }

    let mut result = String::with_capacity(raw.len() + new_yaml.len());
    result.push_str(&raw[..body_after_open]);
    result.push_str(&new_yaml);
    result.push_str(&raw[yaml_end..]);
    Ok(result)
}

/// The top-level key a frontmatter line declares, or `None` for a
/// continuation line (indented, a list item, blank, or without a colon).
/// Mirrors `normalise::top_level_key` so the two passes agree on what counts
/// as a key rather than drifting apart.
fn declared_key(line: &str) -> Option<&str> {
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let key = line.split(':').next()?.trim_end();
    if key.is_empty() || key.contains(char::is_whitespace) || !line.contains(':') {
        return None;
    }
    Some(key)
}

/// Whether `line`'s value spills onto following lines — either the key has no
/// inline value and the next line is indented (a nested block), or the next
/// line is a `- ` sequence item belonging to it.
fn continues_onto_next_line(line: &str, next: Option<&str>) -> bool {
    let inline_value = line.split_once(':').map(|(_, v)| v.trim()).unwrap_or("");
    if !inline_value.is_empty() {
        return false; // `key: value` — self-contained
    }
    // `key:` with nothing after it: a nested block iff something indented or a
    // sequence item follows.
    match next {
        Some(next) => {
            let trimmed = next.trim_start();
            !trimmed.is_empty()
                && (next.starts_with(char::is_whitespace) || trimmed.starts_with("- "))
        }
        None => false,
    }
}

/// Render one `key: value` frontmatter entry, as one line for a scalar or an
/// indented block for a nested value. Always ends with a newline.
fn render_field(key: &str, value: &Value) -> Result<String, DomainError> {
    let yaml =
        serde_yaml::to_string(value).map_err(|e| DomainError::UnrepresentableFrontmatterValue {
            field: key.to_owned(),
            reason: e.to_string(),
        })?;
    let yaml = yaml.trim_end_matches('\n');

    // A scalar serialises to a single line; anything else (a sequence, a
    // mapping) becomes a block indented two spaces under the bare key.
    if !yaml.contains('\n') && !yaml.starts_with("- ") {
        return Ok(format!("{key}: {yaml}\n"));
    }
    let mut out = format!("{key}:\n");
    for line in yaml.lines() {
        if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}
