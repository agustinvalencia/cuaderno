//! Body-scanned extractors for inline `#tag` tokens and `[[wikilink]]`
//! references.
//!
//! Frontmatter `tags:` lists are handled directly by the reconciler;
//! the helpers in this module cover the body-scanned facets the
//! reconciler merges in.
//!
//! ## Skip rules
//!
//! Both extractors walk the body via `pulldown-cmark` and ignore
//! markdown contexts where a `#`-prefixed token or `[[...]]` token
//! shouldn't be load-bearing:
//!
//! - **Tags** skip code blocks, inline code spans, HTML, and headings.
//!   Headings are skipped because a heading's text shouldn't seed
//!   tags — a writer adding `## My #important note` doesn't intend
//!   `important` to become a vault-level tag.
//! - **Wikilinks** skip code blocks, inline code spans, and HTML.
//!   Headings are *not* skipped: a wikilink in a heading is still a
//!   real reference (e.g. `## See also: [[other-note]]`).

use std::collections::HashSet;
use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::index::LinkEntry;
use crate::path::VaultPath;

/// Tag pattern is ASCII-only by design: `#[a-zA-Z0-9][a-zA-Z0-9_/-]*`,
/// with trailing slashes trimmed by the caller. The inner `/` carries
/// namespaced tags like `#action/<slug>` (design §5.11) — the slug is
/// part of the tag, not a separate token. Non-ASCII letters in `#café`
/// produce `caf` (or get rejected), matching the spec in `docs/` and
/// avoiding surprising Unicode behaviour in indexed tags.
fn is_tag_continuation(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'/'
}

/// Compute the byte ranges in `body` where extractors must not look.
/// `include_headings` adds heading event ranges to the protection set
/// — used by tag extraction but not wikilink extraction.
fn protected_ranges(body: &str, include_headings: bool) -> Vec<Range<usize>> {
    let parser = Parser::new_ext(body, Options::all()).into_offset_iter();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut code_block_start: Option<usize> = None;
    let mut heading_start: Option<usize> = None;

    for (event, range) in parser {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                code_block_start.get_or_insert(range.start);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(start) = code_block_start.take() {
                    ranges.push(start..range.end);
                }
            }
            Event::Start(Tag::Heading { .. }) if include_headings => {
                heading_start.get_or_insert(range.start);
            }
            Event::End(TagEnd::Heading(_)) if include_headings => {
                if let Some(start) = heading_start.take() {
                    ranges.push(start..range.end);
                }
            }
            // Inline code spans, raw HTML blocks, and inline HTML all
            // arrive as a single event with the precise byte range
            // they cover — we can record those directly.
            Event::Code(_) | Event::Html(_) | Event::InlineHtml(_) => {
                ranges.push(range);
            }
            _ => {}
        }
    }
    ranges
}

fn is_protected(ranges: &[Range<usize>], offset: usize) -> bool {
    ranges.iter().any(|r| r.contains(&offset))
}

/// Extract every distinct `#tag` from a markdown body, sorted.
///
/// Skips code blocks, inline code spans, HTML, and headings. The tag
/// pattern is `#[a-zA-Z0-9][a-zA-Z0-9_/-]*` — punctuation right after
/// a tag (e.g. `#foo,`) doesn't bleed into the tag. The inner `/`
/// supports namespaced tags (`#action/<slug>`); a trailing slash is
/// not part of the tag, so `#foo/` tags `foo`.
pub fn extract_inline_tags(body: &str) -> Vec<String> {
    let protected = protected_ranges(body, /* include_headings */ true);
    let mut tags: HashSet<String> = HashSet::new();
    let bytes = body.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'#' || is_protected(&protected, i) {
            i += 1;
            continue;
        }

        // Boundary: a tag must start at the beginning of the body
        // or after whitespace / opening bracket. `foo#bar` is not a
        // tag.
        let at_boundary = i == 0 || {
            let prev = bytes[i - 1];
            prev.is_ascii_whitespace() || matches!(prev, b'(' | b'[' | b'{')
        };
        if !at_boundary {
            i += 1;
            continue;
        }

        let start = i + 1;
        let mut end = start;
        // First char of the tag body must be alphanumeric per spec.
        if end < bytes.len() && bytes[end].is_ascii_alphanumeric() {
            end += 1;
            while end < bytes.len() && is_tag_continuation(bytes[end]) {
                end += 1;
            }
            // The scan only advanced through ASCII bytes, so the
            // start..end indices land on UTF-8 boundaries even when
            // the surrounding text is multibyte.
            //
            // Trailing slashes are trimmed: `#foo/` tags `foo`, while a
            // namespaced `#action/slug` keeps its inner slash. `i` still
            // advances past the slash so it isn't re-scanned. The
            // first-char-alphanumeric check above guarantees the trimmed
            // tag is non-empty.
            let tag = body[start..end].trim_end_matches('/');
            tags.insert(tag.to_string());
            i = end;
            continue;
        }
        i += 1;
    }

    let mut result: Vec<String> = tags.into_iter().collect();
    result.sort();
    result
}

/// A wikilink as it appears in the source, before resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikilinkRaw {
    /// The text inside `[[...]]`, before any `|`. Trimmed of
    /// surrounding whitespace.
    pub target: String,
    /// The label after a `|`, if present. `None` for plain
    /// `[[target]]`; `Some` for `[[target|Display Name]]`. Empty
    /// labels (`[[target|]]`) are normalised to `None`.
    pub label: Option<String>,
}

/// Extract every `[[target]]` and `[[target|label]]` wikilink from a
/// markdown body, in source order. Skips code blocks, code spans, and
/// HTML. Wikilinks that span newlines are dropped — they're almost
/// certainly the user accidentally splitting one across lines, and
/// scanning past line breaks is more likely to glue unrelated tokens
/// together than to recover the user's intent.
pub fn extract_wikilinks(body: &str) -> Vec<WikilinkRaw> {
    let protected = protected_ranges(body, /* include_headings */ false);
    let mut links: Vec<WikilinkRaw> = Vec::new();
    let mut cursor = 0;

    while let Some(rel) = body[cursor..].find("[[") {
        let start = cursor + rel;
        if is_protected(&protected, start) {
            cursor = start + 2;
            continue;
        }
        let after_open = start + 2;
        let Some(close_rel) = body[after_open..].find("]]") else {
            // Unclosed `[[` — don't keep scanning for stray `]]`
            // tokens elsewhere in the document.
            break;
        };
        let inner_end = after_open + close_rel;
        let inner = &body[after_open..inner_end];

        // Reject newline-spanning links and empty `[[]]`.
        if !inner.is_empty() && !inner.contains('\n') {
            let (target, label) = match inner.find('|') {
                Some(pipe) => (
                    inner[..pipe].trim().to_string(),
                    Some(inner[pipe + 1..].trim().to_string()),
                ),
                None => (inner.trim().to_string(), None),
            };
            if !target.is_empty() {
                links.push(WikilinkRaw {
                    target,
                    label: label.filter(|s| !s.is_empty()),
                });
            }
        }
        cursor = inner_end + 2;
    }

    links
}

/// Resolve a list of [`WikilinkRaw`]s against the vault's known paths.
///
/// Resolution policy, in order:
/// 1. Exact path match: `[[projects/foo]]` → `projects/foo.md` if
///    that path exists in `vault_paths`.
/// 2. Basename match: `[[foo]]` → `*/foo.md` if exactly one path in
///    the vault has the stem `foo`.
/// 3. Otherwise `resolved_path` is `None` and the link is recorded
///    as broken — `cdno lint` can surface it once the lint surface
///    grows broken-wikilink checks.
pub fn resolve_wikilinks(
    raws: Vec<WikilinkRaw>,
    vault_paths: &HashSet<VaultPath>,
) -> Vec<LinkEntry> {
    raws.into_iter()
        .map(|raw| {
            let resolved = resolve_one(&raw.target, vault_paths);
            LinkEntry {
                target_raw: raw.target,
                resolved_path: resolved,
                label: raw.label,
            }
        })
        .collect()
}

fn resolve_one(target: &str, vault_paths: &HashSet<VaultPath>) -> Option<VaultPath> {
    // 1. Exact path match.
    if let Ok(vp) = VaultPath::new(format!("{target}.md"))
        && vault_paths.contains(&vp)
    {
        return Some(vp);
    }

    // 2. Basename match: collect every path whose `.md` stem equals
    // `target`. A unique match wins; zero or multiple matches mean
    // the link stays unresolved.
    let mut matches = vault_paths.iter().filter(|p| {
        p.as_path()
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|stem| stem == target)
    });
    let first = matches.next()?;
    if matches.next().is_some() {
        // Ambiguous — multiple notes share the basename.
        return None;
    }
    Some(first.clone())
}
