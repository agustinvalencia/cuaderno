use std::collections::HashSet;

use cdno_core::extractors::{
    WikilinkRaw, extract_inline_tags, extract_wikilinks, resolve_wikilinks,
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
