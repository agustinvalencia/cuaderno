//! Shared test helper: drop ANSI styling, keep the visible characters.
//!
//! Lives in a subdirectory so cargo does not compile it as a test target
//! of its own — it holds no tests, only the helper that lets a styled
//! render be compared against a plain one.

/// Remove every SGR escape sequence (`ESC [ … m`) from `text`.
pub fn strip_sgr(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        for c in chars.by_ref() {
            if c == 'm' {
                break;
            }
        }
    }
    out
}
