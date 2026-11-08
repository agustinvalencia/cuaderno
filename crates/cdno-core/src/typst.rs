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

use typst_syntax::{SyntaxKind, SyntaxNode, parse};

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

/// P8: `Ok(())` when the tree holds no Error node; else the first one's line and message.
pub fn parses(source: &str) -> Result<(), SyntaxFault> {
    let root = parse(source);
    let Some(offset) = first_error_offset(&root, 0) else {
        return Ok(());
    };
    let line = source[..offset.min(source.len())]
        .bytes()
        .filter(|b| *b == b'\n')
        .count()
        + 1;
    let (errors, _) = root.errors_and_warnings();
    let message = errors
        .first()
        .map(|e| e.message.to_string())
        .unwrap_or_else(|| "syntax error".to_string());
    Err(SyntaxFault { line, message })
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
