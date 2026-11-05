use cdno_core::extractors::differs_only_by;
use cdno_core::frontmatter_edit::{
    DriftSide, FrontmatterDrift, append_frontmatter_list_value, frontmatter_changed_only_at,
    rewrite_frontmatter_scalar,
};
use proptest::prelude::*;

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
        append_frontmatter_list_value("a: 1\naliases: [x, y]\nb: 2\n", "aliases", "z"),
        "a: 1\naliases: [x, y, z]\nb: 2\n"
    );
    assert_eq!(
        append_frontmatter_list_value("aliases: []\n", "aliases", "z"),
        "aliases: [z]\n"
    );
    assert_eq!(
        append_frontmatter_list_value("a: 1\naliases:\n  - x\n  - y\nb: 2\n", "aliases", "z"),
        "a: 1\naliases:\n  - x\n  - y\n  - z\nb: 2\n"
    );
    assert_eq!(
        append_frontmatter_list_value("aliases:\n- x", "aliases", "z"),
        "aliases:\n- x\n- z"
    );
    assert_eq!(
        append_frontmatter_list_value("a: 1\n", "aliases", "z"),
        "a: 1\naliases: [z]\n"
    );
    assert_eq!(
        append_frontmatter_list_value("a: 1", "aliases", "z"),
        "a: 1\naliases: [z]"
    );
    assert_eq!(
        append_frontmatter_list_value("", "aliases", "z"),
        "aliases: [z]\n"
    );
    assert_eq!(
        append_frontmatter_list_value("aliases: [x]\n", "aliases", "Old: name"),
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
        assert_eq!(append_frontmatter_list_value(block, "aliases", "z"), block);
    }
    let once = append_frontmatter_list_value("a: 1\n", "aliases", "z");
    assert_eq!(append_frontmatter_list_value(&once, "aliases", "z"), once);
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

fn other_line() -> impl Strategy<Value = String> {
    prop_oneof![
        (word(), word()).prop_map(|(k, v)| format!("k{k}: {v}")),
        (word(), word()).prop_map(|(k, v)| format!("k{k}: \"{v}\"")),
        (word(), word()).prop_map(|(k, v)| format!("k{k}: '{v}'")),
        word().prop_map(|v| format!("# project: {v}")),
        word().prop_map(|v| format!("t{v}: [a, b]")),
        word().prop_map(|v| format!("b{v}:\n  - a\n  - b")),
        Just(String::new()),
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
    ) {
        // Keys are made unique by construction only loosely; duplicates are
        // rejected by the parser, so skip such inputs.
        let mut lines = others;
        let at = at.min(lines.len());
        lines.insert(at, target);
        let mut before = format!("title: {title}\n");
        for l in &lines {
            before.push_str(l);
            before.push('\n');
        }
        prop_assume!(frontmatter_changed_only_at(&before, &before, &[]).is_ok());

        let (after, edits) = rewrite_frontmatter_scalar(&before, "project", "old", "new");
        prop_assert_eq!(edits.len(), 1);
        prop_assert!(differs_only_by(&before, &after, &edits));
        prop_assert_eq!(frontmatter_changed_only_at(&before, &after, &["project"]), Ok(()));

        let mut mutated = after.clone();
        mutated.replace_range(7..8, "Z");
        prop_assert_eq!(
            frontmatter_changed_only_at(&before, &mutated, &["project"]),
            Err(FrontmatterDrift::KeyChanged("title".into()))
        );
    }
}
