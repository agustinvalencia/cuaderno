//! The files a Typst document reaches for (RFC 0008 §5.7, T9).

use std::ops::Range;

use typst_syntax::ast::{self, Arg, ArrayItem, AstNode, Expr};
use typst_syntax::{LinkedNode, SyntaxKind, SyntaxNode};

use super::{line_of, with_tree};

/// Calls whose first positional argument is a file path.
const PATH_CALLS: &[&str] = &[
    "image",
    "read",
    "json",
    "csv",
    "yaml",
    "toml",
    "xml",
    "cbor",
    "bibliography",
];

/// A path written as a string literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathLiteral {
    /// Byte range of the string's content, inside the quotes.
    pub span: Range<usize>,
    /// The decoded value.
    pub value: String,
    /// The call it belongs to: `image`, `bibliography`, `include` or `import`.
    pub call: String,
    /// 1-based line.
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathArg {
    Literal(PathLiteral),
    /// A path argument that is not a string literal, so its value cannot be known.
    NotLiteral {
        call: String,
        line: usize,
    },
}

/// Every path argument of `image`, `read`, `json`, `csv`, `yaml`, `toml`, `xml`, `cbor`,
/// `bibliography`, `include` and a path `import`, in document order. `@preview` imports and URLs
/// are not returned. `bibliography` yields one entry per item of an array. An `import` whose
/// source is a name or a field access (a module already in scope) is not a path and is skipped.
/// If the parser cannot run, one `NotLiteral` is returned rather than an empty list, so a caller
/// refuses instead of seeing a document with no assets.
pub fn path_args(source: &str) -> Vec<PathArg> {
    let walked = with_tree(source, |root| {
        let mut out = Vec::new();
        walk(source, &LinkedNode::new(root), &mut out);
        out
    });
    walked.unwrap_or_else(|_| {
        vec![PathArg::NotLiteral {
            call: "parser".to_string(),
            line: 1,
        }]
    })
}

fn walk(source: &str, node: &LinkedNode, out: &mut Vec<PathArg>) {
    match node.kind() {
        SyntaxKind::FuncCall => {
            if let Some(call) = node.cast::<ast::FuncCall>() {
                func_call(source, call, node, out);
            }
        }
        SyntaxKind::ModuleInclude => {
            if let Some(inc) = node.cast::<ast::ModuleInclude>() {
                path_expr(source, "include", inc.source(), node, out);
            }
        }
        SyntaxKind::ModuleImport => {
            if let Some(imp) = node.cast::<ast::ModuleImport>() {
                let src = imp.source();
                if !matches!(src, Expr::Ident(_) | Expr::FieldAccess(_)) {
                    path_expr(source, "import", src, node, out);
                }
            }
        }
        _ => {}
    }
    for child in node.children() {
        walk(source, &child, out);
    }
}

fn func_call(source: &str, call: ast::FuncCall, node: &LinkedNode, out: &mut Vec<PathArg>) {
    let Expr::Ident(name) = call.callee() else {
        return;
    };
    let name = name.as_str();
    if !PATH_CALLS.contains(&name) {
        return;
    }
    let first = call.args().items().find_map(|a| match a {
        Arg::Pos(e) => Some(e),
        _ => None,
    });
    let Some(first) = first else {
        out.push(not_literal(source, name, node));
        return;
    };
    if name == "bibliography" {
        match unparen(first) {
            Expr::Array(items) => {
                for item in items.items() {
                    match item {
                        ArrayItem::Pos(e) => path_expr(source, name, e, node, out),
                        ArrayItem::Spread(_) => out.push(not_literal(source, name, node)),
                    }
                }
                return;
            }
            e => return path_expr(source, name, e, node, out),
        }
    }
    path_expr(source, name, first, node, out);
}

fn unparen(mut e: Expr) -> Expr {
    while let Expr::Parenthesized(p) = e {
        e = p.expr();
    }
    e
}

/// `anchor` supplies the line of a non-literal; a literal reports its own line.
fn path_expr(source: &str, call: &str, expr: Expr, anchor: &LinkedNode, out: &mut Vec<PathArg>) {
    match unparen(expr) {
        Expr::Str(s) => {
            let value = s.get().to_string();
            if is_remote(&value) {
                return;
            }
            let range = locate(anchor, s.to_untyped());
            match range.and_then(|r| inner_span(source, r)) {
                Some(span) => out.push(PathArg::Literal(PathLiteral {
                    line: line_of(source, span.start),
                    span,
                    value,
                    call: call.to_string(),
                })),
                None => out.push(not_literal(source, call, anchor)),
            }
        }
        _ => out.push(not_literal(source, call, anchor)),
    }
}

fn is_remote(value: &str) -> bool {
    value.starts_with('@') || value.contains("://")
}

fn not_literal(source: &str, call: &str, anchor: &LinkedNode) -> PathArg {
    PathArg::NotLiteral {
        call: call.to_string(),
        line: line_of(source, anchor.offset()),
    }
}

/// The range between the quotes, or `None` when the source does not carry quotes there.
fn inner_span(source: &str, whole: Range<usize>) -> Option<Range<usize>> {
    let text = source.get(whole.clone())?;
    if text.len() < 2 || !text.starts_with('"') || !text.ends_with('"') {
        return None;
    }
    Some(whole.start + 1..whole.end - 1)
}

/// The linked form of `target`, which lives somewhere under `anchor`, found by identity.
fn locate(anchor: &LinkedNode, target: &SyntaxNode) -> Option<Range<usize>> {
    if std::ptr::eq(anchor.get(), target) {
        return Some(anchor.range());
    }
    anchor.children().find_map(|c| locate(&c, target))
}
