use std::collections::HashSet;

use std::ops::Range;

use cdno_core::extractors::{
    AddressEdit, AliasMap, WikilinkRaw, differs_only_by, extract_frontmatter_wikilinks,
    extract_inline_tags, extract_wikilinks, resolve_wikilinks, rewrite_wikilink_addresses,
    wikilink_spans,
};
use cdno_core::path::VaultPath;
use proptest::prelude::*;

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
            is_embed: false,
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
            is_embed: false,
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
            is_embed: false,
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
            is_embed: false,
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
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].resolved_path.as_ref(), Some(&vp("projects/foo.md")));
}

#[test]
fn resolve_strips_heading_anchor() {
    let vault = paths(&["projects/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "projects/foo#Some Heading".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(got[0].resolved_path.as_ref(), Some(&vp("projects/foo.md")));
    assert_eq!(got[0].target_raw, "projects/foo#Some Heading");
}

#[test]
fn resolve_strips_empty_anchor() {
    let vault = paths(&["projects/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "projects/foo#".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(got[0].resolved_path.as_ref(), Some(&vp("projects/foo.md")));
}

#[test]
fn resolve_strips_block_anchor() {
    let vault = paths(&["projects/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "projects/foo#^abc123".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(got[0].resolved_path.as_ref(), Some(&vp("projects/foo.md")));
}

#[test]
fn resolve_anchor_on_folder_target_hits_index_note() {
    let vault = paths(&["portfolios/topology/_index.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "portfolios/topology#Summary".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(
        got[0].resolved_path.as_ref(),
        Some(&vp("portfolios/topology/_index.md"))
    );
}

#[test]
fn resolve_same_note_anchor_is_none() {
    let vault = paths(&["notes/Heading.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "#Heading".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert!(got[0].resolved_path.is_none(), "got: {:?}", got[0]);
}

#[test]
fn resolve_anchor_does_not_break_last_segment_fallback() {
    let vault = paths(&["actions/_done/2026/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "actions/foo#Manifest".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(
        got[0].resolved_path.as_ref(),
        Some(&vp("actions/_done/2026/foo.md"))
    );
}

#[test]
fn resolve_exact_path_with_hash_in_filename_wins_over_anchor_split() {
    let vault = paths(&["notes/c#-notes.md", "notes/c.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "notes/c#-notes".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(
        got[0].resolved_path.as_ref(),
        Some(&vp("notes/c#-notes.md"))
    );
}

#[test]
fn resolve_anchor_with_label_keeps_label() {
    let vault = paths(&["projects/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "projects/foo#Some Heading".to_string(),
            label: Some("Label".to_string()),
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(got[0].resolved_path.as_ref(), Some(&vp("projects/foo.md")));
    assert_eq!(got[0].label, Some("Label".to_string()));
}

#[test]
fn resolve_trims_space_before_anchor() {
    let vault = paths(&["projects/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "projects/foo #Heading".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(got[0].resolved_path.as_ref(), Some(&vp("projects/foo.md")));
}

#[test]
fn resolve_picks_unique_basename_when_no_exact_match() {
    let vault = paths(&["notes/foo.md", "other/bar.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "foo".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
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
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
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
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
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
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
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
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
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
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(got[0].resolved_path.as_ref(), Some(&vp("actions/foo.md")));
}

#[test]
fn resolve_folder_target_to_its_index_note() {
    // A `[[portfolios/<slug>]]` link (the form the daily-log writer and
    // `file_to_portfolio` emit) names a folder, not a flat note — it must
    // resolve to that folder's `_index.md`. Without this rule the link is
    // dead: no `<target>.md` exists and the folder segment never matches
    // the `_index` stem.
    let vault = paths(&[
        "portfolios/topology/_index.md",
        "portfolios/topology/2026-07-13-study.md",
    ]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "portfolios/topology".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(
        got[0].resolved_path.as_ref(),
        Some(&vp("portfolios/topology/_index.md"))
    );
}

#[test]
fn resolve_folder_target_generalises_beyond_portfolios() {
    // The folder-index rule is general, not portfolio-special: an expanded
    // stewardship also lives at `<slug>/_index.md`, so `[[stewardships/x]]`
    // resolves to its index note the same way.
    let vault = paths(&[
        "stewardships/reading-group/_index.md",
        "stewardships/reading-group/tracking/2026-07.md",
    ]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "stewardships/reading-group".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(
        got[0].resolved_path.as_ref(),
        Some(&vp("stewardships/reading-group/_index.md"))
    );
}

#[test]
fn resolve_prefers_flat_note_over_folder_index() {
    // A flat `<target>.md` (rule 1) wins over a same-named folder index —
    // the exact path match is the more specific target.
    let vault = paths(&["notes/topology.md", "notes/topology/_index.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "notes/topology".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(
        got[0].resolved_path.as_ref(),
        Some(&vp("notes/topology.md"))
    );
}

#[test]
fn resolve_prefers_folder_index_over_last_segment_fallback() {
    // The folder-index rule is ordered before the fuzzy stem match: a
    // `[[portfolios/foo]]` folder link resolves to its index note, never to
    // an unrelated `elsewhere/foo.md` that merely shares the last segment.
    let vault = paths(&["portfolios/foo/_index.md", "elsewhere/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "portfolios/foo".to_string(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(
        got[0].resolved_path.as_ref(),
        Some(&vp("portfolios/foo/_index.md"))
    );
}

#[test]
fn resolve_preserves_label_through_resolution() {
    let vault = paths(&["notes/foo.md"]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "foo".to_string(),
            label: Some("My Foo".to_string()),
            is_embed: false,
        }],
        &vault,
        &AliasMap::default(),
    );
    assert_eq!(got[0].label.as_deref(), Some("My Foo"));
    assert_eq!(got[0].resolved_path.as_ref(), Some(&vp("notes/foo.md")));
}

// ── the alias rule (RFC 0007 §5.5) ───────────────────────────────────

fn aliases(rows: &[(&str, &str)]) -> AliasMap {
    AliasMap::from_rows(rows.iter().map(|(a, p)| ((*a).to_owned(), vp(p))))
}

/// Resolve one target and return `(resolved_path, via_alias)`.
fn resolve_target(
    target: &str,
    vault: &HashSet<VaultPath>,
    map: &AliasMap,
) -> (Option<VaultPath>, bool) {
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: target.to_owned(),
            label: None,
            is_embed: false,
        }],
        vault,
        map,
    );
    (got[0].resolved_path.clone(), got[0].via_alias)
}

#[test]
fn an_alias_resolves_only_when_no_stem_matches() {
    // Alias `old` on `projects/new.md`: `[[old]]` resolves to it.
    let mut vault = paths(&["projects/new.md"]);
    let map = aliases(&[("old", "projects/new.md")]);
    assert_eq!(
        resolve_target("old", &vault, &map).0,
        Some(vp("projects/new.md"))
    );

    // A note with the stem `old` appears: the stem wins.
    vault.insert(vp("notes/old.md"));
    assert_eq!(
        resolve_target("old", &vault, &map).0,
        Some(vp("notes/old.md"))
    );

    // Without the stem, a second note declaring `old` makes the alias
    // shared, and a shared alias resolves to nothing.
    vault.remove(&vp("notes/old.md"));
    vault.insert(vp("projects/other.md"));
    let shared = aliases(&[("old", "projects/new.md"), ("old", "projects/other.md")]);
    assert_eq!(resolve_target("old", &vault, &shared), (None, false));
}

#[test]
fn an_alias_resolution_is_marked() {
    let vault = paths(&["projects/new.md", "notes/stem.md"]);
    let map = aliases(&[("old", "projects/new.md")]);
    assert_eq!(
        resolve_target("old", &vault, &map),
        (Some(vp("projects/new.md")), true)
    );
    // Exact path, stem, and unresolved: never marked.
    assert_eq!(
        resolve_target("projects/new", &vault, &map),
        (Some(vp("projects/new.md")), false)
    );
    assert_eq!(
        resolve_target("stem", &vault, &map),
        (Some(vp("notes/stem.md")), false)
    );
    assert_eq!(resolve_target("missing", &vault, &map), (None, false));
}

#[test]
fn an_alias_never_beats_an_ambiguous_stem() {
    // Two notes have the stem `old`; one note declares the alias `old`. The
    // stem rule found matches, so the alias rule is never reached.
    let vault = paths(&["notes/one/old.md", "notes/two/old.md", "projects/new.md"]);
    let map = aliases(&[("old", "projects/new.md")]);
    assert_eq!(resolve_target("old", &vault, &map), (None, false));
}

#[test]
fn an_alias_with_an_anchor_resolves() {
    let vault = paths(&["projects/new.md"]);
    let map = aliases(&[("old", "projects/new.md")]);
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: "old#Heading".to_owned(),
            label: None,
            is_embed: false,
        }],
        &vault,
        &map,
    );
    assert_eq!(got[0].resolved_path, Some(vp("projects/new.md")));
    assert!(got[0].via_alias);
    assert_eq!(
        got[0].target_raw, "old#Heading",
        "the anchor stays on the entry"
    );
}

#[test]
fn a_folder_segment_before_an_alias_is_ignored_as_the_stem_rule_ignores_it() {
    // The stem rule matches the target's last segment whatever folders
    // precede it (`[[actions/foo]]` finds `actions/_done/2026/foo.md`, and
    // equally `elsewhere/foo.md`). The alias rule reads the same segment, so
    // `[[projects/old]]` resolves through the alias `old` exactly as
    // `[[old]]` does — and as `[[anywhere/old]]` does.
    let vault = paths(&["projects/new.md"]);
    let map = aliases(&[("old", "projects/new.md")]);
    for target in ["projects/old", "anywhere/old", "old"] {
        assert_eq!(
            resolve_target(target, &vault, &map),
            (Some(vp("projects/new.md")), true),
            "{target}"
        );
    }
    // Mirrors the stem rule on the same shapes.
    let stems = paths(&["notes/old.md"]);
    assert_eq!(
        resolve_target("projects/old", &stems, &AliasMap::default()).0,
        Some(vp("notes/old.md"))
    );
}

#[test]
fn an_alias_whose_holder_is_not_in_the_path_set_does_not_resolve() {
    // Every rule answers with a path from the set; the alias rule too.
    let vault = paths(&["notes/other.md"]);
    let map = aliases(&[("old", "projects/new.md")]);
    assert_eq!(resolve_target("old", &vault, &map), (None, false));
}

#[test]
fn an_alias_one_note_lists_twice_is_still_unique() {
    let map = aliases(&[("old", "projects/new.md"), ("old", "projects/new.md")]);
    assert_eq!(map.unique("old"), Some(&vp("projects/new.md")));
    let shared = aliases(&[
        ("old", "projects/new.md"),
        ("old", "projects/other.md"),
        ("old", "projects/new.md"),
    ]);
    assert_eq!(shared.unique("old"), None);
}

#[test]
fn extract_wikilinks_marks_an_embed() {
    // `![[...]]` is an embed; the `!` immediately before the `[[` is the
    // only difference from a plain link.
    assert_eq!(
        extract_wikilinks("![[assets/img.png]]"),
        vec![WikilinkRaw {
            target: "assets/img.png".to_string(),
            label: None,
            is_embed: true,
        }],
    );
}

#[test]
fn extract_wikilinks_a_plain_link_is_not_an_embed() {
    assert!(!extract_wikilinks("[[other-note]]")[0].is_embed);
}

#[test]
fn extract_wikilinks_a_bang_not_touching_the_brackets_is_not_an_embed() {
    // "no! [[note]]" — the `!` is punctuation, not an embed marker.
    assert!(!extract_wikilinks("no! [[note]]")[0].is_embed);
}

// ── rewrite_wikilink_addresses (RFC 0007 §5.4) ───────────────────────

/// Names the note `old` by its full target, anchor included, as the
/// resolver would see it.
fn names_old(target: &str) -> bool {
    let a = target.split('#').next().unwrap_or(target).trim();
    a == "old" || a.ends_with("/old")
}

fn swap_last_segment(a: &str) -> String {
    match a.rfind('/') {
        Some(i) => format!("{}/new", &a[..i]),
        None => "new".to_string(),
    }
}

#[test]
fn address_rewrites_keep_every_other_byte() {
    let rows: &[(&str, &str)] = &[
        ("[[old]]", "[[new]]"),
        ("[[projects/old]]", "[[projects/new]]"),
        ("[[projects/_parked/old]]", "[[projects/_parked/new]]"),
        ("[[old#Current State]]", "[[new#Current State]]"),
        ("[[old|the calibration]]", "[[new|the calibration]]"),
        ("![[old]]", "![[new]]"),
        ("`[[old]]`", "`[[old]]`"),
        ("```\n[[old]]\n```\n", "```\n[[old]]\n```\n"),
        ("#action/old", "#action/old"),
        ("[[ old ]]", "[[ new ]]"),
        (
            "[[ old #x | the calibration ]]",
            "[[ new #x | the calibration ]]",
        ),
        ("[[other]]", "[[other]]"),
        (
            "see [[old]] and [[other]] and [[a/old|x]]",
            "see [[new]] and [[other]] and [[a/new|x]]",
        ),
    ];
    let mut failures: Vec<String> = Vec::new();
    for (before, want) in rows {
        let (after, edits) = rewrite_wikilink_addresses(before, names_old, swap_last_segment);
        if &after != want {
            failures.push(format!("row {before:?}: got {after:?}, want {want:?}"));
        }
        if !differs_only_by(before, &after, &edits) {
            failures.push(format!("row {before:?}: edits do not explain the change"));
        }
    }
    assert!(
        failures.is_empty(),
        "failing rows:\n{}",
        failures.join("\n")
    );
}

#[test]
fn a_rewrite_reports_its_count() {
    let text = "[[old]] [[other]] ![[x/old#h]] `[[old]]` [[#old]]";
    let (_, edits) = rewrite_wikilink_addresses(text, names_old, swap_last_segment);
    assert_eq!(edits.len(), 2);
    let (same, none) = rewrite_wikilink_addresses("[[other]]", names_old, swap_last_segment);
    assert_eq!((same.as_str(), none.len()), ("[[other]]", 0));
}

#[test]
fn spans_locate_the_trimmed_address() {
    let body = "a ![[ x/y #h|l]]";
    let spans = wikilink_spans(body);
    assert_eq!(spans.len(), 1);
    assert_eq!(&body[spans[0].address.clone()], "x/y");
    assert_eq!(spans[0].open, 3);
    assert_eq!(&body[spans[0].inner_end..], "]]");
    assert!(spans[0].is_embed);
}

fn edit(range: Range<usize>, replacement: &str) -> AddressEdit {
    AddressEdit {
        range,
        replacement: replacement.to_string(),
    }
}

#[test]
fn the_predicate_sees_the_full_target_with_its_anchor() {
    let seen = std::cell::RefCell::new(Vec::new());
    let text = "[[ notes/c #-notes | l ]] [[#h]] [[x]]";
    rewrite_wikilink_addresses(
        text,
        |t| {
            seen.borrow_mut().push(t.to_string());
            false
        },
        |a| a.to_string(),
    );
    assert_eq!(
        *seen.borrow(),
        vec!["notes/c #-notes".to_string(), "x".to_string()]
    );
    let spans = wikilink_spans("[[old#Current State]]");
    assert_eq!(
        &"[[old#Current State]]"[spans[0].target.clone()],
        "old#Current State"
    );
}

#[test]
fn differs_only_by_rejects_a_change_outside_an_edit() {
    let before = "one [[old]] two";
    let edits = [edit(6..9, "new")];
    assert!(!differs_only_by(before, "ONE [[new]] two", &edits));
    assert!(!differs_only_by(before, "one [[new]] twO", &edits));
    assert!(!differs_only_by(before, "one [new]] two", &edits));
    assert!(!differs_only_by(before, "one [[new]] two", &[]));
    // Overlapping, out-of-order or out-of-bounds edits are false.
    let overlap = [edit(6..9, "a"), edit(8..10, "b")];
    assert!(!differs_only_by(before, "one [[ab]] two", &overlap));
    let reversed = [edit(8..9, "a"), edit(6..7, "b")];
    assert!(!differs_only_by(before, "one [[bda]] two", &reversed));
    assert!(!differs_only_by(before, before, &[edit(6..99, "")]));
}

#[test]
fn differs_only_by_rejects_duplicated_text_around_an_edit() {
    assert!(!differs_only_by(
        "[[old]] and [[old]]",
        "[[new]] and [[old]] and [[old]]",
        &[edit(2..5, "new")]
    ));
    assert!(!differs_only_by(
        "a [[old]] b",
        "a [[x]] DELETED [[y]] b",
        &[edit(4..7, "x")]
    ));
}

#[test]
fn differs_only_by_accepts_exactly_the_edits() {
    let before = "one [[old]] two";
    assert!(differs_only_by(
        before,
        "one [[a-much-longer-slug]] two",
        &[edit(6..9, "a-much-longer-slug")]
    ));
    assert!(differs_only_by(before, "one [[]] two", &[edit(6..9, "")]));
    assert!(differs_only_by(before, before, &[]));
    assert!(differs_only_by(
        "a[[x]]b[[y]]c",
        "a[[xx]]b[[z]]c",
        &[edit(3..4, "xx"), edit(9..10, "z")]
    ));
    assert!(!differs_only_by(
        "a[[x]]b[[y]]c",
        "a[[xx]]B[[z]]c",
        &[edit(3..4, "xx"), edit(9..10, "z")]
    ));
}

fn piece() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("prose words".to_string()),
        Just("old".to_string()),
        Just("[[old]]".to_string()),
        Just("[[slug]]".to_string()),
        Just("[[dir/slug]]".to_string()),
        Just("[[dir/old]]".to_string()),
        Just("[[old#anchor]]".to_string()),
        Just("[[slug#anchor]]".to_string()),
        Just("[[#h]]".to_string()),
        Just("[[slug #h]]".to_string()),
        Just("[[old|label]]".to_string()),
        Just("[[ dir/old | label ]]".to_string()),
        Just("[[slug|label]]".to_string()),
        Just("![[old]]".to_string()),
        Just("![[slug]]".to_string()),
        Just("`[[old]]`".to_string()),
        Just("```\n[[old]]\n```".to_string()),
        Just("[[".to_string()),
    ]
}

fn body() -> impl Strategy<Value = String> {
    proptest::collection::vec((piece(), prop_oneof![Just(" "), Just("\n")]), 0..12).prop_map(
        |parts| {
            parts
                .into_iter()
                .map(|(p, sep)| format!("{p}{sep}"))
                .collect::<String>()
        },
    )
}

/// Swap the address part of a `WikilinkRaw.target`, keeping its anchor.
fn map_raw(raw: &WikilinkRaw) -> WikilinkRaw {
    let (address, anchor) = match raw.target.find('#') {
        Some(i) => raw.target.split_at(i),
        None => (raw.target.as_str(), ""),
    };
    let trimmed = address.trim_end();
    // Whitespace between the address and the `#` sits outside the address
    // and survives the rewrite.
    let gap = &address[trimmed.len()..];
    let mut out = raw.clone();
    if names_old(trimmed) {
        out.target = format!("{}{}{}", swap_last_segment(trimmed), gap, anchor);
    }
    out
}

proptest! {
    #[test]
    fn rewriting_then_extracting_equals_extracting_then_mapping(text in body()) {
        let (after, edits) = rewrite_wikilink_addresses(&text, names_old, swap_last_segment);
        let expected: Vec<WikilinkRaw> = extract_wikilinks(&text).iter().map(map_raw).collect();
        prop_assert_eq!(extract_wikilinks(&after), expected);
        prop_assert!(differs_only_by(&text, &after, &edits));
        // Edits built independently of the call that returned them.
        let expected_edits: Vec<AddressEdit> = wikilink_spans(&text)
            .into_iter()
            .filter(|s| !text[s.address.clone()].is_empty() && names_old(&text[s.target.clone()]))
            .map(|s| AddressEdit {
                replacement: swap_last_segment(&text[s.address.clone()]),
                range: s.address,
            })
            .collect();
        prop_assert!(differs_only_by(&text, &after, &expected_edits));
        // Each returned edit's range is an accepted address span of the original.
        prop_assert_eq!(
            edits,
            expected_edits,
            "every edit's range must be an accepted address span of wikilink_spans(original)"
        );
    }
}

// ── bare folder-note names (#538) ────────────────────────────────────

fn resolve_bare(
    target: &str,
    vault: &HashSet<VaultPath>,
    aliases: &AliasMap,
) -> Option<(VaultPath, bool)> {
    let got = resolve_wikilinks(
        vec![WikilinkRaw {
            target: target.to_string(),
            label: None,
            is_embed: false,
        }],
        vault,
        aliases,
    );
    got[0].resolved_path.clone().map(|p| (p, got[0].via_alias))
}

#[test]
fn bare_name_resolves_to_a_portfolio_and_an_expanded_stewardship() {
    let vault = paths(&[
        "portfolios/surrogate-model/_index.md",
        "stewardships/lab-equipment/_index.md",
    ]);
    let none = AliasMap::default();
    assert_eq!(
        resolve_bare("surrogate-model", &vault, &none),
        Some((vp("portfolios/surrogate-model/_index.md"), false))
    );
    assert_eq!(
        resolve_bare("lab-equipment#Routine", &vault, &none),
        Some((vp("stewardships/lab-equipment/_index.md"), false))
    );
}

#[test]
fn a_stem_wins_over_a_folder_name() {
    let vault = paths(&[
        "questions/research/surrogate-model.md",
        "portfolios/surrogate-model/_index.md",
    ]);
    assert_eq!(
        resolve_bare("surrogate-model", &vault, &AliasMap::default()),
        Some((vp("questions/research/surrogate-model.md"), false))
    );
}

#[test]
fn a_stem_ambiguity_stays_unresolved_despite_a_folder_name() {
    let vault = paths(&[
        "projects/surrogate-model.md",
        "notes/surrogate-model.md",
        "portfolios/surrogate-model/_index.md",
    ]);
    assert_eq!(
        resolve_bare("surrogate-model", &vault, &AliasMap::default()),
        None
    );
}

#[test]
fn an_alias_wins_over_a_folder_name() {
    let vault = paths(&["projects/other.md", "portfolios/surrogate-model/_index.md"]);
    let aliases = AliasMap::from_rows([("surrogate-model".to_string(), vp("projects/other.md"))]);
    assert_eq!(
        resolve_bare("surrogate-model", &vault, &aliases),
        Some((vp("projects/other.md"), true))
    );
}

#[test]
fn two_folder_notes_with_one_name_stay_unresolved() {
    let vault = paths(&[
        "portfolios/surrogate-model/_index.md",
        "stewardships/surrogate-model/_index.md",
    ]);
    assert_eq!(
        resolve_bare("surrogate-model", &vault, &AliasMap::default()),
        None
    );
}

#[test]
fn a_qualified_target_gains_no_folder_name_match() {
    let vault = paths(&["portfolios/surrogate-model/_index.md"]);
    let none = AliasMap::default();
    assert_eq!(resolve_bare("other/surrogate-model", &vault, &none), None);
    assert_eq!(
        resolve_bare("portfolios/surrogate-model", &vault, &none),
        Some((vp("portfolios/surrogate-model/_index.md"), false))
    );
}

#[test]
fn a_shared_alias_does_not_fall_through_to_a_folder_note() {
    let vault = paths(&[
        "notes/a.md",
        "notes/b.md",
        "portfolios/surrogate-model/_index.md",
    ]);
    let aliases = AliasMap::from_rows([
        ("surrogate-model".to_string(), vp("notes/a.md")),
        ("surrogate-model".to_string(), vp("notes/b.md")),
    ]);
    assert_eq!(resolve_bare("surrogate-model", &vault, &aliases), None);
}

#[test]
fn is_bare_folder_name_link_pins_each_condition() {
    use cdno_core::extractors::is_bare_folder_name_link as f;
    let folder = vp("portfolios/surrogate-model/_index.md");
    assert!(f(&folder, "surrogate-model"));
    assert!(f(&folder, "surrogate-model#Evidence"));
    assert!(f(&folder, "surrogate-model "));
    // Not an `_index.md`.
    assert!(!f(
        &vp("portfolios/surrogate-model/notes.md"),
        "surrogate-model"
    ));
    // Qualified, or a different folder name.
    assert!(!f(&folder, "portfolios/surrogate-model"));
    assert!(!f(&folder, "other"));
    // A vault-root `_index.md` has no folder name.
    assert!(!f(&vp("_index.md"), "_index"));
    assert!(!f(&vp("_index.md"), ""));
}

fn resolve_one_target(vault: &HashSet<VaultPath>, target: &str) -> Option<VaultPath> {
    resolve_wikilinks(
        vec![WikilinkRaw {
            target: target.to_string(),
            label: None,
            is_embed: false,
        }],
        vault,
        &AliasMap::default(),
    )
    .remove(0)
    .resolved_path
}

#[test]
fn an_address_reaches_an_upper_case_md_note_through_the_stem_rule_only() {
    // `NOTE.MD` is a note (RFC 0008 5.1), but candidates are lower-case:
    // the exact-path rules never produce `NOTE.MD`, so `[[dir/NOTE]]`
    // arrives there by the last-segment match and nothing else does.
    let vault = paths(&["dir/NOTE.MD"]);
    assert_eq!(
        resolve_one_target(&vault, "dir/NOTE"),
        Some(vp("dir/NOTE.MD"))
    );
    assert_eq!(resolve_one_target(&vault, "dir/note"), None);
    // `_index.MD` is not a folder index.
    let vault = paths(&["portfolios/p/_index.MD"]);
    assert_eq!(resolve_one_target(&vault, "portfolios/p"), None);
}

#[test]
fn exact_path_candidates_run_markdown_then_typst_then_the_folder_index() {
    let all = paths(&["a/x.md", "a/x.typ", "a/x/_index.md"]);
    assert_eq!(resolve_one_target(&all, "a/x"), Some(vp("a/x.md")));
    let no_md = paths(&["a/x.typ", "a/x/_index.md"]);
    assert_eq!(resolve_one_target(&no_md, "a/x"), Some(vp("a/x.typ")));
    let folder = paths(&["a/x/_index.md", "b/x.md"]);
    assert_eq!(
        resolve_one_target(&folder, "a/x"),
        Some(vp("a/x/_index.md"))
    );
}
