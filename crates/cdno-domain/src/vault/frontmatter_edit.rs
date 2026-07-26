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
//!
//! The caller's keys are untrusted — an agent supplies them — so both the key
//! and the value go through the YAML emitter rather than being interpolated
//! as text. Interpolating a key is how `{"x: 1\nweight": …}` would smuggle a
//! second frontmatter line past the caller's own validation.

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
/// - [`DomainError::MultilineFrontmatterField`] — the key is present and its
///   value already spans lines. Replacing it means consuming an unknown number
///   of continuation lines, and guessing wrong would silently drop or
///   duplicate data; a freshly scaffolded note never has one.
/// - [`DomainError::UnrepresentableFrontmatterValue`] — the key carries a line
///   break (never a legitimate frontmatter key) or the value cannot be
///   serialised as YAML.
pub fn merge_fields_into_frontmatter(
    raw: &str,
    fields: &serde_json::Map<String, Value>,
) -> Result<String, DomainError> {
    if fields.is_empty() {
        return Ok(raw.to_owned());
    }

    // Locate the frontmatter region. `Frontmatter::parse` accepts both `\n`
    // and `\r\n`, so a merge that only understood LF would report "missing
    // frontmatter" for a note the parser reads fine — and a template file is
    // exactly the sort of thing that arrives CRLF-terminated.
    let (opening, newline) = if let Some(rest) = raw.strip_prefix("---\r\n") {
        (raw.len() - rest.len(), "\r\n")
    } else if let Some(rest) = raw.strip_prefix("---\n") {
        (raw.len() - rest.len(), "\n")
    } else {
        return Err(DomainError::MissingSection("frontmatter"));
    };
    let closing_offset = raw[opening..]
        .find("\n---")
        .ok_or(DomainError::MissingSection("frontmatter"))?;
    let yaml_end = opening + closing_offset + 1; // include the trailing \n
    let yaml = &raw[opening..yaml_end];

    let mut new_yaml = String::with_capacity(yaml.len() + 128);
    let mut replaced: Vec<&str> = Vec::new();

    // First pass: replace in place any key the block already declares whose
    // value fits on one line. A key whose value spans lines is a hard error.
    let lines: Vec<&str> = yaml.split_inclusive('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        match declared_key(line).and_then(|k| fields.get_key_value(k)) {
            Some((key, value)) => {
                if continues_onto_next_line(lines.get(i + 1).copied()) {
                    return Err(DomainError::MultilineFrontmatterField(key.clone()));
                }
                new_yaml.push_str(&render_field(key, value, newline)?);
                replaced.push(key);
            }
            None => new_yaml.push_str(line),
        }
    }

    // Second pass: everything the block did not already declare goes at the
    // end, in the map's key order.
    for (key, value) in fields {
        if !replaced.contains(&key.as_str()) {
            new_yaml.push_str(&render_field(key, value, newline)?);
        }
    }

    let mut result = String::with_capacity(raw.len() + new_yaml.len());
    result.push_str(&raw[..opening]);
    result.push_str(&new_yaml);
    result.push_str(&raw[yaml_end..]);
    Ok(result)
}

/// The top-level YAML key a frontmatter line declares, or `None` for a
/// continuation line (indented, a list item, blank, or without a colon).
/// Mirrors `normalise::top_level_key` so the two passes agree on what counts
/// as a key rather than drifting apart, with one addition: a quoted key is
/// unwrapped, so `"weight": 82` is recognised as `weight` and replaced rather
/// than appended a second time.
fn declared_key(line: &str) -> Option<&str> {
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let key = line.split(':').next()?.trim_end();
    let unquoted = key
        .strip_prefix('"')
        .and_then(|k| k.strip_suffix('"'))
        .or_else(|| key.strip_prefix('\'').and_then(|k| k.strip_suffix('\'')))
        .unwrap_or(key);
    if unquoted.is_empty() || unquoted.contains(char::is_whitespace) || !line.contains(':') {
        return None;
    }
    Some(unquoted)
}

/// Whether the line *after* a key belongs to that key's value.
///
/// Decided from the following line alone, deliberately. Judging by the key's
/// own inline value would miss every block scalar (`key: |`, `key: >`) and
/// every wrapped or multi-line flow collection — all of which carry a
/// non-empty inline value and still continue. Replacing one of those orphans
/// its continuation lines, which either breaks the document or, worse, lets
/// the orphaned text be absorbed into the replacement value.
fn continues_onto_next_line(next: Option<&str>) -> bool {
    match next {
        Some(next) => {
            let trimmed = next.trim_start();
            !trimmed.is_empty()
                && (next.starts_with(char::is_whitespace) || trimmed.starts_with("- "))
        }
        None => false,
    }
}

/// The characters YAML treats as line breaks. `\n` and `\r` are the obvious
/// two; U+2028 and U+2029 are breaks to the parser as well, and the emitter
/// passes them through literally rather than escaping them.
const YAML_LINE_BREAKS: [char; 4] = ['\n', '\r', '\u{2028}', '\u{2029}'];

/// Render one `key: value` frontmatter entry, as one line for a scalar or an
/// indented block for a non-empty sequence or mapping. Always ends with
/// `newline`.
fn render_field(key: &str, value: &Value, newline: &str) -> Result<String, DomainError> {
    let unrepresentable = |e: serde_yaml::Error| DomainError::UnrepresentableFrontmatterValue {
        field: key.to_owned(),
        reason: e.to_string(),
    };
    // The key goes through the emitter too, so one needing quotes (`#reps`,
    // `true`, `a: b`) is quoted rather than silently becoming a comment, a
    // different key, or a second line.
    let key_scalar = serde_yaml::to_string(&key)
        .map_err(unrepresentable)?
        .trim_end()
        .to_owned();
    // The key is spliced into implicit-key position, where a scalar spanning
    // lines is not legal. Test the EMITTED scalar against YAML's full
    // line-break set, not just `\n`: the emitter writes U+2028/U+2029 through
    // literally (they are breaks to the parser, so the scalar is folded around
    // them) while escaping NEL and tab, so an input-side `\n`/`\r` check would
    // pass such a key through and hand the caller an unparseable block.
    if key_scalar.contains(YAML_LINE_BREAKS) {
        return Err(DomainError::UnrepresentableFrontmatterValue {
            field: key.to_owned(),
            reason: "a frontmatter key cannot contain a line break".to_owned(),
        });
    }
    let yaml = serde_yaml::to_string(value).map_err(unrepresentable)?;
    let yaml = yaml.trim_end_matches('\n');
    // A string value carrying a newline is emitted as a multi-line literal
    // block, so even the inline branch can be more than one physical line.
    // Re-separate on the document's own ending, or a CRLF note ends up with
    // LF-terminated lines in the middle of it. YAML normalises breaks back to
    // `\n` on read, so the value round-trips unchanged.
    let yaml = if newline == "\n" {
        yaml.to_owned()
    } else {
        yaml.replace('\n', newline)
    };
    let yaml = yaml.as_str();

    // Block-vs-inline is decided by the value's SHAPE, not by whether its
    // serialisation happens to fit on one line: a single-key mapping
    // serialises to `duration: 45`, which inlined would read
    // `session: duration: 45` — invalid YAML. An empty collection has no
    // block form and stays inline as `[]` / `{}`.
    let nested = value.as_array().is_some_and(|a| !a.is_empty())
        || value.as_object().is_some_and(|o| !o.is_empty());
    if !nested {
        return Ok(format!("{key_scalar}: {yaml}{newline}"));
    }
    let mut out = format!("{key_scalar}:{newline}");
    for line in yaml.lines() {
        if line.is_empty() {
            out.push_str(newline);
        } else {
            out.push_str("  ");
            out.push_str(line);
            out.push_str(newline);
        }
    }
    Ok(out)
}
