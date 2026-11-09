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
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn escape_single(s: &str) -> String {
    s.replace('\'', "''")
}

/// Whether `value` can stand bare, as a plain string, in a YAML scalar or
/// list position: it reads back as a string and holds nothing a plain
/// scalar trips on (`#`, `: `, flow or quote characters, a leading
/// indicator, edge whitespace, a line break).
fn plain_safe(value: &str) -> bool {
    yaml_scalar_is_string(value)
        && value == value.trim()
        && !value.contains([',', '[', ']', '{', '}', '#', '"', '\'', '\n', '\r'])
        && !value.contains(": ")
        && !value.ends_with(':')
        && !value.starts_with(['-', '?', '&', '*', '!', '|', '>', '%', '@', '`'])
}

/// Whether the scalar on `lines[i]` continues onto the next line: the next
/// line that is not blank is indented and is not a `- ` item.
fn continues_on_next_line(all: &[Line<'_>], i: usize) -> bool {
    for l in &all[i + 1..] {
        let body = l.text.trim_end();
        if body.is_empty() {
            continue;
        }
        let indented = body.starts_with([' ', '\t']);
        let trimmed = body.trim_start();
        return indented
            && !trimmed.starts_with('#')
            && trimmed != "-"
            && !trimmed.starts_with("- ");
    }
    false
}

/// One scalar value found under a top-level key.
struct ScalarSite {
    /// Byte range of the scalar's content (inside the quotes when quoted),
    /// relative to the block.
    range: Range<usize>,
    quote: Option<char>,
}

/// Every scalar under top-level `key` that the rewrite can address: the
/// matcher shared by [`rewrite_frontmatter_scalar`] and
/// [`frontmatter_scalar_spans`]. An empty value, a flow collection, a
/// block scalar, an indented key, a key inside a comment, and a bare
/// scalar that continues onto the next line are not sites.
fn scalar_sites(frontmatter: &str, key: &str) -> Vec<ScalarSite> {
    let all = lines(frontmatter);
    let mut out = Vec::new();
    for (i, line) in all.iter().enumerate() {
        let Some(offset) = key_value_offset(line.text, key) else {
            continue;
        };
        let Some(span) = scalar_span(line.text, offset) else {
            continue;
        };
        if span.quote.is_none() && continues_on_next_line(&all, i) {
            continue;
        }
        out.push(ScalarSite {
            range: line.start + span.content.start..line.start + span.content.end,
            quote: span.quote,
        });
    }
    out
}

/// The byte ranges (relative to `frontmatter`, inside any quotes) of every
/// scalar value under top-level `key`: exactly the values
/// [`rewrite_frontmatter_scalar`] considers for a rewrite, found by the same
/// matcher. Lists, flow collections, block scalars and empty values yield
/// nothing.
pub fn frontmatter_scalar_spans(frontmatter: &str, key: &str) -> Vec<Range<usize>> {
    scalar_sites(frontmatter, key)
        .into_iter()
        .map(|s| s.range)
        .collect()
}

/// Rewrite the value of every top-level `key:` line whose scalar is
/// `from`, to `to`, in the same quoting.
///
/// `frontmatter` is the block between the `---` fences (fences not
/// included). The value may be bare, `'single'` or `"double"` quoted,
/// with any whitespace around it and a trailing `# comment`; each
/// edit's `range` covers only the value's bytes (inside the quotes),
/// relative to `frontmatter`. `from` and `to` are given unescaped and
/// escaped to suit the quoting. A bare value is rewritten bare only when
/// `to` stays a plain string there (it reads back as a string and holds
/// no `#`, `: ` or flow/quote character); otherwise `to` is written
/// double-quoted, so the value always reads back as `to`. Nothing else
/// moves: comments, key order, blank lines, other keys, an indented key
/// of the same name, a key named inside a comment, a plain scalar that
/// continues onto the next line (its value is not `from`), and values
/// that are lists or wikilinks are left alone. A key that appears twice
/// at top level is rewritten at both places. (The drift check then
/// reports such a block as unparsable, since the parser rejects duplicate
/// keys.) The drift check does not look at the rewritten key, so this
/// function is what keeps its value a string.
pub fn rewrite_frontmatter_scalar(
    frontmatter: &str,
    key: &str,
    from: &str,
    to: &str,
) -> (String, Vec<AddressEdit>) {
    let mut edits = Vec::new();
    for site in scalar_sites(frontmatter, key) {
        let raw = &frontmatter[site.range.clone()];
        let (matches, replacement) = match site.quote {
            Some('"') => (raw == escape_double(from), escape_double(to)),
            Some(_) => (raw == escape_single(from), escape_single(to)),
            None if raw != from => continue,
            None if plain_safe(to) => (true, to.to_owned()),
            None => (true, format!("\"{}\"", escape_double(to))),
        };
        if matches {
            edits.push(AddressEdit {
                range: site.range,
                replacement,
            });
        }
    }
    (apply(frontmatter, &edits), edits)
}

fn apply(text: &str, edits: &[AddressEdit]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for edit in edits {
        out.push_str(&text[at..edit.range.start]);
        out.push_str(&edit.replacement);
        at = edit.range.end;
    }
    out.push_str(&text[at..]);
    out
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
    if plain_safe(value) {
        value.to_owned()
    } else {
        format!("\"{}\"", escape_double(value))
    }
}

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
/// Returns the new text and the one insertion edit that made it (an empty
/// `range` at the insertion offset in `frontmatter` and the inserted bytes
/// as `replacement`; an `[ ]` list is filled by replacing its blank
/// inside), or no edit when nothing changed. A flow list (`[a, b]`) gets
/// `, value` before the `]` (`[value]` when empty); a block list (`- a`
/// lines) gets a `- value` line after its last item at the same indent
/// (a list of maps is not supported: the result would not parse, and the
/// drift check reports it). An absent key (or `key:` with no value) gets
/// `key: [value]` as the block's last line, keeping the block's
/// line-ending and trailing-newline convention (an empty block gains one
/// newline). `value` is written bare when that is safe and double-quoted
/// otherwise. When `value` is already an item nothing changes, and
/// nothing changes either when `key` holds a scalar (including `null`) or
/// anything else that is not a list: a list is never conjured by
/// overwriting a value. Only the first top-level `key:` is considered.
pub fn append_frontmatter_list_value(
    frontmatter: &str,
    key: &str,
    value: &str,
) -> (String, Vec<AddressEdit>) {
    match list_append_edit(frontmatter, key, value) {
        Some(edit) => (apply(frontmatter, std::slice::from_ref(&edit)), vec![edit]),
        None => (frontmatter.to_owned(), Vec::new()),
    }
}

fn insertion(at: usize, text: String) -> Option<AddressEdit> {
    Some(AddressEdit {
        range: at..at,
        replacement: text,
    })
}

fn list_append_edit(frontmatter: &str, key: &str, value: &str) -> Option<AddressEdit> {
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
        let open = value_start;
        let close = flow_close(frontmatter, open)?;
        let inner = &frontmatter[open + 1..close];
        if inner.trim().is_empty() {
            return Some(AddressEdit {
                range: open + 1..close,
                replacement: token,
            });
        }
        if split_flow_items(inner)
            .iter()
            .any(|i| unquote_item(i) == value)
        {
            return None;
        }
        let content_end = open + 1 + inner.trim_end().len();
        let sep = if inner.trim_end().ends_with(',') {
            " "
        } else {
            ", "
        };
        return insertion(content_end, format!("{sep}{token}"));
    }

    if !strip_comment(after_ws).trim().is_empty() {
        return None;
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
        return insertion(line.start + offset, format!(" [{token}]"));
    };
    for l in &all[idx + 1..=last] {
        let body = l.text.trim_start_matches(' ');
        if let Some(item) = body.strip_prefix("- ")
            && unquote_item(strip_comment(item)) == value
        {
            return None;
        }
    }
    let eol = if frontmatter.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let new_line = format!("{}- {}", " ".repeat(indent), token);
    let end = all[last].start + all[last].text.len();
    if frontmatter[end..].is_empty() {
        insertion(end, format!("{eol}{new_line}"))
    } else {
        // `end` stops before a `\n` (and a `\r` is inside `text`).
        insertion(end + 1, format!("{new_line}{eol}"))
    }
}

/// The closing `]` of the flow list opening at `open`, which may span
/// lines, found outside quotes.
fn flow_close(frontmatter: &str, open: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
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
                ']' => return Some(open + 1 + i),
                _ => {}
            },
        }
        prev = c;
    }
    None
}

/// Remove `value` from the list under top-level `key`, keeping everything
/// else: the counterpart of [`append_frontmatter_list_value`], with the
/// same shape.
///
/// `frontmatter` is the block between the fences. Returns the new text and
/// the one edit that made it (a range of `frontmatter` and its
/// replacement), or no edit when nothing changed. In a flow list
/// (`[a, b]`) the item goes with one separator, so `[a, value, b]` becomes
/// `[a, b]` and `[value]` becomes `[]`; in a block list (`- a` lines) its
/// line goes, and removing the only item leaves `key:` with no value. Only
/// the first item equal to `value` (quoted or bare) of the first top-level
/// `key:` is removed. Nothing changes when `value` is not an item, when
/// `key` is absent, or when it holds anything that is not a list.
pub fn remove_frontmatter_list_value(
    frontmatter: &str,
    key: &str,
    value: &str,
) -> (String, Vec<AddressEdit>) {
    match list_remove_edit(frontmatter, key, value) {
        Some(edit) => (apply(frontmatter, std::slice::from_ref(&edit)), vec![edit]),
        None => (frontmatter.to_owned(), Vec::new()),
    }
}

fn list_remove_edit(frontmatter: &str, key: &str, value: &str) -> Option<AddressEdit> {
    let all = lines(frontmatter);
    let (idx, offset) = all
        .iter()
        .enumerate()
        .find_map(|(i, l)| key_value_offset(l.text, key).map(|o| (i, o)))?;
    let line = &all[idx];
    let rest = &line.text[offset..];
    let after_ws = rest.trim_start_matches([' ', '\t']);
    let value_start = line.start + offset + (rest.len() - after_ws.len());
    let removal = |range: Range<usize>| {
        Some(AddressEdit {
            range,
            replacement: String::new(),
        })
    };

    if after_ws.starts_with('[') {
        let open = value_start;
        let close = flow_close(frontmatter, open)?;
        let inner_start = open + 1;
        let inner = &frontmatter[inner_start..close];
        // Each item's trimmed range, in `frontmatter`'s bytes.
        let items: Vec<Range<usize>> = split_flow_items(inner)
            .into_iter()
            .map(|item| {
                let at = inner_start + (item.as_ptr() as usize - inner.as_ptr() as usize);
                let lead = item.len() - item.trim_start().len();
                at + lead..at + lead + item.trim().len()
            })
            .collect();
        let i = items
            .iter()
            .position(|r| unquote_item(&frontmatter[r.clone()]) == value)?;
        return match (items.len(), i + 1 < items.len()) {
            (1, _) => removal(inner_start..close),
            // Not the last: the item, its comma and the space before the
            // next item.
            (_, true) => removal(items[i].start..items[i + 1].start),
            // The last: from the end of the item before it.
            (_, false) => removal(items[i - 1].end..items[i].end),
        };
    }

    if !strip_comment(after_ws).trim().is_empty() {
        return None;
    }

    // Block list: the run of `- ` items right below the key.
    let mut want: Option<usize> = None;
    for l in all.iter().skip(idx + 1) {
        let indent = l.text.len() - l.text.trim_start_matches(' ').len();
        let body = l.text[indent..].trim_end();
        let is_item = body == "-" || body.starts_with("- ");
        if body.is_empty() || body.starts_with('#') {
            continue;
        }
        if !is_item || want.is_some_and(|w| w != indent) {
            break;
        }
        want = Some(indent);
        if let Some(item) = body.strip_prefix("- ")
            && unquote_item(strip_comment(item)) == value
        {
            let end = l.start + l.text.len();
            return if end < frontmatter.len() {
                // The line and its `\n`.
                removal(l.start..end + 1)
            } else {
                // The block's last line, with no line break after it: take
                // the one before it instead.
                removal(
                    all[idx..]
                        .iter()
                        .take_while(|p| p.start < l.start)
                        .last()
                        .map_or(l.start, |p| p.start + p.text.len())..end,
                )
            };
        }
    }
    None
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

fn append_absent(frontmatter: &str, key: &str, token: &str) -> Option<AddressEdit> {
    let eol = if frontmatter.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let line = format!("{key}: [{token}]");
    if frontmatter.is_empty() || frontmatter.ends_with('\n') {
        insertion(frontmatter.len(), format!("{line}{eol}"))
    } else {
        insertion(frontmatter.len(), format!("{eol}{line}"))
    }
}

fn parse_block(block: &str, which: DriftSide) -> Result<serde_json::Value, FrontmatterDrift> {
    let eol = if block.is_empty() || block.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let doc = format!("---\n{block}{eol}---\n");
    // YAML on purpose: `doc` is built here as a Markdown frontmatter block.
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
    values_changed_only_at(&b, &a, keys)
}

/// The comparison behind [`frontmatter_changed_only_at`], over values already read: a Typst
/// metadata block is compared with it too. Anything but two objects compares clean.
pub fn values_changed_only_at(
    before: &serde_json::Value,
    after: &serde_json::Value,
    keys: &[&str],
) -> Result<(), FrontmatterDrift> {
    let (Some(b), Some(a)) = (before.as_object(), after.as_object()) else {
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
