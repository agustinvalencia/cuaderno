//! Frontmatter edits done as text (RFC 0007 §5.4, §5.5).
//!
//! A frontmatter block is edited the way a person would edit it: the
//! bytes of one value change and nothing else moves (comments, key
//! order, blank lines and quoting all stay). The functions here work on
//! the block **without** its `---` fences, exactly what
//! [`split_frontmatter`](crate::frontmatter::split_frontmatter) hands
//! back (it normally ends with a line break). Core knows no keys: `key`
//! is always a parameter.
//!
//! [`frontmatter_changed_only_at`] is the parser-backed check behind the
//! surgery: it re-parses both sides and proves nothing outside the
//! expected keys changed.

use std::ops::Range;

use crate::extractors::AddressEdit;
use crate::frontmatter::{Frontmatter, yaml_scalar_is_string};

/// Which side of a comparison failed to parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftSide {
    Before,
    After,
}

/// Why [`frontmatter_changed_only_at`] refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrontmatterDrift {
    /// One side is not frontmatter the parser accepts.
    #[error("{which:?} frontmatter does not parse: {error}")]
    Unparsable { which: DriftSide, error: String },
    /// A top-level key's value differs.
    #[error("frontmatter key `{0}` changed")]
    KeyChanged(String),
    /// A top-level key exists only in `after`.
    #[error("frontmatter key `{0}` was added")]
    KeyAdded(String),
    /// A top-level key exists only in `before`.
    #[error("frontmatter key `{0}` was removed")]
    KeyRemoved(String),
}

/// A line of the block with its byte offset.
struct Line<'a> {
    start: usize,
    /// The line without its `\n` (a `\r` stays in).
    text: &'a str,
}

fn lines(block: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    for chunk in block.split_inclusive('\n') {
        let text = chunk.strip_suffix('\n').unwrap_or(chunk);
        out.push(Line { start, text });
        start += chunk.len();
    }
    out
}

/// For a top-level `key:` line, the byte offset (within the line) just
/// after the colon. Top-level means column 0; a comment or a `- item`
/// line never matches.
fn key_value_offset(line: &str, key: &str) -> Option<usize> {
    let after = line.strip_prefix(key)?;
    let trimmed = after.trim_start_matches([' ', '\t']);
    let rest = trimmed.strip_prefix(':')?;
    if !(rest.is_empty() || rest.starts_with([' ', '\t', '\r'])) {
        return None;
    }
    Some(line.len() - rest.len())
}

/// Where a value sits on a line, past the colon.
struct ScalarSpan {
    /// Bytes of the scalar's content (inside quotes when quoted), within the line.
    content: Range<usize>,
    quote: Option<char>,
}

/// Locate the scalar after `from_offset` on `line`; `None` when the
/// value is empty, a flow collection, a block scalar, or malformed.
fn scalar_span(line: &str, from_offset: usize) -> Option<ScalarSpan> {
    let rest = &line[from_offset..];
    let lead = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let start = from_offset + lead;
    let value = &line[start..];
    let first = value.chars().next()?;
    match first {
        '"' => {
            let mut escaped = false;
            for (i, c) in value.char_indices().skip(1) {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    return Some(ScalarSpan {
                        content: start + 1..start + i,
                        quote: Some('"'),
                    });
                }
            }
            None
        }
        '\'' => {
            let bytes = value.as_bytes();
            let mut i = 1;
            while i < bytes.len() {
                if bytes[i] == b'\'' {
                    if bytes.get(i + 1) == Some(&b'\'') {
                        i += 2;
                        continue;
                    }
                    return Some(ScalarSpan {
                        content: start + 1..start + i,
                        quote: Some('\''),
                    });
                }
                i += 1;
            }
            None
        }
        '#' | '[' | '{' | '|' | '>' | '&' | '*' | '!' | '%' | '@' | '`' => None,
        _ => {
            // Bare: runs to a ` #` comment or the end of the line.
            let mut end = value.len();
            for (i, _) in value.match_indices('#') {
                if i > 0 && matches!(value.as_bytes()[i - 1], b' ' | b'\t') {
                    end = i;
                    break;
                }
            }
            let content = value[..end].trim_end();
            if content.is_empty() {
                return None;
            }
            Some(ScalarSpan {
                content: start..start + content.len(),
                quote: None,
            })
        }
    }
}

fn escape_double(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_single(s: &str) -> String {
    s.replace('\'', "''")
}

/// Rewrite the value of every top-level `key:` line whose scalar is
/// `from`, to `to`, in the same quoting.
///
/// `frontmatter` is the block between the `---` fences (fences not
/// included). The value may be bare, `'single'` or `"double"` quoted,
/// with any whitespace around it and a trailing `# comment`; each
/// edit's `range` covers only the value's bytes (inside the quotes),
/// relative to `frontmatter`. In quoted values `from` and `to` are
/// given unescaped and escaped to suit the quoting. Nothing else
/// moves: comments, key order, blank lines, other keys, an indented
/// key of the same name, a key named inside a comment, and values that
/// are lists or wikilinks are left alone. A key that appears twice at
/// top level is rewritten at both places. (The drift check
/// then reports such a block as unparsable, since the parser rejects duplicate keys.)
///
/// A bare `to` is written verbatim, so the caller owns choosing a value
/// that stays a plain string; [`frontmatter_changed_only_at`] catches a
/// result that does not.
pub fn rewrite_frontmatter_scalar(
    frontmatter: &str,
    key: &str,
    from: &str,
    to: &str,
) -> (String, Vec<AddressEdit>) {
    let mut edits = Vec::new();
    for line in lines(frontmatter) {
        let Some(offset) = key_value_offset(line.text, key) else {
            continue;
        };
        let Some(span) = scalar_span(line.text, offset) else {
            continue;
        };
        let raw = &line.text[span.content.clone()];
        let (matches, replacement) = match span.quote {
            Some('"') => (raw == escape_double(from), escape_double(to)),
            Some(_) => (raw == escape_single(from), escape_single(to)),
            None => (raw == from, to.to_owned()),
        };
        if matches {
            edits.push(AddressEdit {
                range: line.start + span.content.start..line.start + span.content.end,
                replacement,
            });
        }
    }
    let mut out = String::with_capacity(frontmatter.len());
    let mut at = 0;
    for edit in &edits {
        out.push_str(&frontmatter[at..edit.range.start]);
        out.push_str(&edit.replacement);
        at = edit.range.end;
    }
    out.push_str(&frontmatter[at..]);
    (out, edits)
}

fn unquote_item(item: &str) -> String {
    let item = item.trim();
    if item.len() >= 2 {
        if let Some(inner) = item.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            return inner.replace("\\\"", "\"").replace("\\\\", "\\");
        }
        if let Some(inner) = item.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
            return inner.replace("''", "'");
        }
    }
    item.to_owned()
}

/// `value` as a list item: bare when that is safe, double-quoted otherwise.
fn list_token(value: &str) -> String {
    let plain = yaml_scalar_is_string(value)
        && value == value.trim()
        && !value.contains([',', '[', ']', '{', '}', '#', '"', '\'', '\n'])
        && !value.contains(": ")
        && !value.ends_with(':')
        && !value.starts_with(['-', '?', '&', '*', '!', '|', '>', '%', '@', '`']);
    if plain {
        value.to_owned()
    } else {
        format!("\"{}\"", escape_double(value))
    }
}

/// Strip a trailing ` # comment` from block-list item text.
fn strip_comment(s: &str) -> &str {
    for (i, _) in s.match_indices('#') {
        if i == 0 || matches!(s.as_bytes()[i - 1], b' ' | b'\t') {
            return &s[..i];
        }
    }
    s
}

/// Add `value` to the list under top-level `key`, keeping everything else.
///
/// `frontmatter` is the block between the fences (fences not included).
/// A flow list (`[a, b]`) gets `, value` before the `]` (`[value]` when
/// empty); a block list (`- a` lines) gets a `- value` line after its
/// last item at the same indent. An absent key (or `key:` with no
/// value) gets `key: [value]` as the block's last line, keeping the
/// block's line-ending and trailing-newline convention (an empty block
/// gains one newline). `value` is written bare when that is safe and
/// double-quoted otherwise. When `value` is already an item the text is
/// returned unchanged, and so it is when `key` holds a scalar or
/// anything else that is not a list: a list is never conjured by
/// overwriting a value. Only the first top-level `key:` is considered.
pub fn append_frontmatter_list_value(frontmatter: &str, key: &str, value: &str) -> String {
    let token = list_token(value);
    let all = lines(frontmatter);
    let found = all
        .iter()
        .enumerate()
        .find_map(|(i, l)| key_value_offset(l.text, key).map(|o| (i, o)));

    let Some((idx, offset)) = found else {
        return append_absent(frontmatter, key, &token);
    };
    let line = &all[idx];
    let rest = &line.text[offset..];
    let after_ws = rest.trim_start_matches([' ', '\t']);
    let value_start = line.start + offset + (rest.len() - after_ws.len());

    if after_ws.starts_with('[') {
        // Flow list, possibly spanning lines: find the closing bracket
        // outside quotes.
        let open = value_start;
        let mut quote: Option<char> = None;
        let mut close = None;
        let mut prev = '\0';
        for (i, c) in frontmatter[open + 1..].char_indices() {
            match quote {
                Some(q) => {
                    if c == q && !(q == '"' && prev == '\\') {
                        quote = None;
                    }
                }
                None => match c {
                    '"' | '\'' => quote = Some(c),
                    ']' => {
                        close = Some(open + 1 + i);
                        break;
                    }
                    _ => {}
                },
            }
            prev = c;
        }
        let Some(close) = close else {
            return frontmatter.to_owned();
        };
        let inner = &frontmatter[open + 1..close];
        if inner.trim().is_empty() {
            return format!(
                "{}{}{}",
                &frontmatter[..open + 1],
                token,
                &frontmatter[close..]
            );
        }
        if split_flow_items(inner)
            .iter()
            .any(|i| unquote_item(i) == value)
        {
            return frontmatter.to_owned();
        }
        let content_end = open + 1 + inner.trim_end().len();
        let sep = if inner.trim_end().ends_with(',') {
            " "
        } else {
            ", "
        };
        return format!(
            "{}{}{}{}",
            &frontmatter[..content_end],
            sep,
            token,
            &frontmatter[content_end..]
        );
    }

    let bare_empty = strip_comment(after_ws).trim().is_empty();
    if !bare_empty {
        return frontmatter.to_owned();
    }

    // Block list: the run of `- ` items right below the key.
    let mut last_item: Option<(usize, usize)> = None; // (line index, indent)
    for (j, l) in all.iter().enumerate().skip(idx + 1) {
        let indent = l.text.len() - l.text.trim_start_matches(' ').len();
        let body = l.text[indent..].trim_end();
        let is_item = body == "-" || body.starts_with("- ");
        match last_item {
            _ if body.is_empty() || body.starts_with('#') => continue,
            None if is_item => last_item = Some((j, indent)),
            Some((_, want)) if is_item && indent == want => last_item = Some((j, indent)),
            _ => break,
        }
    }
    let Some((last, indent)) = last_item else {
        // `key:` with nothing under it is a null: give it the list.
        let at = line.start + offset;
        let mut out = frontmatter.to_owned();
        out.insert_str(at, &format!(" [{token}]"));
        return out;
    };
    for l in &all[idx + 1..=last] {
        let body = l.text.trim_start_matches(' ');
        if let Some(item) = body.strip_prefix("- ")
            && unquote_item(strip_comment(item)) == value
        {
            return frontmatter.to_owned();
        }
    }
    let eol = if frontmatter.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let new_line = format!("{}- {}", " ".repeat(indent), token);
    let end = all[last].start + all[last].text.len();
    let mut out = frontmatter.to_owned();
    if frontmatter[end..].is_empty() {
        out.push_str(eol);
        out.push_str(&new_line);
    } else {
        // `end` stops before a `\n` (and a `\r` is inside `text`).
        let ins = end + 1;
        out.insert_str(ins, &format!("{new_line}{eol}"));
    }
    out
}

fn split_flow_items(inner: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut quote: Option<char> = None;
    let mut start = 0;
    let mut prev = '\0';
    for (i, c) in inner.char_indices() {
        match quote {
            Some(q) => {
                if c == q && !(q == '"' && prev == '\\') {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                ',' => {
                    items.push(&inner[start..i]);
                    start = i + 1;
                }
                _ => {}
            },
        }
        prev = c;
    }
    items.push(&inner[start..]);
    items.retain(|s| !s.trim().is_empty());
    items
}

fn append_absent(frontmatter: &str, key: &str, token: &str) -> String {
    let eol = if frontmatter.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let line = format!("{key}: [{token}]");
    if frontmatter.is_empty() {
        return format!("{line}{eol}");
    }
    if frontmatter.ends_with('\n') {
        format!("{frontmatter}{line}{eol}")
    } else {
        format!("{frontmatter}{eol}{line}")
    }
}

fn parse_block(block: &str, which: DriftSide) -> Result<serde_json::Value, FrontmatterDrift> {
    let eol = if block.is_empty() || block.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let doc = format!("---\n{block}{eol}---\n");
    Frontmatter::parse(&doc)
        .map(|(fm, _)| fm.as_json())
        .map_err(|e| FrontmatterDrift::Unparsable {
            which,
            error: e.to_string(),
        })
}

/// Prove that `after` differs from `before` only at `keys`.
///
/// Both are frontmatter blocks without fences. Each goes through
/// [`Frontmatter::parse`]; every top-level value is compared and the
/// first difference outside `keys` (a changed, added or removed key) is
/// reported. Values under `keys` may differ freely, be added or removed.
/// Formatting is not compared, only parsed values.
pub fn frontmatter_changed_only_at(
    before: &str,
    after: &str,
    keys: &[&str],
) -> Result<(), FrontmatterDrift> {
    let b = parse_block(before, DriftSide::Before)?;
    let a = parse_block(after, DriftSide::After)?;
    let (Some(b), Some(a)) = (b.as_object(), a.as_object()) else {
        return Ok(());
    };
    for (k, bv) in b {
        if keys.contains(&k.as_str()) {
            continue;
        }
        match a.get(k) {
            None => return Err(FrontmatterDrift::KeyRemoved(k.clone())),
            Some(av) if av != bv => return Err(FrontmatterDrift::KeyChanged(k.clone())),
            Some(_) => {}
        }
    }
    for k in a.keys() {
        if !keys.contains(&k.as_str()) && !b.contains_key(k) {
            return Err(FrontmatterDrift::KeyAdded(k.clone()));
        }
    }
    Ok(())
}
