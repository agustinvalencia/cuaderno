use cdno_core::extractors::differs_only_by;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::frontmatter_edit::{
    DriftSide, FrontmatterDrift, append_frontmatter_list_value, frontmatter_changed_only_at,
    rewrite_frontmatter_scalar,
};
use proptest::prelude::*;

/// Append, and check the returned edit explains the whole change.
fn append(fm: &str, key: &str, value: &str) -> String {
    let (after, edits) = append_frontmatter_list_value(fm, key, value);
    assert!(differs_only_by(fm, &after, &edits), "{fm:?} -> {after:?}");
    assert_eq!(edits.is_empty(), after == fm);
    after
}

fn fixture(project_line: &str) -> String {
    format!(
        "title: \"Keep old\"\n# project: old\n{project_line}\nouter:\n  project: old\ntags: [old]\nlink: \"[[old]]\"\n"
    )
}

#[test]
fn scalar_rewrite_keeps_quoting_and_order() {
    for (line, want) in [
        ("project: old", "project: new"),
        ("project: 'old'", "project: 'new'"),
        ("project: \"old\"", "project: \"new\""),
    ] {
        let before = fixture(line);
        let (after, edits) = rewrite_frontmatter_scalar(&before, "project", "old", "new");
        assert_eq!(after, fixture(want));
        assert_eq!(edits.len(), 1);
        assert_eq!(&before[edits[0].range.clone()], "old");
        assert!(differs_only_by(&before, &after, &edits));
    }
}

#[test]
fn scalar_rewrite_misses_a_different_value() {
    let before = fixture("project: older");
    let (after, edits) = rewrite_frontmatter_scalar(&before, "project", "old", "new");
    assert!(edits.is_empty());
    assert_eq!(after, before);
}

#[test]
fn scalar_rewrite_with_a_trailing_comment_keeps_it() {
    let before = "project:   \"old\"   # keep me\nother: old\n";
    let (after, edits) = rewrite_frontmatter_scalar(before, "project", "old", "new");
    assert_eq!(after, "project:   \"new\"   # keep me\nother: old\n");
    assert!(differs_only_by(before, &after, &edits));
    let (after, _) = rewrite_frontmatter_scalar("project: old # c\r\n", "project", "old", "new");
    assert_eq!(after, "project: new # c\r\n");
}

#[test]
fn scalar_rewrite_rewrites_a_repeated_key_twice() {
    let (after, edits) = rewrite_frontmatter_scalar("p: old\nq: 1\np: 'old'\n", "p", "old", "new");
    assert_eq!(after, "p: new\nq: 1\np: 'new'\n");
    assert_eq!(edits.len(), 2);
}

#[test]
fn list_append_extends_flow_block_and_absent() {
    assert_eq!(
        append("a: 1\naliases: [x, y]\nb: 2\n", "aliases", "z"),
        "a: 1\naliases: [x, y, z]\nb: 2\n"
    );
    assert_eq!(append("aliases: []\n", "aliases", "z"), "aliases: [z]\n");
    assert_eq!(
        append("a: 1\naliases:\n  - x\n  - y\nb: 2\n", "aliases", "z"),
        "a: 1\naliases:\n  - x\n  - y\n  - z\nb: 2\n"
    );
    assert_eq!(
        append("aliases:\n- x", "aliases", "z"),
        "aliases:\n- x\n- z"
    );
    assert_eq!(append("a: 1\n", "aliases", "z"), "a: 1\naliases: [z]\n");
    assert_eq!(append("a: 1", "aliases", "z"), "a: 1\naliases: [z]");
    assert_eq!(append("", "aliases", "z"), "aliases: [z]\n");
    assert_eq!(
        append("aliases: [x]\n", "aliases", "Old: name"),
        "aliases: [x, \"Old: name\"]\n"
    );
}

#[test]
fn list_append_is_idempotent() {
    for block in [
        "aliases: [x, \"z\"]\n",
        "aliases:\n  - x\n  - z\n",
        "aliases: [z]\n",
    ] {
        assert_eq!(append(block, "aliases", "z"), block);
    }
    let once = append("a: 1\n", "aliases", "z");
    assert_eq!(append(&once, "aliases", "z"), once);
}

#[test]
fn a_bare_to_that_would_not_stay_a_string_is_quoted() {
    for to in ["123", "true", "null", "x #y", "a: b", "[x]"] {
        let before = "project: old\nother: 1\n";
        let (after, edits) = rewrite_frontmatter_scalar(before, "project", "old", to);
        assert!(differs_only_by(before, &after, &edits));
        let doc = format!("---\n{after}---\n");
        let (fm, _) = Frontmatter::parse(&doc).unwrap();
        assert_eq!(
            fm.require_field::<String>("project").unwrap(),
            to,
            "{after:?}"
        );
    }
    // A safe value stays bare.
    let (after, _) = rewrite_frontmatter_scalar("project: old\n", "project", "old", "new-slug");
    assert_eq!(after, "project: new-slug\n");
}

#[test]
fn a_multi_line_plain_scalar_is_not_rewritten() {
    let before = "project: old\n  continued\ntitle: Surrogate model\n";
    let (after, edits) = rewrite_frontmatter_scalar(before, "project", "old", "new");
    assert!(edits.is_empty());
    assert_eq!(after, before);
    // An indented comment or a following key does not continue it.
    let before = "project: old\n# c\nnext: 1\n";
    assert_eq!(
        rewrite_frontmatter_scalar(before, "project", "old", "new")
            .1
            .len(),
        1
    );
}

#[test]
fn drift_check_names_the_other_key() {
    let before = "title: Keep\nproject: old\n";
    let (after, _) = rewrite_frontmatter_scalar(before, "project", "old", "new");
    assert_eq!(
        frontmatter_changed_only_at(before, &after, &["project"]),
        Ok(())
    );
    let drifted = after.replace("Keep", "Kept");
    assert_eq!(
        frontmatter_changed_only_at(before, &drifted, &["project"]),
        Err(FrontmatterDrift::KeyChanged("title".into()))
    );
    assert_eq!(
        frontmatter_changed_only_at(
            before,
            "title: Keep\nproject: old\nextra: 1\n",
            &["project"]
        ),
        Err(FrontmatterDrift::KeyAdded("extra".into()))
    );
    assert_eq!(
        frontmatter_changed_only_at(before, "project: old\n", &["project"]),
        Err(FrontmatterDrift::KeyRemoved("title".into()))
    );
    assert!(matches!(
        frontmatter_changed_only_at(before, "title: [unclosed\n", &["project"]),
        Err(FrontmatterDrift::Unparsable {
            which: DriftSide::After,
            ..
        })
    ));
    assert!(matches!(
        frontmatter_changed_only_at("a: [", before, &[]),
        Err(FrontmatterDrift::Unparsable {
            which: DriftSide::Before,
            ..
        })
    ));
}

fn word() -> impl Strategy<Value = String> {
    "[a-z]{1,6}"
}

/// A generated line, and (key, offset of the first value char) when it is a
/// scalar the test may mutate.
type Gen = (String, Option<(String, usize)>);

fn other_line() -> impl Strategy<Value = Gen> {
    prop_oneof![
        (word(), word())
            .prop_map(|(k, v)| (format!("k{k}: {v}"), Some((format!("k{k}"), k.len() + 3)))),
        (word(), word()).prop_map(|(k, v)| (
            format!("k{k}: \"{v}\""),
            Some((format!("k{k}"), k.len() + 4))
        )),
        (word(), word())
            .prop_map(|(k, v)| (format!("k{k}: '{v}'"), Some((format!("k{k}"), k.len() + 4)))),
        word().prop_map(|v| (format!("# project: {v}"), None)),
        word().prop_map(|v| (format!("t{v}: [a, b]"), None)),
        word().prop_map(|v| (format!("b{v}:\n  - a\n  - b"), None)),
        Just(("outer:\n  project: old".to_owned(), None)),
        Just(("link: \"[[old]]\"".to_owned(), None)),
        Just((String::new(), None)),
    ]
}

fn target_line() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("project: old".to_owned()),
        Just("project: 'old'".to_owned()),
        Just("project: \"old\" # note".to_owned()),
    ]
}

proptest! {
    #[test]
    fn a_rewrite_passes_its_own_drift_check(
        others in proptest::collection::vec(other_line(), 0..8),
        target in target_line(),
        at in 0usize..9,
        title in word(),
        pick in 0usize..9,
    ) {
        let at = at.min(others.len());
        let mut before = format!("title: {title}\n");
        // (key, absolute offset of the first value char) per mutable scalar.
        let mut mutable = vec![("title".to_owned(), 7usize)];
        let mut push = |line: &str, m: Option<(String, usize)>, before: &mut String| {
            if let Some((k, off)) = m {
                mutable.push((k, before.len() + off));
            }
            before.push_str(line);
            before.push('\n');
        };
        for (i, (l, m)) in others.iter().enumerate() {
            if i == at {
                push(&target, None, &mut before);
            }
            push(l, m.clone(), &mut before);
        }
        if at >= others.len() {
            push(&target, None, &mut before);
        }
        // Duplicate keys are rejected by the parser; skip such inputs.
        prop_assume!(frontmatter_changed_only_at(&before, &before, &[]).is_ok());

        let (after, edits) = rewrite_frontmatter_scalar(&before, "project", "old", "new");
        prop_assert_eq!(edits.len(), 1);
        prop_assert!(differs_only_by(&before, &after, &edits));
        prop_assert_eq!(frontmatter_changed_only_at(&before, &after, &["project"]), Ok(()));

        let (key, off) = &mutable[pick % mutable.len()];
        // "old" and "new" are the same length, so offsets survive the rewrite.
        let mut mutated = after.clone();
        mutated.replace_range(*off..*off + 1, "Z");
        prop_assert_eq!(
            frontmatter_changed_only_at(&before, &mutated, &["project"]),
            Err(FrontmatterDrift::KeyChanged(key.clone()))
        );
    }
}
