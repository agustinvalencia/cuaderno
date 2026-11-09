//! Headings and wikilink spans over Typst markup (RFC 0008 §5.5).
//!
//! Everything here reads the parsed tree. Tree offsets can drift from the source on odd inputs, so
//! a leaf's bytes are taken from the source only when they equal the leaf's own text.

use typst_syntax::ast;
use typst_syntax::{Lines, SyntaxKind, SyntaxNode, is_newline};

use super::{CDNO_LABEL, line_in, with_tree};
use crate::extractors::{self, WikilinkSpan};

const UNREADABLE: &str = "unreadable source range";
const END_OF_RUN: &str = "end of run";

/// A `[[` that is not a link: its interior is not one inline run, it spans a line, or its run holds
/// no `]]`. `leaf` names what broke it, a syntax kind such as `Hash` where there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkFault {
    pub open: usize,
    pub line: usize,
    pub leaf: String,
}

/// The links of one Typst source, in source order. `spans`, `addresses`, `targets`, `labels` and
/// `in_block` run in parallel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypstLinks {
    /// Raw byte ranges, escapes included: `target` holds the backslash of an escaped anchor and
    /// `address` ends before it.
    pub spans: Vec<WikilinkSpan>,
    /// The decoded address, no anchor.
    pub addresses: Vec<String>,
    /// The decoded target with its anchor, as the resolver sees it.
    pub targets: Vec<String>,
    /// The decoded label after a `|`; `None` when there is none or it is empty.
    pub labels: Vec<Option<String>>,
    /// Whether the link sits in a `<cdno>` block string, which escapes by string rules, not
    /// markup rules.
    pub in_block: Vec<bool>,
    pub faults: Vec<LinkFault>,
}

/// `(level, source_text)` per `=`-heading, in source order and at any nesting: the bytes after the
/// `=` run and one space, a trailing `<label>` excluded, Escape leaves decoded, trimmed and
/// whitespace collapsed. A heading whose bytes cannot be read from the source is left out.
pub fn headings(source: &str) -> Vec<(u8, String)> {
    with_tree(source, |root| {
        let mut out = Vec::new();
        collect_headings(source, root, 0, &mut out);
        out
    })
    .unwrap_or_default()
}

/// Every wikilink in the markup and in the string values of the `<cdno>` block, in source order,
/// and every `[[` in the markup that is not a link. A parser that cannot start is one fault.
pub fn wikilink_spans(source: &str) -> TypstLinks {
    let scanned = with_tree(source, |root| {
        let mut scan = Scan {
            source,
            found: Vec::new(),
            faults: Vec::new(),
        };
        scan.markup(root, 0, true);
        scan.finish()
    });
    let mut links = match scanned {
        Ok(links) => links,
        Err(fault) => {
            return TypstLinks {
                faults: vec![LinkFault {
                    open: 0,
                    line: fault.line,
                    leaf: fault.message,
                }],
                ..TypstLinks::default()
            };
        }
    };
    if !links.faults.is_empty() {
        let lines = Lines::new(source);
        for fault in &mut links.faults {
            fault.line = line_in(&lines, fault.open);
        }
    }
    links
}

/// `address` written so that a scan reads it back as the same address: with the string escapes of
/// a `<cdno>` block when `in_block`, otherwise with a backslash before each Typst markup
/// character (§5.5). Over-escaping is harmless, so the set errs wide; a `/` is escaped only
/// where it would start `//` or `/*`.
pub fn escape_link_address(address: &str, in_block: bool) -> String {
    let mut out = String::with_capacity(address.len());
    let mut chars = address.chars().peekable();
    while let Some(c) = chars.next() {
        if in_block {
            match c {
                '\\' | '"' => {
                    out.push('\\');
                    out.push(c);
                }
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                _ => out.push(c),
            }
            continue;
        }
        let markup = matches!(
            c,
            '\\' | '#' | '_' | '*' | '$' | '`' | '<' | '>' | '@' | '~' | '[' | ']'
        );
        let comment = c == '/' && matches!(chars.peek(), Some('/' | '*'));
        if markup || comment {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn collect_headings(source: &str, node: &SyntaxNode, start: usize, out: &mut Vec<(u8, String)>) {
    if matches!(node.kind(), SyntaxKind::Raw | SyntaxKind::Equation) {
        return;
    }
    if node.kind() == SyntaxKind::Heading
        && let Some(heading) = read_heading(source, node, start)
    {
        out.push(heading);
    }
    let mut at = start;
    for child in node.children() {
        collect_headings(source, child, at, out);
        at += child.len();
    }
}

fn read_heading(source: &str, heading: &SyntaxNode, start: usize) -> Option<(u8, String)> {
    let mut level = 0;
    let mut text = String::new();
    let mut at = start;
    for child in heading.children() {
        match child.kind() {
            SyntaxKind::HeadingMarker => level = child.len().min(usize::from(u8::MAX)) as u8,
            SyntaxKind::Markup => source_text(source, child, at, &mut text)?,
            _ => {}
        }
        at += child.len();
    }
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Some((level, collapsed))
}

fn source_text(source: &str, node: &SyntaxNode, start: usize, out: &mut String) -> Option<()> {
    if node.children().next().is_some() {
        let mut at = start;
        for child in node.children() {
            source_text(source, child, at, out)?;
            at += child.len();
        }
        return Some(());
    }
    let text = read_leaf(source, node, start)?;
    match node.cast::<ast::Escape>() {
        Some(escape) => out.push(escape.get()),
        None => out.push_str(text),
    }
    Some(())
}

fn read_leaf<'s>(source: &'s str, node: &SyntaxNode, start: usize) -> Option<&'s str> {
    source
        .get(start..start + node.len())
        .filter(|text| *text == node.leaf_text().as_str())
}

/// One leaf of an inline run. `text` is `None` when the source does not hold the leaf's bytes.
struct Leaf<'t, 's> {
    node: &'t SyntaxNode,
    start: usize,
    text: Option<&'s str>,
}

impl Leaf<'_, '_> {
    fn is_bracket(&self, bracket: &str) -> bool {
        self.node.kind() == SyntaxKind::Text && self.node.leaf_text() == bracket
    }

    fn decoded(&self) -> Option<char> {
        self.node.cast::<ast::Escape>().map(ast::Escape::get)
    }

    fn holds_newline(&self) -> bool {
        match self.decoded() {
            Some(c) => is_newline(c),
            None => self.node.leaf_text().chars().any(is_newline),
        }
    }
}

/// One decoded character of a link's interior and the raw bytes it came from.
struct Piece {
    ch: char,
    start: usize,
    end: usize,
}

struct Found {
    span: WikilinkSpan,
    address: String,
    target: String,
    label: Option<String>,
    in_block: bool,
}

struct Scan<'s> {
    source: &'s str,
    found: Vec<Found>,
    faults: Vec<LinkFault>,
}

fn is_inline(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Text
            | SyntaxKind::Escape
            | SyntaxKind::Shorthand
            | SyntaxKind::SmartQuote
            | SyntaxKind::Space
    )
}

fn is_cdno_block(call: &SyntaxNode, siblings: &[&SyntaxNode]) -> bool {
    let is_metadata = call.cast::<ast::FuncCall>().is_some_and(
        |call| matches!(call.callee(), ast::Expr::Ident(ident) if ident.as_str() == "metadata"),
    );
    is_metadata
        && siblings
            .iter()
            .find(|sibling| !sibling.kind().is_trivia())
            .is_some_and(|next| next.kind() == SyntaxKind::Label && next.leaf_text() == CDNO_LABEL)
}

impl<'s> Scan<'s> {
    /// A block's strings are read before its call is walked, so a content block among its
    /// values would otherwise come out of order.
    fn finish(mut self) -> TypstLinks {
        self.found.sort_by_key(|found| found.span.open);
        self.faults.sort_by_key(|fault| fault.open);
        let mut links = TypstLinks {
            faults: self.faults,
            ..TypstLinks::default()
        };
        for found in self.found {
            links.spans.push(found.span);
            links.addresses.push(found.address);
            links.targets.push(found.target);
            links.labels.push(found.label);
            links.in_block.push(found.in_block);
        }
        links
    }

    fn walk(&mut self, node: &SyntaxNode, start: usize) {
        match node.kind() {
            SyntaxKind::Raw | SyntaxKind::Equation => {}
            SyntaxKind::Markup => self.markup(node, start, false),
            _ => {
                let mut at = start;
                for child in node.children() {
                    self.walk(child, at);
                    at += child.len();
                }
            }
        }
    }

    fn markup(&mut self, node: &SyntaxNode, start: usize, top_level: bool) {
        let children: Vec<&SyntaxNode> = node.children().collect();
        let mut run: Vec<Leaf<'_, 's>> = Vec::new();
        let mut at = start;
        for (i, child) in children.iter().copied().enumerate() {
            if is_inline(child.kind()) {
                run.push(Leaf {
                    node: child,
                    start: at,
                    text: read_leaf(self.source, child, at),
                });
            } else {
                self.run(&run, Some(child.kind()));
                run.clear();
                if top_level
                    && child.kind() == SyntaxKind::FuncCall
                    && is_cdno_block(child, &children[i + 1..])
                {
                    self.block_arguments(child, at);
                }
                self.walk(child, at);
            }
            at += child.len();
        }
        self.run(&run, None);
    }

    fn fault(&mut self, open: usize, leaf: String) {
        // The line is filled in at the end, from one line table for all the faults.
        self.faults.push(LinkFault {
            open,
            line: 0,
            leaf,
        });
    }

    fn run(&mut self, run: &[Leaf<'_, 's>], breaker: Option<SyntaxKind>) {
        let n = run.len();
        if n < 2 {
            return;
        }
        // The nearest `]]` and line break at or after each leaf, so that a run of many stray
        // openings stays linear.
        let mut next_close = vec![None; n + 1];
        let mut next_newline = vec![None; n + 1];
        for i in (0..n).rev() {
            let closes =
                run[i].is_bracket("]") && run.get(i + 1).is_some_and(|l| l.is_bracket("]"));
            next_close[i] = if closes { Some(i) } else { next_close[i + 1] };
            next_newline[i] = if run[i].holds_newline() {
                Some(i)
            } else {
                next_newline[i + 1]
            };
        }

        let mut i = 0;
        while i + 1 < n {
            if !(run[i].is_bracket("[") && run[i + 1].is_bracket("[")) {
                i += 1;
                continue;
            }
            let open = run[i].start;
            let Some(close) = next_close[i + 2] else {
                let leaf = breaker.map_or_else(|| END_OF_RUN.to_string(), |k| format!("{k:?}"));
                self.fault(open, leaf);
                i += 2;
                continue;
            };
            // A line-broken opening consumes nothing past itself: a stray `[[` must not hide a
            // whole link on a later line from a rename.
            if let Some(newline) = next_newline[i + 2].filter(|&at| at < close) {
                self.fault(open, format!("{:?}", run[newline].node.kind()));
                i += 2;
                continue;
            }
            let is_embed = i > 0
                && run[i - 1].node.kind() == SyntaxKind::Text
                && run[i - 1].node.leaf_text().ends_with('!');
            self.link(run, i, close, is_embed);
            i = close + 2;
        }
    }

    fn link(&mut self, run: &[Leaf<'_, 's>], opening: usize, close: usize, is_embed: bool) {
        let open = run[opening].start;
        if run[opening..close + 2].iter().any(|l| l.text.is_none()) {
            self.fault(open, UNREADABLE.to_string());
            return;
        }
        let after_open = run[opening + 1].start + 1;
        let inner_end = run[close].start;

        let mut pieces = Vec::new();
        for leaf in &run[opening + 2..close] {
            match leaf.decoded() {
                Some(ch) => pieces.push(Piece {
                    ch,
                    start: leaf.start,
                    end: leaf.start + leaf.node.len(),
                }),
                None => pieces.extend(leaf.text.unwrap_or_default().char_indices().map(
                    |(at, ch)| Piece {
                        ch,
                        start: leaf.start + at,
                        end: leaf.start + at + ch.len_utf8(),
                    },
                )),
            }
        }

        let target_end = pieces
            .iter()
            .position(|p| p.ch == '|')
            .unwrap_or(pieces.len());
        let target_part = trimmed(&pieces[..target_end]);
        let Some(first) = target_part.first() else {
            return;
        };
        let target = first.start..target_part.last().map_or(first.end, |p| p.end);

        let address_end = pieces[..target_end]
            .iter()
            .position(|p| p.ch == '#')
            .unwrap_or(target_end);
        let address_part = trimmed(&pieces[..address_end]);
        let address = match (address_part.first(), address_part.last()) {
            (Some(first), Some(last)) => first.start..last.end,
            _ => after_open..after_open,
        };

        let label: String = pieces
            .get(target_end + 1..)
            .map_or(&[][..], trimmed)
            .iter()
            .map(|p| p.ch)
            .collect();

        self.found.push(Found {
            span: WikilinkSpan {
                open,
                address,
                target,
                inner_end,
                is_embed,
            },
            address: address_part.iter().map(|p| p.ch).collect(),
            target: target_part.iter().map(|p| p.ch).collect(),
            label: Some(label).filter(|l| !l.is_empty()),
            in_block: false,
        });
    }

    fn block_arguments(&mut self, call: &SyntaxNode, start: usize) {
        let mut at = start;
        for child in call.children() {
            if child.kind() == SyntaxKind::Args {
                let mut arg_at = at;
                for arg in child.children() {
                    if arg.kind() == SyntaxKind::Dict {
                        self.block_value(arg, arg_at);
                    }
                    arg_at += arg.len();
                }
            }
            at += child.len();
        }
    }

    fn block_value(&mut self, node: &SyntaxNode, start: usize) {
        match node.kind() {
            SyntaxKind::Str => self.block_string(node, start),
            SyntaxKind::Named | SyntaxKind::Keyed => {
                // The key sits before the colon and is not a value.
                let mut past_colon = false;
                let mut at = start;
                for child in node.children() {
                    if past_colon {
                        self.block_value(child, at);
                    }
                    past_colon |= child.kind() == SyntaxKind::Colon;
                    at += child.len();
                }
            }
            SyntaxKind::Dict | SyntaxKind::Array | SyntaxKind::Parenthesized => {
                let mut at = start;
                for child in node.children() {
                    self.block_value(child, at);
                    at += child.len();
                }
            }
            _ => {}
        }
    }

    /// A block string holds no markup, so it is scanned as its decoded value, the way the
    /// Markdown scan reads a YAML string, and the spans are carried back to the raw bytes.
    fn block_string(&mut self, node: &SyntaxNode, start: usize) {
        let decoded = read_leaf(self.source, node, start)
            .zip(node.cast::<ast::Str>())
            .and_then(|(raw, parsed)| {
                let (value, map) = decode_string(raw, start)?;
                (value == parsed.get().as_str()).then_some((value, map))
            });
        let Some((value, map)) = decoded else {
            self.fault(start, format!("{:?}", SyntaxKind::Str));
            return;
        };
        for span in extractors::wikilink_spans(&value) {
            let raw_range = |range: &std::ops::Range<usize>| {
                if range.is_empty() {
                    map[range.start].0..map[range.start].0
                } else {
                    map[range.start].0..map[range.end - 1].1
                }
            };
            let label = value[span.open + 2..span.inner_end]
                .split_once('|')
                .map(|(_, label)| label.trim().to_string())
                .filter(|label| !label.is_empty());
            self.found.push(Found {
                span: WikilinkSpan {
                    open: map[span.open].0,
                    address: raw_range(&span.address),
                    target: raw_range(&span.target),
                    inner_end: map[span.inner_end].0,
                    is_embed: span.is_embed,
                },
                address: value[span.address].to_string(),
                target: value[span.target].to_string(),
                label,
                in_block: true,
            });
        }
    }
}

fn trimmed(pieces: &[Piece]) -> &[Piece] {
    let Some(first) = pieces.iter().position(|p| !p.ch.is_whitespace()) else {
        return &[];
    };
    let last = pieces.iter().rposition(|p| !p.ch.is_whitespace());
    &pieces[first..=last.unwrap_or(first)]
}

/// Decodes a quoted string leaf. Beside the value it returns, per decoded byte, the raw bytes of
/// the character that byte belongs to. `start` is the offset of the opening quote. `None` when
/// the leaf is not quoted. Like `ast::Str::get`, a `\u{}` that names no character stays literal;
/// a value that differs from the parser's own is refused by the caller.
fn decode_string(raw: &str, start: usize) -> Option<(String, Vec<(usize, usize)>)> {
    let body = raw.strip_prefix('"')?.strip_suffix('"')?;
    let base = start + 1;
    let mut value = String::with_capacity(body.len());
    let mut map = Vec::with_capacity(body.len());
    let mut chars = body.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let mut decoded = c;
        let mut end = at + c.len_utf8();
        if c == '\\' {
            let simple = match chars.peek().map(|&(_, next)| next) {
                Some('\\') => Some('\\'),
                Some('"') => Some('"'),
                Some('n') => Some('\n'),
                Some('r') => Some('\r'),
                Some('t') => Some('\t'),
                _ => None,
            };
            if let Some(escaped) = simple {
                chars.next();
                decoded = escaped;
                end = at + 2;
            } else if body[at..].starts_with("\\u{") {
                let hex_start = at + 3;
                let hex_len = body[hex_start..]
                    .find(|ch: char| !ch.is_ascii_hexdigit())
                    .unwrap_or(body.len() - hex_start);
                let named = u32::from_str_radix(&body[hex_start..hex_start + hex_len], 16)
                    .ok()
                    .and_then(char::from_u32);
                if let Some(named) = named {
                    decoded = named;
                    end = hex_start + hex_len;
                    if body[end..].starts_with('}') {
                        end += 1;
                    }
                    while chars.peek().is_some_and(|&(next, _)| next < end) {
                        chars.next();
                    }
                }
            }
        }
        value.push(decoded);
        map.extend(std::iter::repeat_n(
            (base + at, base + end),
            decoded.len_utf8(),
        ));
    }
    Some((value, map))
}
