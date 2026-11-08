//! The metadata block reader (RFC 0008 §5.3): a tree walk, with nothing evaluated.

use std::ops::Range;

use serde_json::{Map, Value};
use typst_syntax::ast::{Expr, FuncCall};
use typst_syntax::{LinkedNode, SyntaxKind, SyntaxNode};

use super::{CDNO_LABEL, first_error_offset, line_of, with_tree};

/// A note's `#metadata((…)) <cdno>` block.
#[derive(Debug, Clone, PartialEq)]
pub struct MetadataBlock {
    /// From the `#` of `#metadata` to the `>` of `<cdno>`, inclusive.
    pub span: Range<usize>,
    /// The dictionary as JSON: strings, arrays of strings, booleans.
    pub value: Value,
    /// Each key's spans in the source, in source order.
    pub entries: Vec<EntrySpan>,
    /// The offset of the dictionary's closing parenthesis, where a new entry would go.
    pub insert_at: usize,
}

/// Where one key and its value sit in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrySpan {
    pub key: String,
    pub key_span: Range<usize>,
    pub value_span: Range<usize>,
}

/// Why a source has no readable metadata block. Lines are 1-based.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TypstError {
    #[error("no `#metadata((…)) <cdno>` block at the top level")]
    NoBlock,
    #[error("line {line}: `#metadata` has a `type` key but is not labelled `<cdno>`")]
    LabelBroken { line: usize },
    #[error("line {line}: the `<cdno>` block is not at the top level of the document")]
    Nested { line: usize },
    #[error("line {line}: only whitespace and comments may precede the `<cdno>` block")]
    NotFirst { line: usize },
    #[error("line {line}: a second `<cdno>` block")]
    Duplicate { line: usize },
    #[error("line {line}: a metadata value must be a literal, found {what}")]
    NotLiteral { line: usize, what: String },
    #[error("line {line}: the argument of the `<cdno>` block is not a single dictionary")]
    NotADictionary { line: usize },
    /// The parser could not run, or its offsets do not match the source.
    #[error("line {line}: {message}")]
    Fault { line: usize, message: String },
}

/// Read the source's metadata block, or say by name and line why there is none.
pub fn metadata_block(source: &str) -> Result<MetadataBlock, TypstError> {
    with_tree(source, |root| read_block(source, root)).map_err(|fault| TypstError::Fault {
        line: fault.line,
        message: fault.message,
    })?
}

fn read_block(source: &str, root: &SyntaxNode) -> Result<MetadataBlock, TypstError> {
    let root = LinkedNode::new(root);
    let calls = metadata_calls(&root);
    let line = |node: &LinkedNode| line_of(source, node.offset());

    let labelled: Vec<_> = calls
        .iter()
        .filter_map(|c| Some((c, label_of(c)?)))
        .collect();
    let Some((first, _)) = labelled.first() else {
        return match calls.iter().find(|c| is_broken_block(c)) {
            Some(call) => Err(TypstError::LabelBroken { line: line(call) }),
            None => Err(TypstError::NoBlock),
        };
    };
    let Some((call, label)) = labelled.iter().find(|(c, _)| is_top_level(c)) else {
        return Err(TypstError::Nested { line: line(first) });
    };
    let Some(hash) = call.prev_sibling().filter(|n| n.kind() == SyntaxKind::Hash) else {
        return Err(fault(source, call, "a top-level call without a `#`"));
    };

    let preceded = root
        .children()
        .take_while(|n| n.offset() < hash.offset())
        .any(|n| !is_blank(n.kind()));
    if preceded {
        return Err(TypstError::NotFirst { line: line(call) });
    }
    if let Some((second, _)) = labelled.get(1) {
        return Err(TypstError::Duplicate { line: line(second) });
    }

    let span = checked(source, &hash)?.start..checked(source, label)?.end;
    let dict = single_dictionary(source, call)?;
    let (value, entries) = read_dictionary(source, &dict)?;
    let closing = dict
        .children()
        .rfind(|n| n.kind() == SyntaxKind::RightParen)
        .ok_or_else(|| fault(source, &dict, "a dictionary with no closing parenthesis"))?;
    Ok(MetadataBlock {
        span,
        value,
        entries,
        insert_at: checked(source, &closing)?.start,
    })
}

/// Every call of `metadata` by name, in document order. Iterative, as a deep tree would overflow
/// a recursive walk's frames.
fn metadata_calls<'a>(root: &LinkedNode<'a>) -> Vec<LinkedNode<'a>> {
    let mut calls = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if node.kind() == SyntaxKind::FuncCall && callee_is_metadata(&node) {
            calls.push(node.clone());
        }
        stack.extend(node.children().rev());
    }
    calls
}

fn callee_is_metadata(call: &LinkedNode) -> bool {
    call.cast::<FuncCall>()
        .is_some_and(|c| matches!(c.callee(), Expr::Ident(i) if i.as_str() == "metadata"))
}

fn label_of<'a>(call: &LinkedNode<'a>) -> Option<LinkedNode<'a>> {
    call.next_sibling()
        .filter(|n| n.kind() == SyntaxKind::Label && n.leaf_text() == CDNO_LABEL)
}

fn is_top_level(node: &LinkedNode) -> bool {
    node.parent().is_some_and(|p| p.parent().is_none())
}

/// §5.3's "whitespace and comments": the trivia kinds without Shebang.
fn is_blank(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Space
            | SyntaxKind::Parbreak
            | SyntaxKind::LineComment
            | SyntaxKind::BlockComment
    )
}

/// A `#metadata((…))` written as a block, with a `type` key: it was meant to be the note's.
fn is_broken_block(call: &LinkedNode) -> bool {
    let hashed = call
        .prev_sibling()
        .is_some_and(|n| n.kind() == SyntaxKind::Hash);
    let Some(args) = call.children().find(|n| n.kind() == SyntaxKind::Args) else {
        return false;
    };
    let Some(dict) = args.children().find(|n| n.kind() == SyntaxKind::Dict) else {
        return false;
    };
    hashed
        && dict
            .children()
            .filter_map(|entry| key_and_value(&entry).map(|(k, _)| k))
            .any(|key| key_text(&key).as_deref() == Some("type"))
}

/// The argument node of a call whose sole argument is a dictionary.
fn single_dictionary<'a>(
    source: &str,
    call: &LinkedNode<'a>,
) -> Result<LinkedNode<'a>, TypstError> {
    let not_a_dictionary = || TypstError::NotADictionary {
        line: line_of(source, call.offset()),
    };
    let args = call
        .children()
        .find(|n| n.kind() == SyntaxKind::Args)
        .ok_or_else(not_a_dictionary)?;
    if let Some(at) = first_error_offset(args.get(), args.offset()) {
        return Err(TypstError::NotLiteral {
            line: line_of(source, at),
            what: "a syntax error".to_string(),
        });
    }
    let mut items = args.children().filter(|n| is_item(n.kind()));
    match (items.next(), items.next()) {
        (Some(dict), None) if dict.kind() == SyntaxKind::Dict => Ok(dict),
        _ => Err(not_a_dictionary()),
    }
}

fn is_item(kind: SyntaxKind) -> bool {
    !kind.is_trivia()
        && !matches!(
            kind,
            SyntaxKind::LeftParen | SyntaxKind::RightParen | SyntaxKind::Comma
        )
}

fn read_dictionary(source: &str, dict: &LinkedNode) -> Result<(Value, Vec<EntrySpan>), TypstError> {
    let mut map = Map::new();
    let mut entries = Vec::new();
    for entry in dict.children().filter(|n| is_item(n.kind())) {
        if entry.kind() == SyntaxKind::Colon {
            continue;
        }
        let Some((key_node, value_node)) = key_and_value(&entry) else {
            return Err(not_literal(source, &entry));
        };
        let Some(key) = key_text(&key_node) else {
            return Err(not_literal(source, &key_node));
        };
        map.insert(key.clone(), read_value(source, &value_node)?);
        entries.push(EntrySpan {
            key,
            key_span: checked(source, &key_node)?,
            value_span: checked(source, &value_node)?,
        });
    }
    Ok((Value::Object(map), entries))
}

/// The key and value nodes of a `Named` or `Keyed` entry; `None` for a spread.
fn key_and_value<'a>(entry: &LinkedNode<'a>) -> Option<(LinkedNode<'a>, LinkedNode<'a>)> {
    if !matches!(entry.kind(), SyntaxKind::Named | SyntaxKind::Keyed) {
        return None;
    }
    let mut parts = entry
        .children()
        .filter(|n| !n.kind().is_trivia() && n.kind() != SyntaxKind::Colon);
    Some((parts.next()?, parts.next()?))
}

/// An identifier's name or a string's decoded value; `None` for any other key expression.
fn key_text(key: &LinkedNode) -> Option<String> {
    match key.cast::<Expr>()? {
        Expr::Ident(ident) => Some(ident.as_str().to_string()),
        Expr::Str(string) => Some(string.get().to_string()),
        _ => None,
    }
}

fn read_value(source: &str, node: &LinkedNode) -> Result<Value, TypstError> {
    match node.cast::<Expr>() {
        Some(Expr::Str(string)) => Ok(Value::String(string.get().to_string())),
        Some(Expr::Bool(boolean)) => Ok(Value::Bool(boolean.get())),
        Some(Expr::Array(_)) => node
            .children()
            .filter(|n| is_item(n.kind()))
            .map(|item| match item.cast::<Expr>() {
                Some(Expr::Str(string)) => Ok(Value::String(string.get().to_string())),
                _ => Err(not_literal(source, &item)),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        // `("x")` is what a hand-written one-element array usually looks like (§5.3).
        Some(Expr::Parenthesized(inner)) => match inner.expr() {
            Expr::Str(string) => Ok(Value::Array(vec![Value::String(string.get().to_string())])),
            _ => Err(not_literal(source, node)),
        },
        _ => Err(not_literal(source, node)),
    }
}

fn not_literal(source: &str, node: &LinkedNode) -> TypstError {
    TypstError::NotLiteral {
        line: line_of(source, node.offset()),
        what: node.kind().name().to_string(),
    }
}

/// The node's range, once the source is shown to hold the node's text there.
fn checked(source: &str, node: &LinkedNode) -> Result<Range<usize>, TypstError> {
    let range = node.range();
    let text = node.get().full_text();
    if source.get(range.clone()) == Some(text.as_str()) {
        Ok(range)
    } else {
        Err(fault(source, node, "tree offsets do not match the source"))
    }
}

fn fault(source: &str, node: &LinkedNode, message: &str) -> TypstError {
    TypstError::Fault {
        line: line_of(source, node.offset()),
        message: message.to_string(),
    }
}
