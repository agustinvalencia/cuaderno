//! Writing and editing the metadata block (RFC 0008 §5.3), the Typst twin of
//! `frontmatter_edit.rs`: an edit lands at a span, and nothing else in the source moves.

use std::ops::Range;

use serde_json::Value;
use typst_syntax::is_ident;

use crate::extractors::AddressEdit;

use super::metadata::{MetadataBlock, TypstError, metadata_block};

/// Typst's keywords: an identifier the parser reads as a keyword is no dictionary key.
const KEYWORDS: &[&str] = &[
    "none", "auto", "true", "false", "not", "and", "or", "let", "set", "show", "context", "if",
    "else", "for", "in", "while", "break", "continue", "return", "import", "include", "as",
];

const INDENT: &str = "  ";

/// The block for `fields`, in the given order: one key per line, `<cdno>` after the closing
/// parentheses. No trailing newline. Strings take Typst's string escapes, never the markup
/// escape rule. Fails on a value the reader would refuse: null, an object, a nested array, an
/// array item that is not a string, a repeated key, or an integer outside `i64::MIN + 1..=i64::MAX` (Typst reads a wider literal as a float).
pub fn write_block(fields: &[(String, Value)]) -> Result<String, TypstError> {
    for (i, (key, _)) in fields.iter().enumerate() {
        if fields[..i].iter().any(|(k, _)| k == key) {
            return Err(TypstError::Unwritable {
                what: format!("the key `{key}` appears twice"),
            });
        }
    }
    if fields.is_empty() {
        return Ok("#metadata((:)) <cdno>".to_string());
    }
    let mut out = String::from("#metadata((\n");
    for (key, value) in fields {
        out.push_str(&format!(
            "{INDENT}{}: {},\n",
            key_token(key),
            literal(value)?
        ));
    }
    out.push_str(")) <cdno>");
    Ok(out)
}

/// Where `key`'s value sits in the source.
pub fn scalar_span(block: &MetadataBlock, key: &str) -> Option<Range<usize>> {
    block
        .entries
        .iter()
        .find(|e| e.key == key)
        .map(|e| e.value_span.clone())
}

/// Set `key` to `value`: the value's span is replaced, or the key inserted where the dictionary
/// closes. Returns the new source and the edit made (a range of `source` and its replacement),
/// which `differs_only_by` checks exactly.
pub fn rewrite_scalar(
    source: &str,
    key: &str,
    value: &Value,
) -> Result<(String, Vec<AddressEdit>), TypstError> {
    let block = metadata_block(source)?;
    let text = literal(value)?;
    match scalar_span(&block, key) {
        Some(span) => Ok(applied(splice(source, span, &text))),
        None => Ok(applied(insert_entry(source, &block, key, &text))),
    }
}

/// Add `item` to the array at `key`: `("a",)` and `("a")` both become `("a", "b")`. An absent key
/// is inserted as a one-element array. Nothing changes (the range is empty) when `item` is
/// already there. Only the array's own span is rewritten, in one-line form.
pub fn append_list_value(
    source: &str,
    key: &str,
    item: &str,
) -> Result<(String, Vec<AddressEdit>), TypstError> {
    let block = metadata_block(source)?;
    let Some(span) = scalar_span(&block, key) else {
        let text = literal(&Value::Array(vec![Value::String(item.to_string())]))?;
        return Ok(applied(insert_entry(source, &block, key, &text)));
    };
    let mut items = items_of(&block, key)?;
    if items.iter().any(|i| i == item) {
        return Ok((source.to_string(), Vec::new()));
    }
    items.push(item.to_string());
    Ok(applied(splice(source, span, &array_literal(&items))))
}

/// Remove `item` from the array at `key`; the last one leaves `()`. Nothing changes (the range is
/// empty) when the key or the item is absent.
pub fn remove_list_value(
    source: &str,
    key: &str,
    item: &str,
) -> Result<(String, Vec<AddressEdit>), TypstError> {
    let block = metadata_block(source)?;
    let Some(span) = scalar_span(&block, key) else {
        return Ok((source.to_string(), Vec::new()));
    };
    let mut items = items_of(&block, key)?;
    if !items.iter().any(|i| i == item) {
        return Ok((source.to_string(), Vec::new()));
    }
    items.retain(|i| i != item);
    Ok(applied(splice(source, span, &array_literal(&items))))
}

/// The edit that makes the string literal whose content sits at `span` read `value`: `span` is
/// inside the quotes (a [`PathLiteral`](super::PathLiteral)'s), and the value takes the same
/// string escapes as every other literal this module writes.
pub fn rewrite_string_content(span: Range<usize>, value: &str) -> AddressEdit {
    let quoted = quote(value);
    AddressEdit {
        range: span,
        replacement: quoted[1..quoted.len() - 1].to_string(),
    }
}

fn items_of(block: &MetadataBlock, key: &str) -> Result<Vec<String>, TypstError> {
    let not_a_list = || TypstError::Unwritable {
        what: format!("`{key}` is not a list of strings"),
    };
    block
        .value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(not_a_list)?
        .iter()
        .map(|v| v.as_str().map(str::to_string).ok_or_else(not_a_list))
        .collect()
}

fn splice(source: &str, span: Range<usize>, text: &str) -> (String, AddressEdit) {
    let mut out = String::with_capacity(source.len() + text.len());
    out.push_str(&source[..span.start]);
    out.push_str(text);
    out.push_str(&source[span.end..]);
    (
        out,
        AddressEdit {
            range: span,
            replacement: text.to_string(),
        },
    )
}

fn applied((source, edit): (String, AddressEdit)) -> (String, Vec<AddressEdit>) {
    (source, vec![edit])
}

/// Insert `key: text` as a new entry, in the layout the dictionary already has.
fn insert_entry(
    source: &str,
    block: &MetadataBlock,
    key: &str,
    text: &str,
) -> (String, AddressEdit) {
    let eol = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let entry = format!("{}: {}", key_token(key), text);
    if let Some(colon) = &block.empty_colon {
        return splice(
            source,
            colon.clone(),
            &format!("{eol}{INDENT}{entry},{eol}"),
        );
    }
    let Some(last) = block.entries.last() else {
        return splice(
            source,
            block.insert_at..block.insert_at,
            &format!("{eol}{INDENT}{entry},{eol}"),
        );
    };
    let before_close = &source[..block.insert_at];
    let line_start = before_close.rfind('\n').map_or(0, |i| i + 1);
    let own_line = line_start > last.value_span.end && before_close[line_start..].trim().is_empty();
    let indent = indent_of(source, last.key_span.start);
    match (block.trailing_comma, own_line) {
        (true, true) => splice(
            source,
            line_start..line_start,
            &format!("{indent}{entry},{eol}"),
        ),
        (true, false) => splice(
            source,
            block.insert_at..block.insert_at,
            &format!(" {entry},"),
        ),
        (false, true) => splice(
            source,
            last.value_span.end..last.value_span.end,
            &format!(",{eol}{indent}{entry}"),
        ),
        (false, false) => splice(
            source,
            last.value_span.end..last.value_span.end,
            &format!(", {entry}"),
        ),
    }
}

/// The whitespace before the key on its line, or the default indent when the key shares its line.
fn indent_of(source: &str, key_start: usize) -> String {
    let line_start = source[..key_start].rfind('\n').map_or(0, |i| i + 1);
    let before = &source[line_start..key_start];
    if line_start > 0 && before.trim().is_empty() {
        before.to_string()
    } else {
        INDENT.to_string()
    }
}

fn key_token(key: &str) -> String {
    if is_ident(key) && key != "_" && !KEYWORDS.contains(&key) {
        key.to_string()
    } else {
        quote(key)
    }
}

fn literal(value: &Value) -> Result<String, TypstError> {
    let unwritable = |what: &str| TypstError::Unwritable {
        what: what.to_string(),
    };
    match value {
        Value::String(s) => Ok(quote(s)),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Number(n)
            if n.as_i64().is_some_and(|i| i == i64::MIN) || n.is_u64() && !n.is_i64() =>
        {
            Err(unwritable("an integer the reader would take for a float"))
        }
        Value::Number(n) => Ok(n.to_string()),
        Value::Array(items) => {
            let strings: Option<Vec<String>> = items
                .iter()
                .map(|i| i.as_str().map(str::to_string))
                .collect();
            strings
                .map(|s| array_literal(&s))
                .ok_or_else(|| unwritable("an array holds something other than strings"))
        }
        Value::Null => Err(unwritable("null")),
        Value::Object(_) => Err(unwritable("a nested dictionary")),
    }
}

/// `()`, `("a",)` (a one-element array needs its comma), `("a", "b")`.
fn array_literal(items: &[String]) -> String {
    match items {
        [] => "()".to_string(),
        [one] => format!("({},)", quote(one)),
        many => format!(
            "({})",
            many.iter().map(|i| quote(i)).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// A Typst string literal. `"` and `\` are escaped; so are line breaks and control characters, so
/// that one value stays on one line.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
