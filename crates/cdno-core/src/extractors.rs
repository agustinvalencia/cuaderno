//! Body-scanned extractors for inline `#tag` tokens and `[[wikilink]]`
//! references.
//!
//! [`extract_note_facets`] is the single place a note's `tags` and
//! `links` facets are derived: it merges the frontmatter `tags:` list and
//! frontmatter wikilinks with the body-scanned ones below. Reconciliation
//! and the transaction's commit seam both call it, so a note indexed by
//! an in-tool write and the same note reindexed from disk produce the
//! same facet rows (#646).
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

use crate::error::ValidationError;
use crate::frontmatter::Frontmatter;
use crate::index::LinkEntry;
use crate::path::VaultPath;

/// A note's derived `tags` and `links` facets, ready for
/// `VaultIndex::replace_tags` / `VaultIndex::replace_links`.
///
/// The two facets are independent: an invalid frontmatter `tags:` value
/// fails `tags` alone, and `links` is always derived, so a caller never
/// loses a note's edges to a malformed tag list.
#[derive(Debug)]
pub struct NoteFacets {
    /// Frontmatter `tags:` merged with body `#tag` tokens, deduped and
    /// sorted so the `note_tags` table doesn't churn across passes. `Err`
    /// when the frontmatter `tags:` field is present but not a list of
    /// strings; the body's inline tags alone are then
    /// [`extract_inline_tags`] of the same body.
    pub tags: Result<Vec<String>, ValidationError>,
    /// Body wikilinks followed by frontmatter wikilinks, deduped by
    /// `(target, label)` and resolved with [`resolve_wikilinks`].
    pub links: Vec<LinkEntry>,
}

/// Derive a note's `tags` and `links` facets from its parsed frontmatter
/// and body.
///
/// `frontmatter_json` is `frontmatter.as_json()`, passed in so a caller
/// that also needs it (reconcile stores it on the `notes` row) serialises
/// once. `vault_paths` is the set of note paths links resolve against.
///
/// Tags: the frontmatter `tags:` list merged with the body's inline tags,
/// deduped and sorted; an invalid `tags:` value makes
/// [`NoteFacets::tags`] an `Err` and nothing else. Links: body wikilinks
/// merged with frontmatter wikilinks (a project's `core_question:`, an
/// evidence or concept note's `origin:`, …) so backlinks see frontmatter
/// references too (#395). Deduped by `(target, label)`, body first so its
/// position wins, then resolved against `vault_paths`.
pub fn extract_note_facets(
    frontmatter: &Frontmatter,
    frontmatter_json: &serde_json::Value,
    body: &str,
    vault_paths: &HashSet<VaultPath>,
) -> NoteFacets {
    let tags = frontmatter
        .optional_field::<Vec<String>>("tags")
        .map(|frontmatter_tags| {
            let mut tag_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            tag_set.extend(frontmatter_tags.unwrap_or_default());
            tag_set.extend(extract_inline_tags(body));
            tag_set.into_iter().collect()
        });

    let mut raw_links = extract_wikilinks(body);
    raw_links.extend(extract_frontmatter_wikilinks(frontmatter_json));
    let mut seen = HashSet::new();
    raw_links.retain(|l| seen.insert((l.target.clone(), l.label.clone())));
    let links = resolve_wikilinks(raw_links, vault_paths);

    NoteFacets { tags, links }
}

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
    /// Whether the source form was an Obsidian embed (`![[...]]`)
    /// rather than a plain link (`[[...]]`). An embed transcludes a
    /// note or displays an attachment inline; the distinction only
    /// changes how a *failed* target reads — a missing embed is a
    /// missing file, not a link that "resolves to no note".
    pub is_embed: bool,
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
                // An `![[...]]` embed carries a `!` immediately before the
                // opening `[[`. The scan above only advanced through ASCII
                // `[`/`]`, so `start - 1` lands on a UTF-8 boundary.
                let is_embed = start > 0 && body.as_bytes()[start - 1] == b'!';
                links.push(WikilinkRaw {
                    target,
                    label: label.filter(|s| !s.is_empty()),
                    is_embed,
                });
            }
        }
        cursor = inner_end + 2;
    }

    links
}

/// Extract every `[[wikilink]]` from the string values of a note's parsed
/// frontmatter (recursing into arrays and nested maps), in traversal order
/// (#395).
///
/// Domain-agnostic by design: cdno-core knows nothing about which fields
/// carry links, so it simply scans every scalar string. A link-bearing
/// field — a project's `core_question:`, a portfolio's `project:`, an
/// evidence note's `origin:` — holds a wikilink string this catches without
/// naming the field, while non-link fields (`status:`, `created:`, `energy:`)
/// contain no `[[...]]` and contribute nothing. Fed alongside the body links
/// during reconciliation so `find_backlinks` sees frontmatter references,
/// not just body ones.
///
/// The one accepted cost of not knowing field semantics: a literal `[[…]]`
/// typed into a freeform frontmatter field (e.g. an action's `criteria:`, or
/// an arbitrary key on a custom note type) becomes a real edge too. Rare,
/// and no worse than the same text in the body — a resolved edge is a
/// legitimate backlink; an unresolved one is a harmless dangling edge.
pub fn extract_frontmatter_wikilinks(frontmatter: &serde_json::Value) -> Vec<WikilinkRaw> {
    fn walk(value: &serde_json::Value, out: &mut Vec<WikilinkRaw>) {
        match value {
            serde_json::Value::String(s) => out.extend(extract_wikilinks(s)),
            serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            serde_json::Value::Object(map) => map.values().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(frontmatter, &mut out);
    out
}

/// Resolve a list of [`WikilinkRaw`]s against the vault's known paths.
///
/// Resolution policy, in order:
/// 0. Anchor strip: `[[note#Heading]]` resolves as `[[note]]` — the anchor
///    is opaque and kept only on `target_raw`. The exact path is tried on
///    the unsplit target *before* the anchor is stripped, so a filename
///    that itself contains `#` still resolves. `[[#Heading]]` (no path
///    part) is a same-note reference and never resolves.
/// 1. Exact path match: `[[projects/foo]]` → `projects/foo.md` if
///    that path exists in `vault_paths`.
/// 2. Folder-index match: `[[portfolios/foo]]` → `portfolios/foo/_index.md`
///    if that path exists — a folder-backed note (a portfolio, an expanded
///    stewardship) is named by its folder, so a bare link to it resolves to
///    its `_index.md`. Ordered before the fuzzy stem match so a folder link
///    can't be hijacked by an unrelated note sharing its last segment.
/// 3. Last-segment match: the target's final path segment against note
///    stems — `[[foo]]` or `[[actions/foo]]` → `*/foo.md` if exactly
///    one path has the stem `foo`. This resolves a link to a note that
///    relocated within its tree (e.g. an action archived to
///    `actions/_done/<year>/`); a unique match is required (#215).
/// 4. Otherwise `resolved_path` is `None` and the link is recorded as
///    broken — `cdno lint` surfaces it as a warning.
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

pub(crate) fn resolve_one(target: &str, vault_paths: &HashSet<VaultPath>) -> Option<VaultPath> {
    // 0a. Exact path match on the unsplit target, before the anchor is
    // stripped. A `#` in a filename is otherwise unsupported — it is only
    // reachable through an exact-path link — because rule 0b below would
    // otherwise always treat it as an anchor separator and mis-resolve
    // `notes/c#-notes` to `notes/c` if that also exists.
    if let Ok(vp) = VaultPath::new(format!("{target}.md"))
        && vault_paths.contains(&vp)
    {
        return Some(vp);
    }

    // 0b. Strip the anchor. `[[note#Heading]]` (and Obsidian's `#^block`)
    // addresses a place *inside* a note; the note is the path part before
    // the first `#`. The anchor is opaque here — it stays on
    // `LinkEntry::target_raw` for any later heading check — so a link with
    // one resolves exactly as the bare link would. Without this every
    // anchored link (`milestone:` fields, `## Notes` pointers, `origin:`)
    // silently produced no edge and no backlink (RFC 0002 stage 0).
    let target = target.split_once('#').map_or(target, |(path, _)| path);
    let target = target.trim_end();
    if target.is_empty() {
        // `[[#Heading]]` names a heading in the linking note itself; that
        // is not a link to another note.
        return None;
    }

    // 1. Exact path match.
    if let Ok(vp) = VaultPath::new(format!("{target}.md"))
        && vault_paths.contains(&vp)
    {
        return Some(vp);
    }

    // 1b. Folder-index match: a target naming a folder resolves to that
    // folder's `_index.md`. A portfolio (and any expanded folder note) is
    // a directory whose canonical note is `_index.md`, so the
    // `[[portfolios/<slug>]]` form the daily-log writer and
    // `file_to_portfolio` emit has no flat `<target>.md` to satisfy rule 1,
    // and its final segment (`<slug>`) never equals the index file's stem
    // (`_index`) for rule 2 — leaving every portfolio link dead. Resolve it
    // explicitly to the index note. Exact and unambiguous like rule 1, and
    // ordered before the fuzzy stem match so a folder link can never be
    // hijacked by an unrelated note that happens to share the last segment.
    if let Ok(vp) = VaultPath::new(format!("{target}/_index.md"))
        && vault_paths.contains(&vp)
    {
        return Some(vp);
    }

    // 2. Last-segment match: resolve by the note's filename stem
    // against the target's final path segment. For a bare target
    // (`[[foo]]`) the segment is the whole thing; for a qualified one
    // (`[[actions/foo]]`) it's `foo`. This lets a link survive a note
    // relocating *within its tree* without rewriting every reference:
    // `[[actions/<slug>]]` still resolves after the note is archived to
    // `actions/_done/<year>/<slug>.md` (#215), and likewise for parked
    // projects and completed commitments.
    //
    // A unique stem match wins; zero or multiple matches leave the link
    // unresolved. This keeps resolution *sound* — a stem collision never
    // resolves to the wrong note, it resolves to neither (`None`). The
    // cost is availability, not correctness: when two notes share a stem
    // (slugs aren't globally unique — see `vault::slug`), a legitimately
    // relocated note's backlinks degrade to unresolved rather than
    // misdirecting. Still strictly better than the pervasive dangling it
    // fixes; a slug-uniqueness pass would shrink the collision window
    // further.
    let needle = target.rsplit('/').next().unwrap_or(target);
    let mut matches = vault_paths.iter().filter(|p| {
        p.as_path()
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|stem| stem == needle)
    });
    let first = matches.next()?;
    if matches.next().is_some() {
        // Ambiguous — multiple notes share the basename.
        return None;
    }
    Some(first.clone())
}

/// The names a note answers to under [`resolve_wikilinks`]'s rules: its
/// file stem always (the exact rule, e.g. `[[portfolios/<slug>/_index]]`,
/// and the stem rule), and for a folder note (`<dir>/_index.md`) also the
/// folder's name, when it has one (the folder-index rule, e.g.
/// `[[portfolios/<slug>]]`). Every rule that can resolve a target to
/// `path` compares the target's last segment with one of these (see
/// [`target_may_name`]).
pub fn note_link_names(path: &VaultPath) -> Vec<&str> {
    let p = path.as_path();
    let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let mut names = vec![stem];
    if stem == "_index"
        && let Some(folder) = p
            .parent()
            .and_then(|d| d.file_name())
            .and_then(|f| f.to_str())
    {
        names.push(folder);
    }
    names
}

/// Whether the wikilink `target` could resolve to, or be disambiguated by,
/// a note with one of [`note_link_names`] in `names`. A cheap pre-filter for
/// re-resolution: it never rejects a target that some rule of
/// [`resolve_wikilinks`] could match to such a note. The exact rule (with
/// or without the anchor) and the stem rule all end on the note's stem
/// (`_index` for an explicit `[[<dir>/_index]]`), the folder-index rule on
/// its folder's name, and a stem ambiguity is
/// only cleared by removing a note with the same stem. Both the unsplit
/// target (rule 0a, for a `#` in a file name) and the anchor-stripped one
/// are checked.
pub fn target_may_name(target: &str, names: &HashSet<&str>) -> bool {
    let last = |t: &str| t.rsplit('/').next().unwrap_or(t).to_owned();
    let unsplit = last(target);
    let stripped = target.split_once('#').map_or(target, |(path, _)| path);
    let stripped = last(stripped.trim_end());
    names.contains(unsplit.as_str()) || names.contains(stripped.as_str())
}

/// The text of the body's first level-1 (`# `) heading, trimmed, or
/// `None` if there isn't one.
///
/// Cuaderno notes carry their human title as the body H1, not a
/// frontmatter `title:` field, so this is the canonical title source —
/// e.g. for the FTS `title` column, where it earns the bm25 weight a
/// `notes.title` (frontmatter-derived, ~always absent) cannot. A simple
/// line scan, matching the per-module `extract_h1` helpers in cdno-domain
/// (a future cleanup could collapse those onto this one).
pub fn first_h1(body: &str) -> Option<String> {
    body.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("# ")
            .map(|text| text.trim().to_owned())
    })
}
