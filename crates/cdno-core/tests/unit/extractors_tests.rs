use std::collections::HashSet;

use cdno_core::extractors::{
    WikilinkRaw, extract_frontmatter_wikilinks, extract_inline_tags, extract_wikilinks,
    resolve_wikilinks,
};
use cdno_core::path::VaultPath;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn paths(items: &[&str]) -> HashSet<VaultPath> {
    items.iter().map(|p| vp(p)).collect()
}

// ── extract_inline_tags ──────────────────────────────────────────────

#[test]
fn extract_tags_returns_empty_for_a_plain_body() {
    assert!(extract_inline_tags("nothing tagged here").is_empty());
}

#[test]
fn extract_tags_picks_up_a_simple_tag() {
    assert_eq!(extract_inline_tags("a thought #meeting"), vec!["meeting"]);
}

#[test]
fn extract_tags_dedupes_and_sorts() {
    let body = "before #beta and #alpha and #beta again";
    assert_eq!(extract_inline_tags(body), vec!["alpha", "beta"]);
}

#[test]
fn extract_tags_supports_underscores_and_hyphens() {
    assert_eq!(
        extract_inline_tags("#tag-with-hyphen #tag_with_underscore"),
        vec!["tag-with-hyphen", "tag_with_underscore"],
    );
}

#[test]
fn extract_tags_strips_trailing_punctuation() {
    assert_eq!(
        extract_inline_tags("ending #foo, then #bar."),
        vec!["bar", "foo"]
    );
}

#[test]
fn extract_tags_ignores_hash_inside_a_word() {
    assert!(extract_inline_tags("not#a-tag because no boundary").is_empty());
}

#[test]
fn extract_tags_ignores_double_hashes() {
    assert!(extract_inline_tags("##notatag").is_empty());
}

#[test]
fn extract_tags_skips_code_blocks() {
    let body = "before\n\n```\n#nope\n#also-nope\n```\n\nafter #real";
    assert_eq!(extract_inline_tags(body), vec!["real"]);
}

#[test]
fn extract_tags_skips_inline_code_spans() {
    let body = "look at `#nope` and #yes";
    assert_eq!(extract_inline_tags(body), vec!["yes"]);
}

#[test]
fn extract_tags_skips_headings() {
    let body = "## a heading with #nope inside\n\nbody #yes";
    assert_eq!(extract_inline_tags(body), vec!["yes"]);
}

#[test]
fn extract_tags_supports_digit_starts() {
    // `#[a-zA-Z0-9][a-zA-Z0-9_-]*` allows digits at the start.
    assert_eq!(extract_inline_tags("year #2026"), vec!["2026"]);
}

#[test]
fn extract_tags_handles_tag_at_start_of_body() {
    assert_eq!(extract_inline_tags("#first thing"), vec!["first"]);
}

#[test]
fn extract_tags_supports_namespaced_slash() {
    // The headline action-layer case (design §5.11): the slug is part
    // of the tag, so the whole `action/<slug>` is one queryable token.
    assert_eq!(
        extract_inline_tags("logged #action/characterise-sample-efficiency today"),
        vec!["action/characterise-sample-efficiency"],
    );
}

#[test]
fn extract_tags_supports_nested_namespaces() {
    assert_eq!(extract_inline_tags("#a/b/c"), vec!["a/b/c"]);
}

#[test]
fn extract_tags_trims_trailing_slash() {
    // A trailing slash isn't part of the tag: `#foo/` tags `foo`.
    assert_eq!(
        extract_inline_tags("ends with #foo/ then more"),
        vec!["foo"]
    );
}

#[test]
fn extract_tags_ignores_hash_in_url() {
    // The `#` in a URL fragment is preceded by a non-whitespace char,
    // so the boundary rule rejects it — no tag, no leaked `anchor`.
    assert!(extract_inline_tags("see https://example.com/page#anchor").is_empty());
}

// ── extract_wikilinks ────────────────────────────────────────────────

#[test]
fn extract_wikilinks_returns_empty_for_plain_body() {
    assert!(extract_wikilinks("just prose, nothing fancy").is_empty());
}

#[test]
fn extract_wikilinks_picks_up_a_simple_target() {
    assert_eq!(
        extract_wikilinks("see [[other-note]] for more"),
        vec![WikilinkRaw {
            target: "other-note".to_string(),
            label: None,
        }],
    );
}

#[test]
fn extract_wikilinks_separates_target_and_label() {
    assert_eq!(
        extract_wikilinks("[[other-note|My Other Note]]"),
        vec![WikilinkRaw {
            target: "other-note".to_string(),
            label: Some("My Other Note".to_string()),
        }],
    );
}

#[test]
fn extract_wikilinks_normalises_empty_label_to_none() {
    assert_eq!(
        extract_wikilinks("[[foo|]]"),
        vec![WikilinkRaw {
            target: "foo".to_string(),
            label: None,
        }],
    );
}

#[test]
fn extract_wikilinks_returns_multiple_in_source_order() {
    let body = "first [[a]] then [[b|B]] and finally [[c]]";
    let got = extract_wikilinks(body);
    let targets: Vec<&str> = got.iter().map(|w| w.target.as_str()).collect();
    assert_eq!(targets, vec!["a", "b", "c"]);
}

#[test]
fn extract_wikilinks_skips_code_blocks() {
    let body = "before\n\n```\n[[nope]]\n```\n\nafter [[real]]";
    let got = extract_wikilinks(body);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].target, "real");
}

#[test]
fn extract_wikilinks_skips_code_spans() {
    let body = "see `[[nope]]` and [[yes]]";
    let got = extract_wikilinks(body);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].target, "yes");
}

#[test]
fn extract_wikilinks_keeps_links_in_headings() {
    // Wikilinks in headings ARE kept — a reference is a reference,
    // even from a section title.
    let body = "## See also: [[other]]\n\nbody";
    let got = extract_wikilinks(body);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].target, "other");
}

#[test]
fn extract_wikilinks_skips_empty_brackets() {
    assert!(extract_wikilinks("[[]] is nothing").is_empty());
}

#[test]
fn extract_wikilinks_skips_links_spanning_newlines() {
    assert!(extract_wikilinks("[[foo\nbar]]").is_empty());
}

#[test]
fn extract_frontmatter_wikilinks_scans_string_values_only() {
    // A link-bearing scalar (core_question), a nested one, and an array —
    // all caught; non-link scalars and non-strings contribute nothing (#395).
    let fm = serde_json::json!({
        "type": "project",
        "status": "active",
        "created": "2026-04-01",
        "core_question": "[[questions/research/surrogate-cost]]",
        "collaborators": ["[[people/ada]]", "not a link"],
        "count": 3,
        "meta": { "origin": "[[portfolios/x/_index]]" },
    });
    let targets: Vec<String> = extract_frontmatter_wikilinks(&fm)
        .into_iter()
        .map(|w| w.target)
        .collect();
    assert!(targets.contains(&"questions/research/surrogate-cost".to_owned()));
    assert!(targets.contains(&"people/ada".to_owned()));
    assert!(targets.contains(&"portfolios/x/_index".to_owned()));
    // Non-link scalars (status/created/count) and the plain array string
    // yield nothing beyond the three wikilinks.
    assert_eq!(targets.len(), 3, "{targets:?}");
}

#[test]
fn extract_frontmatter_wikilinks_empty_when_no_links() {
    let fm = serde_json::json!({ "type": "action", "status": "active", "due": null });
    assert!(extract_frontmatter_wikilinks(&fm).is_empty());
}

#[test]
fn extract_wikilinks_handles_unclosed_brackets() {
    // `[[foo` with no `]]` — don't keep scanning past it for a stray
    // closer elsewhere in the document.
    assert!(extract_wikilinks("trailing [[foo without close").is_empty());
}

#[test]
fn extract_wikilinks_trims_whitespace_in_target_and_label() {
    assert_eq!(
        extract_wikilinks("[[  foo  |  Foo Label  ]]"),
        vec![WikilinkRaw {
            target: "foo".to_string(),
            label: Some("Foo Label".to_string()),
        }],
    );
}

// ── resolve_wikilinks ────────────────────────────────────────────────

#[test]
fn resolve_picks_exact_path_match() {
    let vault = paths(&["projects/foo.md", "notes/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "projects/foo".to_string(),
            label: None,
        }],
        &vault,
    );
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].resolved_path.as_ref(), Some(&vp("projects/foo.md")));
}

#[test]
fn resolve_picks_unique_basename_when_no_exact_match() {
    let vault = paths(&["notes/foo.md", "other/bar.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "foo".to_string(),
            label: None,
        }],
        &vault,
    );
    assert_eq!(got[0].resolved_path.as_ref(), Some(&vp("notes/foo.md")));
}

#[test]
fn resolve_returns_none_when_basename_is_ambiguous() {
    let vault = paths(&["a/foo.md", "b/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "foo".to_string(),
            label: None,
        }],
        &vault,
    );
    assert!(got[0].resolved_path.is_none(), "got: {:?}", got[0]);
}

#[test]
fn resolve_returns_none_when_no_match_at_all() {
    let vault = paths(&["a/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "missing".to_string(),
            label: None,
        }],
        &vault,
    );
    assert!(got[0].resolved_path.is_none());
    assert_eq!(got[0].target_raw, "missing");
}

#[test]
fn resolve_qualified_target_falls_back_to_a_relocated_note() {
    // #215: a `[[actions/<slug>]]` reference still resolves after the
    // note is archived to `actions/_done/<year>/<slug>.md` — the
    // last-segment fallback matches the stem.
    let vault = paths(&["actions/_done/2026/characterise.md", "projects/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "actions/characterise".to_string(),
            label: None,
        }],
        &vault,
    );
    assert_eq!(
        got[0].resolved_path.as_ref(),
        Some(&vp("actions/_done/2026/characterise.md"))
    );
}

#[test]
fn resolve_qualified_target_is_none_when_last_segment_is_ambiguous() {
    // The uniqueness guard still holds for the fallback: two notes with
    // the same stem leave the link unresolved.
    let vault = paths(&["a/foo.md", "b/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "actions/foo".to_string(),
            label: None,
        }],
        &vault,
    );
    assert!(got[0].resolved_path.is_none(), "got: {:?}", got[0]);
}

#[test]
fn resolve_prefers_exact_path_over_last_segment_fallback() {
    // An active note and an archived one share a slug; an
    // `[[actions/foo]]` link must resolve to the active note (exact
    // path), not the relocated copy.
    let vault = paths(&["actions/foo.md", "actions/_done/2026/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "actions/foo".to_string(),
            label: None,
        }],
        &vault,
    );
    assert_eq!(got[0].resolved_path.as_ref(), Some(&vp("actions/foo.md")));
}

#[test]
fn resolve_preserves_label_through_resolution() {
    let vault = paths(&["notes/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "foo".to_string(),
            label: Some("My Foo".to_string()),
        }],
        &vault,
    );
    assert_eq!(got[0].label.as_deref(), Some("My Foo"));
    assert_eq!(got[0].resolved_path.as_ref(), Some(&vp("notes/foo.md")));
}
