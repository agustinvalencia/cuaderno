//! The files a Typst document reaches for (RFC 0008 §5.7, T9).

use std::ops::Range;

use typst_syntax::ast::{self, Arg, ArrayItem, AstNode, Expr};
use typst_syntax::{LinkedNode, SyntaxKind, SyntaxNode};

use super::{line_of, with_tree};

/// What a named argument holds.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// Any string is a path.
    Path,
    /// A string is a path only when it has an extension (`my.csl`); other strings are built-in style names.
    Style,
}

/// A listed function: whether its first positional is a path, and its path-valued named arguments.
struct Spec {
    positional: bool,
    named: &'static [(&'static str, Kind)],
}

fn spec(name: &str) -> Option<Spec> {
    const P: Spec = Spec {
        positional: true,
        named: &[],
    };
    Some(match name {
        "read" | "json" | "csv" | "yaml" | "toml" | "xml" | "cbor" | "plugin" | "pdf.attach" => P,
        "image" => Spec {
            positional: true,
            named: &[("icc", Kind::Path)],
        },
        "bibliography" => Spec {
            positional: true,
            named: &[("style", Kind::Style)],
        },
        "cite" => Spec {
            positional: false,
            named: &[("style", Kind::Style)],
        },
        "raw" => Spec {
            positional: false,
            named: &[("theme", Kind::Path), ("syntaxes", Kind::Path)],
        },
        _ => return None,
    })
}

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

/// Every path argument, in document order, of `image` (and its `icc:`), `read`, `json`, `csv`,
/// `yaml`, `toml`, `xml`, `cbor`, `plugin`, `pdf.attach`, `bibliography`, `include` and a path
/// `import`; of `bibliography(style:)` and `cite(style:)` when the string has an extension (any
/// other string is a built-in style name); and of `raw(theme:, syntaxes:)`. Arrays yield one entry
/// per item. The callee may be bare, `std.`-qualified, parenthesised or reached through
/// `.with(..)`; `set` rules are scanned too.
///
/// Not returned: an `import` or `include` of an `@` package, and an `import` of a standard-library
/// module (`calc`, `std`, `sys`, `math`, `pdf`, `sym`, `emoji`, `std.<name>`). A `://` string is an
/// ordinary local path. Anything whose value cannot be known is `NotLiteral`: a non-string path, an
/// `import` from any other name, a listed call (or `.with(..)`) with no positional path outside a
/// `set` rule. If the parser cannot run, one `NotLiteral` is returned rather than an empty list.
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
                func_call(source, call.callee(), call.args(), false, node, out);
            }
        }
        SyntaxKind::SetRule => {
            if let Some(set) = node.cast::<ast::SetRule>() {
                func_call(source, set.target(), set.args(), true, node, out);
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
                if !is_std_module(src) {
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

/// The listed function a callee names, and whether it was reached through `.with(...)`. Accepts `name`, `std.name`, `pdf.attach`, `std.pdf.attach`,
/// `name.with(..)` and a parenthesised callee.
fn resolve(callee: Expr) -> Option<(&'static str, bool)> {
    let canon = |n: &str| -> Option<&'static str> {
        [
            "image",
            "read",
            "json",
            "csv",
            "yaml",
            "toml",
            "xml",
            "cbor",
            "bibliography",
            "cite",
            "raw",
            "plugin",
            "pdf.attach",
        ]
        .into_iter()
        .find(|k| *k == n)
    };
    match unparen(callee) {
        Expr::Ident(i) => canon(i.as_str()).map(|n| (n, false)),
        Expr::FuncCall(c) => resolve(c.callee()).map(|(n, _)| (n, true)),
        Expr::FieldAccess(a) => {
            let field = a.field().as_str();
            let target = unparen(a.target());
            match (&target, field) {
                (_, "with") => resolve(target).map(|(n, _)| (n, true)),
                (Expr::Ident(t), f) if t.as_str() == "std" => canon(f).map(|n| (n, false)),
                (Expr::Ident(t), "attach") if t.as_str() == "pdf" => Some(("pdf.attach", false)),
                (Expr::FieldAccess(t), "attach")
                    if t.field().as_str() == "pdf"
                        && matches!(unparen(t.target()), Expr::Ident(s) if s.as_str() == "std") =>
                {
                    Some(("pdf.attach", false))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// `is_set`: a `set` rule needs no positional path, so a missing one is not reported.
fn func_call(
    source: &str,
    callee: Expr,
    args: ast::Args,
    is_set: bool,
    node: &LinkedNode,
    out: &mut Vec<PathArg>,
) {
    let Some((name, _)) = resolve(callee) else {
        return;
    };
    let Some(spec) = spec(name) else { return };
    if spec.positional {
        let first = args.items().find_map(|a| match a {
            Arg::Pos(e) => Some(e),
            _ => None,
        });
        match first {
            Some(e) => path_value(source, name, e, Kind::Path, node, out),
            None if !is_set => out.push(not_literal(source, name, node)),
            None => {}
        }
    }
    for arg in args.items() {
        if let Arg::Named(n) = arg
            && let Some((_, kind)) = spec.named.iter().find(|(k, _)| *k == n.name().as_str())
        {
            path_value(source, name, n.expr(), *kind, node, out);
        }
    }
}

/// A path value: a string, or an array of them. `none` and `auto` name no file.
fn path_value(
    source: &str,
    call: &str,
    expr: Expr,
    kind: Kind,
    anchor: &LinkedNode,
    out: &mut Vec<PathArg>,
) {
    match unparen(expr) {
        Expr::Array(items) => {
            for item in items.items() {
                match item {
                    ArrayItem::Pos(e) => path_value(source, call, e, kind, anchor, out),
                    ArrayItem::Spread(_) => out.push(not_literal(source, call, anchor)),
                }
            }
        }
        Expr::None(_) | Expr::Auto(_)
            if kind == Kind::Style || call == "raw" || call == "image" => {}
        Expr::Str(s)
            if kind == Kind::Style
                && std::path::Path::new(s.get().as_str()).extension().is_none() => {}
        e => path_expr(source, call, e, anchor, out),
    }
}

/// The standard-library modules an `import` may name; they are not files.
fn is_std_module(e: Expr) -> bool {
    const MODULES: &[&str] = &["calc", "std", "sys", "math", "pdf", "sym", "emoji"];
    match unparen(e) {
        Expr::Ident(i) => MODULES.contains(&i.as_str()),
        Expr::FieldAccess(a) => {
            matches!(unparen(a.target()), Expr::Ident(t) if t.as_str() == "std")
        }
        _ => false,
    }
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
            if value.starts_with('@') && (call == "import" || call == "include") {
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
