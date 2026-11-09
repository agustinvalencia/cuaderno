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

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::error::ValidationError;
use crate::frontmatter::Frontmatter;
use crate::index::LinkEntry;
use crate::path::VaultPath;
use crate::paths::{NoteFormat, note_format};
use crate::typst;

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
    /// `(target, label)` and resolved with [`resolve_wikilinks`] against the
    /// path set and the [`AliasMap`].
    pub links: Vec<LinkEntry>,
}

/// Derive a note's `tags` and `links` facets from its parsed frontmatter
/// and body.
///
/// `frontmatter_json` is `frontmatter.as_json()`, passed in so a caller
/// that also needs it (reconcile stores it on the `notes` row) serialises
/// once. `vault_paths` is the set of note paths links resolve against, and
/// `aliases` the vault's alias map (the final rule of [`resolve_wikilinks`]).
///
/// Tags: the frontmatter `tags:` list merged with the body's inline tags,
/// deduped and sorted; an invalid `tags:` value makes
/// [`NoteFacets::tags`] an `Err` and nothing else. Links: body wikilinks
/// merged with frontmatter wikilinks (a project's `core_question:`, an
/// evidence or concept note's `origin:`, …) so backlinks see frontmatter
/// references too (#395). Deduped by `(target, label)`, body first so its
/// position wins, then resolved against `vault_paths` and `aliases`.
pub fn extract_note_facets(
    path: &VaultPath,
    frontmatter: &Frontmatter,
    frontmatter_json: &serde_json::Value,
    body: &str,
    vault_paths: &HashSet<VaultPath>,
    aliases: &AliasMap,
) -> NoteFacets {
    let tags = frontmatter
        .optional_field::<Vec<String>>("tags")
        .map(|frontmatter_tags| {
            let mut tag_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            tag_set.extend(frontmatter_tags.unwrap_or_default());
            tag_set.extend(note_inline_tags(path, body));
            tag_set.into_iter().collect()
        });

    let mut raw_links = note_body_links(path, body);
    raw_links.extend(extract_frontmatter_wikilinks(frontmatter_json));
    let mut seen = HashSet::new();
    raw_links.retain(|l| seen.insert((l.target.clone(), l.label.clone())));
    let links = resolve_wikilinks(raw_links, vault_paths, aliases);

    NoteFacets { tags, links }
}

/// A note's inline tags, by format. Typst text carries none: a `#` there is
/// code, and tags come from the metadata block alone (RFC 0008 §5.4).
pub fn note_inline_tags(path: &VaultPath, body: &str) -> Vec<String> {
    match note_format(path) {
        Some(NoteFormat::Typst) => Vec::new(),
        _ => extract_inline_tags(body),
    }
}

/// The wikilinks of a note's `body` by its format, in source order: the Typst scanner for a
/// `.typ`, the Markdown scanner otherwise.
pub fn note_body_links(path: &VaultPath, body: &str) -> Vec<WikilinkRaw> {
    if note_format(path) != Some(NoteFormat::Typst) {
        return extract_wikilinks(body);
    }
    let links = typst::wikilink_spans(body);
    links
        .spans
        .iter()
        .zip(links.targets)
        .zip(links.labels)
        .map(|((span, target), label)| WikilinkRaw {
            target,
            label,
            is_embed: span.is_embed,
        })
        .collect()
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

/// Where one wikilink sits in a body, as byte offsets. The offsets the
/// scanner computes anyway, kept so a caller can edit part of a link
/// in place (RFC 0007 §5.4) instead of re-deriving them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikilinkSpan {
    /// Offset of the opening `[[`.
    pub open: usize,
    /// Bytes between `[[` and the first `#` or `|` inside the link,
    /// trimmed of surrounding whitespace — the range excludes that
    /// whitespace. Empty for `[[#Heading]]`.
    pub address: Range<usize>,
    /// Bytes between `[[` and the first `|`, trimmed, anchor included
    /// (`old#Current State` for `[[old#Current State]]`) — the string the
    /// resolver sees. After trimming, `address` is a prefix of it, or empty
    /// (`[[ #h]]` has an empty address at the start of the inner text).
    pub target: Range<usize>,
    /// Offset of the closing `]]`.
    pub inner_end: usize,
    /// Whether a `!` immediately precedes the opening `[[`.
    pub is_embed: bool,
}

/// Locate every wikilink in a markdown body, in source order, under the
/// same rules as [`extract_wikilinks`]: code blocks, code spans and HTML
/// are skipped, a link spanning a newline or with an empty target is
/// dropped, and an unclosed `[[` ends the scan.
pub fn wikilink_spans(body: &str) -> Vec<WikilinkSpan> {
    let protected = protected_ranges(body, /* include_headings */ false);
    let mut spans: Vec<WikilinkSpan> = Vec::new();
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
            let target_part = inner.find('|').map_or(inner, |pipe| &inner[..pipe]);
            if !target_part.trim().is_empty() {
                let address_part = target_part
                    .find('#')
                    .map_or(target_part, |hash| &target_part[..hash]);
                let target_trimmed = target_part.trim();
                let target_lead = target_part.len() - target_part.trim_start().len();
                let target =
                    after_open + target_lead..after_open + target_lead + target_trimmed.len();
                let trimmed = address_part.trim();
                let address = if trimmed.is_empty() {
                    after_open..after_open
                } else {
                    let lead = address_part.len() - address_part.trim_start().len();
                    after_open + lead..after_open + lead + trimmed.len()
                };
                // An `![[...]]` embed carries a `!` immediately before the
                // opening `[[`. The scan above only advanced through ASCII
                // `[`/`]`, so `start - 1` lands on a UTF-8 boundary.
                let is_embed = start > 0 && body.as_bytes()[start - 1] == b'!';
                spans.push(WikilinkSpan {
                    open: start,
                    address,
                    target,
                    inner_end,
                    is_embed,
                });
            }
        }
        cursor = inner_end + 2;
    }

    spans
}

/// Extract every `[[target]]` and `[[target|label]]` wikilink from a
/// markdown body, in source order. Skips code blocks, code spans, and
/// HTML. Wikilinks that span newlines are dropped — they're almost
/// certainly the user accidentally splitting one across lines, and
/// scanning past line breaks is more likely to glue unrelated tokens
/// together than to recover the user's intent.
pub fn extract_wikilinks(body: &str) -> Vec<WikilinkRaw> {
    wikilink_spans(body)
        .into_iter()
        .map(|span| {
            let inner = &body[span.open + 2..span.inner_end];
            let (target, label) = match inner.find('|') {
                Some(pipe) => (
                    inner[..pipe].trim().to_string(),
                    Some(inner[pipe + 1..].trim().to_string()),
                ),
                None => (inner.trim().to_string(), None),
            };
            WikilinkRaw {
                target,
                label: label.filter(|s| !s.is_empty()),
                is_embed: span.is_embed,
            }
        })
        .collect()
}

/// One replacement made by [`rewrite_wikilink_addresses`]: the byte
/// range of the address in the original text and what now stands there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressEdit {
    pub range: Range<usize>,
    pub replacement: String,
}

/// Rewrite the address of every wikilink in `text` whose full target
/// `names_target` accepts, returning the new text and the edits made
/// (ranges are into `text`, ascending, non-overlapping).
///
/// `names_target` receives the target as the resolver sees it: the text
/// before the first `|`, trimmed, anchor included (`old#Current State`).
/// `rewrite` receives the address only, and only the address bytes are
/// replaced; the anchor, label, the `!` of an embed, whitespace inside
/// the brackets, code spans, code blocks and HTML stay as they were. A
/// link with an empty address (`[[#Heading]]`, a same-note reference) is
/// never rewritten. `rewrite` must return a bare address: non-empty, with
/// no `#`, `|`, brackets or line breaks, or the link's structure changes.
pub fn rewrite_wikilink_addresses(
    text: &str,
    names_target: impl Fn(&str) -> bool,
    rewrite: impl Fn(&str) -> String,
) -> (String, Vec<AddressEdit>) {
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut edits = Vec::new();
    for span in wikilink_spans(text) {
        let address = &text[span.address.clone()];
        if address.is_empty() || !names_target(&text[span.target.clone()]) {
            continue;
        }
        let replacement = rewrite(address);
        out.push_str(&text[copied..span.address.start]);
        out.push_str(&replacement);
        copied = span.address.end;
        edits.push(AddressEdit {
            range: span.address,
            replacement,
        });
    }
    out.push_str(&text[copied..]);
    (out, edits)
}

/// [`rewrite_wikilink_addresses`] by the note's format: a `.typ` path is scanned by the Typst
/// scanner (RFC 0008 §5.5), `names_target` and `rewrite` receive the decoded target and address,
/// and the replacement is written with the escape rule of where the link sits, a `<cdno>` block
/// string or markup. The edit ranges are raw bytes of `text`, so [`differs_only_by`] holds as for
/// Markdown. A `[[` the Typst scanner reports as a fault is not a link and is left alone.
pub fn rewrite_note_wikilink_addresses(
    path: &VaultPath,
    text: &str,
    names_target: impl Fn(&str) -> bool,
    rewrite: impl Fn(&str) -> String,
) -> (String, Vec<AddressEdit>) {
    if note_format(path) != Some(NoteFormat::Typst) {
        return rewrite_wikilink_addresses(text, names_target, rewrite);
    }
    let links = typst::wikilink_spans(text);
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut edits = Vec::new();
    for (i, span) in links.spans.iter().enumerate() {
        let address = &links.addresses[i];
        if address.is_empty() || !names_target(&links.targets[i]) {
            continue;
        }
        let replacement = typst::escape_link_address(&rewrite(address), links.in_block[i]);
        out.push_str(&text[copied..span.address.start]);
        out.push_str(&replacement);
        copied = span.address.end;
        edits.push(AddressEdit {
            range: span.address.clone(),
            replacement,
        });
    }
    out.push_str(&text[copied..]);
    (out, edits)
}

/// True exactly when `after` equals `before` with `edits` applied in
/// order. The ranges must be ascending and non-overlapping, in bounds and
/// on char boundaries; otherwise the answer is false.
pub fn differs_only_by(before: &str, after: &str, edits: &[AddressEdit]) -> bool {
    let mut expected = String::with_capacity(after.len());
    let mut at = 0;
    for edit in edits {
        if edit.range.start < at
            || edit.range.end < edit.range.start
            || edit.range.end > before.len()
            || !before.is_char_boundary(edit.range.start)
            || !before.is_char_boundary(edit.range.end)
        {
            return false;
        }
        expected.push_str(&before[at..edit.range.start]);
        expected.push_str(&edit.replacement);
        at = edit.range.end;
    }
    expected.push_str(&before[at..]);
    expected == after
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
///    stewardship) is named by its folder path. A folder note at the vault
///    root is an exact path match for its bare name, so the precedence of
///    rule 5 does not apply to it; a bare link to a nested folder note
///    belongs to rule 5. Ordered before the fuzzy stem match so a folder link
///    can't be hijacked by an unrelated note sharing its last segment.
/// 3. Last-segment match: the target's final path segment against note
///    stems — `[[foo]]` or `[[actions/foo]]` → `*/foo.md` if exactly
///    one path has the stem `foo`. This resolves a link to a note that
///    relocated within its tree (e.g. an action archived to
///    `actions/_done/<year>/`); a unique match is required (#215).
/// 4. Alias match (RFC 0007 §5.5), only when rule 3 found **no** note with
///    the stem: the same last segment (anchor stripped, every folder segment
///    before it ignored, exactly as rule 3 ignores them — so
///    `[[projects/old]]` resolves like `[[old]]`) against the slug-shaped
///    `aliases:` entries in `aliases`, resolving when exactly one note in
///    `vault_paths` declares it ([`AliasMap::unique`]). A stem match, unique
///    or ambiguous, always wins over an alias, and an alias two notes share
///    resolves to nothing. The entry is marked [`LinkEntry::via_alias`].
/// 5. Bare folder-note name (#538), after every rule above: a target with no
///    `/` (anchor stripped) resolves to `<dir>/<target>/_index.md` when
///    exactly one such folder note exists, so `[[<portfolio-slug>]]` reaches
///    its portfolio. A stem or alias match always wins, so only a link that
///    resolved to nothing changes; a flat note and a folder note sharing a
///    name is not an error (a question and its portfolio share a slug), the
///    flat note keeps the bare name. Direct, not `via_alias`. A folder note
///    at the vault root is an exact path match (rule 1b) for its name, so
///    this precedence does not apply to it.
/// 6. Otherwise `resolved_path` is `None` and the link is recorded as
///    broken — `cdno lint` surfaces it as a warning.
pub fn resolve_wikilinks(
    raws: Vec<WikilinkRaw>,
    vault_paths: &HashSet<VaultPath>,
    aliases: &AliasMap,
) -> Vec<LinkEntry> {
    raws.into_iter()
        .map(|raw| {
            let resolved = resolve_one(&raw.target, vault_paths, aliases);
            let via_alias = resolved.as_ref().is_some_and(|r| r.via_alias);
            LinkEntry {
                target_raw: raw.target,
                resolved_path: resolved.map(|r| r.path),
                label: raw.label,
                via_alias,
            }
        })
        .collect()
}

/// The slug-shaped aliases notes declare (RFC 0007 §5.5), keyed by alias,
/// as resolution reads them.
///
/// Built from [`VaultIndex::list_aliases`](crate::index::VaultIndex::list_aliases)
/// rows by [`AliasMap::from_rows`]. An alias exactly one note declares maps
/// to that note; one that two or more notes declare maps to nothing, so a
/// shared alias is unresolvable by construction rather than by a check every
/// reader must remember. A note that lists the same alias twice still counts
/// as one note.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AliasMap(HashMap<String, Option<VaultPath>>);

impl AliasMap {
    /// Fold `(alias, path)` rows (one per occurrence, as `list_aliases`
    /// returns them) into the map: `Some(path)` when every row for the
    /// alias names the same note, `None` once a second note declares it.
    pub fn from_rows(rows: impl IntoIterator<Item = (String, VaultPath)>) -> Self {
        let mut map: HashMap<String, Option<VaultPath>> = HashMap::new();
        for (alias, path) in rows {
            match map.get_mut(&alias) {
                None => {
                    map.insert(alias, Some(path));
                }
                Some(holder) => {
                    if holder.as_ref() != Some(&path) {
                        *holder = None;
                    }
                }
            }
        }
        Self(map)
    }

    /// The one note declaring `name` as an alias, or `None` when no note
    /// does or when two or more share it.
    pub fn unique(&self, name: &str) -> Option<&VaultPath> {
        self.0.get(name).and_then(Option::as_ref)
    }

    /// Whether two or more notes declare `name`, so it resolves to nothing.
    pub fn is_shared(&self, name: &str) -> bool {
        matches!(self.0.get(name), Some(None))
    }

    /// Whether no note declares any alias.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Where [`resolve_one`] resolved a target, and whether the alias rule
/// (rule 4 of [`resolve_wikilinks`]) was the one that matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolution {
    pub(crate) path: VaultPath,
    pub(crate) via_alias: bool,
}

impl Resolution {
    fn direct(path: VaultPath) -> Self {
        Self {
            path,
            via_alias: false,
        }
    }
}

pub(crate) fn resolve_one(
    target: &str,
    vault_paths: &HashSet<VaultPath>,
    aliases: &AliasMap,
) -> Option<Resolution> {
    // 0a. Exact path match on the unsplit target, before the anchor is
    // stripped. A `#` in a filename is otherwise unsupported — it is only
    // reachable through an exact-path link — because rule 0b below would
    // otherwise always treat it as an anchor separator and mis-resolve
    // `notes/c#-notes` to `notes/c` if that also exists.
    if let Some(vp) = crate::paths::note_format_candidates(target)
        .into_iter()
        .find(|vp| vault_paths.contains(vp))
    {
        return Some(Resolution::direct(vp));
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
    // Rules 1 and 1b are one walk over `note_candidates`, which lists the
    // formats first and the folder index last.
    if let Some(vp) = crate::paths::note_candidates(target)
        .into_iter()
        .find(|vp| vault_paths.contains(vp))
    {
        return Some(Resolution::direct(vp));
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
    if let Some(first) = matches.next() {
        if matches.next().is_some() {
            // Ambiguous — multiple notes share the basename.
            return None;
        }
        return Some(Resolution::direct(first.clone()));
    }

    // 3. Alias match (RFC 0007 §5.5): reached only when no note has the
    // stem, so a stem — unique or ambiguous — always wins over an alias and
    // an alias can never move a link that resolves today. The same `needle`
    // as rule 2, so the folder segments before it are ignored here exactly
    // as they are there. A shared alias has no unique holder and resolves
    // to nothing. The holder must be in the path set, like every other
    // rule's answer, so a map read moments apart from the paths cannot
    // resolve to a note the set no longer has.
    // An alias two notes share resolves to nothing, and does not fall
    // through to the folder-note rule below.
    if aliases.is_shared(needle) {
        return None;
    }
    if let Some(holder) = aliases
        .unique(needle)
        .filter(|holder| vault_paths.contains(*holder))
    {
        return Some(Resolution {
            path: holder.clone(),
            via_alias: true,
        });
    }

    // 4. Bare folder-note name (#538): a target with no `/` resolves to the
    // one folder note `<dir>/<target>/_index.md`, so `[[<portfolio-slug>]]`
    // and `[[<stewardship-slug>]]` reach the note they are called. Last, so
    // only a link that resolves to nothing today can change: a stem match
    // (unique or ambiguous, returned above) and an alias both win. A
    // qualified target gains nothing; rule 1b already covers real paths.
    // Two folder notes with the name leave the link unresolved. The
    // predicate is shared with re-resolution (`is_bare_folder_name_link`).
    let mut folders = vault_paths
        .iter()
        .filter(|p| is_bare_folder_name_link(p, target));
    if let Some(only) = folders.next()
        && folders.next().is_none()
    {
        return Some(Resolution::direct(only.clone()));
    }
    None
}

/// Whether a link row resolved to `path` is a bare link to that folder note
/// by its folder name (the last rule of [`resolve_wikilinks`], #538). Such a
/// row ranks below the stem and alias rules but is recorded as a direct
/// resolution, so re-resolution has to reconsider it explicitly when a
/// path or an alias appears or goes. Anchor stripped, like the rule.
pub fn is_bare_folder_name_link(path: &VaultPath, target_raw: &str) -> bool {
    let p = path.as_path();
    // Markdown on purpose: a folder note is `_index.md`.
    if p.file_name().and_then(|f| f.to_str()) != Some("_index.md") {
        return false;
    }
    let target = target_raw.split_once('#').map_or(target_raw, |(t, _)| t);
    let target = target.trim_end();
    // A qualified target contains a `/`, which no single folder name does.
    p.parent()
        .and_then(|d| d.file_name())
        .and_then(|f| f.to_str())
        == Some(target)
}

/// The names a note answers to under [`resolve_wikilinks`]'s rules: its
/// file stem always (the exact rule, e.g. `[[portfolios/<slug>/_index]]`,
/// and the stem rule), and for a folder note (`<dir>/_index.md`) also the
/// folder's name, when it has one (the folder-index rule, e.g.
/// `[[portfolios/<slug>]]`, and the bare-folder-name rule, `[[<slug>]]`).
/// Every rule that can resolve a target to
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
/// its folder's name (also the bare-folder-name rule's), and a stem ambiguity is
/// only cleared by removing a note with the same stem. The alias rule ends
/// on the same anchor-stripped last segment, so an alias name in `names`
/// (one whose holder changed) admits every target that rule could resolve
/// through it. Both the unsplit target (rule 0a, for a `#` in a file name)
/// and the anchor-stripped one are checked.
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
