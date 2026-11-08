//! The Typst parser, isolated (RFC 0008 §5.2, P8).
//!
//! This is the only module that names `typst_syntax`. It offers the cheap byte search that runs
//! before any parse, and the single definition of "does not parse".
//!
//! Measured in the T0 spike (typst-syntax 0.15.1, macOS arm64, release build, warm cache):
//!
//! - Parse cost is about 11 us per KB: 25 us at 2 KB, 226 us at 20 KB, 2.2 ms at 200 KB. A byte
//!   search for `<cdno>` is about 400 times cheaper (0.04 us, 0.6 us, 13 us for `contains`).
//! - `Vault::new` over 58 stubbed attachments of 20 KB each (1.19 MB): 4.8 to 5.4 ms unmodified,
//!   about 6.1 ms when every `.typ` is read and byte-searched. Roughly 1 ms per command, an upper
//!   bound, since a file inside an attachment's folder need not be read at all.
//! - Compile time: the workspace build grows by 0.6 to 0.9 s wall (about 22.4 s to 23.2 s).
//! - Crate count: `cargo tree -e normal -p cdno-core --prefix none | sort -u` grows from 75 to
//!   101 lines, 24 crates added (typst-syntax, -timing, -utils, ecow, rayon, parking_lot,
//!   unicode-* and their dependencies).

mod edit;
mod links;
mod metadata;

pub use links::{LinkFault, TypstLinks, headings, wikilink_spans};

use typst_syntax::{Lines, SyntaxKind, SyntaxNode, parse};

pub use edit::{append_list_value, remove_list_value, rewrite_scalar, scalar_span, write_block};
pub use metadata::{EntrySpan, MetadataBlock, TypstError, metadata_block};

mod paths;

pub use paths::{PathArg, PathLiteral, path_args};

/// The label that marks a Typst file as a note.
pub const CDNO_LABEL: &str = "<cdno>";

/// A syntax error found by [`parses`]: the 1-based line of the first error node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxFault {
    pub line: usize,
    pub message: String,
}

/// The byte search of §5.2: cheap, run before any parse. It is deliberately naive, so the label
/// inside a comment or a string still answers `true`; the parse decides.
pub fn has_label(source: &str) -> bool {
    source.contains(CDNO_LABEL)
}

/// Whether the tree holds a `<cdno>` Label node: the label as the parser reads it, so one in a
/// comment, a raw block or a string does not count. A parser that cannot start answers `true`.
pub fn has_label_node(source: &str) -> bool {
    with_tree(source, label_node_in).unwrap_or(true)
}

fn label_node_in(node: &SyntaxNode) -> bool {
    if node.kind() == SyntaxKind::Label && node.leaf_text() == CDNO_LABEL {
        return true;
    }
    node.children().any(label_node_in)
}

/// P8: `Ok(())` when the tree holds no Error node; else the first one's line and message.
pub fn parses(source: &str) -> Result<(), SyntaxFault> {
    with_tree(source, |root| {
        let Some(offset) = first_error_offset(root, 0) else {
            return Ok(());
        };
        let (errors, _) = root.errors_and_warnings();
        let message = errors
            .first()
            .map(|e| e.message.to_string())
            .unwrap_or_else(|| "syntax error".to_string());
        Err(SyntaxFault {
            line: line_of(source, offset),
            message,
        })
    })?
}

/// Every use of the parser must go through this. A chain such as `#a.b.b.b...` builds a tree as
/// deep as the input is long, and walking or dropping it recurses once per level: on a default
/// stack that is an abort, not a panic. So the parse, the walk `f` and the drop of the tree all
/// run on a scoped thread whose stack grows with the source.
///
/// The stack is `max(8 MiB, 1024 * source.len())`. Measured on the worst input found, an
/// error-carrying chain (`$#(1 +)!!!...$`), one level costs 208 bytes of stack per source byte in
/// a release build and 480 in a dev build; 1024 leaves a margin of about twofold over the dev
/// figure. The stack is reserved address space, not committed memory, so the allowance costs
/// nothing until a deep tree uses it. On a very large file the reservation itself may fail; that
/// surfaces as a fault from the spawn, not an abort.
///
/// Unlike the bare `R` one might expect, this returns a `Result`: if the thread cannot be
/// spawned, the caller gets a fault rather than a panic.
fn with_tree<R: Send>(
    source: &str,
    f: impl FnOnce(&SyntaxNode) -> R + Send,
) -> Result<R, SyntaxFault> {
    let stack = (1024 * source.len()).max(8 * 1024 * 1024);
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .stack_size(stack)
            .spawn_scoped(scope, || {
                let root = parse(source);
                f(&root)
            })
            .map_err(|e| SyntaxFault {
                line: 1,
                message: format!("cannot start the Typst parser thread: {e}"),
            })?;
        match handle.join() {
            Ok(r) => Ok(r),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    })
}

/// The 1-based line of a byte offset, by Typst's own line rules (`\n`, `\r`, VT, FF, U+0085,
/// U+2028, U+2029). Never slices `source`, so an offset inside a character cannot panic.
fn line_of(source: &str, offset: usize) -> usize {
    line_in(&Lines::new(source), offset)
}

/// [`line_of`] against a line table already built, for a caller with many offsets to place.
fn line_in(lines: &Lines<&str>, offset: usize) -> usize {
    lines
        .byte_to_line(offset.min(lines.text().len()))
        .map_or(1, |l| l + 1)
}

fn first_error_offset(node: &SyntaxNode, start: usize) -> Option<usize> {
    if node.kind() == SyntaxKind::Error {
        return Some(start);
    }
    if !node.diagnosis().errors {
        return None;
    }
    let mut at = start;
    for child in node.children() {
        if let Some(found) = first_error_offset(child, at) {
            return Some(found);
        }
        at += child.len();
    }
    None
}
