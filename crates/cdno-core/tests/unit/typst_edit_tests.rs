use cdno_core::extractors::differs_only_by;
use cdno_core::typst::{
    TypstError, append_list_value, metadata_block, remove_list_value, rewrite_scalar, scalar_span,
    write_block,
};
use proptest::prelude::*;
use serde_json::{Map, Value, json};

const KEYWORDS: &[&str] = &[
    "none", "auto", "true", "false", "not", "and", "or", "let", "set", "show", "context", "if",
    "else", "for", "in", "while", "break", "continue", "return", "import", "include", "as",
];

const NOTE: &str = r#"// the surrogate model's note
#metadata((
  type: "concept",
  // kept as written
  title: "Surrogate model",
  tags: ("surrogate",),
  draft: false,
)) <cdno>

= Surrogate model
"#;

fn fields(pairs: &[(&str, Value)]) -> Vec<(String, Value)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn read(source: &str) -> Value {
    metadata_block(source).unwrap().value
}

// ── the writer ───────────────────────────────────────────────────────

#[test]
fn write_block_has_one_key_per_line_and_the_label_on_the_closing_line() {
    let block = write_block(&fields(&[
        ("type", json!("concept")),
        ("tags", json!(["surrogate"])),
        ("draft", json!(true)),
        ("n", json!(2)),
    ]))
    .unwrap();
    assert_eq!(
        block,
        "#metadata((\n  type: \"concept\",\n  tags: (\"surrogate\",),\n  draft: true,\n  n: 2,\n)) <cdno>"
    );
}

#[test]
fn write_block_of_no_fields_is_the_empty_dictionary() {
    assert_eq!(write_block(&[]).unwrap(), "#metadata((:)) <cdno>");
    assert_eq!(read(&write_block(&[]).unwrap()), json!({}));
}

#[test]
fn a_quote_in_a_value_is_escaped_for_typst_not_markup() {
    let block = write_block(&fields(&[("title", json!("a \"b\" #c"))])).unwrap();
    assert!(block.contains(r#"title: "a \"b\" #c","#), "{block}");
    assert_eq!(read(&block), json!({"title": "a \"b\" #c"}));
}

#[test]
fn a_backslash_and_a_line_break_survive() {
    let value = "a\\b\nc\r\td\u{2028}e\u{7}";
    let block = write_block(&fields(&[("t", json!(value))])).unwrap();
    assert_eq!(block.lines().count(), 3, "{block}");
    assert_eq!(read(&block), json!({"t": value}));
}

#[test]
fn keywords_and_non_identifiers_are_written_as_string_keys() {
    for key in KEYWORDS
        .iter()
        .copied()
        .chain(["a b", "", "1x", "tag:x", "_"])
    {
        let block = write_block(&fields(&[(key, json!("v"))])).unwrap();
        assert!(block.contains(&format!("\"{key}\": ")), "{key}: {block}");
        assert_eq!(read(&block), json!({ key: "v" }), "{key}");
    }
    let plain = write_block(&fields(&[("type", json!("v"))])).unwrap();
    assert!(plain.contains("  type: "), "{plain}");
}

#[test]
fn a_lone_underscore_key_is_written_as_a_string_key() {
    let block = write_block(&fields(&[("_", json!(""))])).unwrap();
    assert!(block.contains("\"_\": "), "{block}");
    assert_eq!(read(&block), json!({"_": ""}));
}

#[test]
fn a_value_the_reader_would_refuse_is_not_written() {
    for value in [
        json!(null),
        json!({"a": 1}),
        json!([1]),
        json!([["a"]]),
        json!(u64::MAX),
    ] {
        let result = write_block(&fields(&[("k", value.clone())]));
        assert!(
            matches!(result, Err(TypstError::Unwritable { .. })),
            "{value}"
        );
    }
}

#[test]
fn numbers_keep_their_kind() {
    let block = write_block(&fields(&[
        ("i", json!(1)),
        ("f", json!(1.0)),
        ("n", json!(-7)),
        ("m", json!(i64::MIN + 1)),
        ("e", json!(1e21)),
        ("s", json!(1.5e-7)),
    ]))
    .unwrap();
    let value = read(&block);
    assert!(value["i"].is_i64() && value["f"].is_f64(), "{block}");
    assert_eq!(value["m"], json!(i64::MIN + 1));
    assert_eq!(value["e"], json!(1e21));
    assert_eq!(value["s"], json!(1.5e-7));
}

fn key_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        proptest::sample::select(KEYWORDS.to_vec()).prop_map(str::to_string),
        "[a-z][a-z0-9_-]{0,6}",
        ".{0,6}",
    ]
}

fn value_strategy() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<String>().prop_map(Value::String),
        proptest::collection::vec(any::<String>(), 0..4)
            .prop_map(|v| Value::Array(v.into_iter().map(Value::String).collect())),
        any::<bool>().prop_map(Value::Bool),
        (i64::MIN + 1..=i64::MAX).prop_map(|n| json!(n)),
        any::<f64>().prop_filter_map("finite", |f| serde_json::Number::from_f64(f)
            .map(Value::Number)),
    ]
}

fn dictionary() -> impl Strategy<Value = Vec<(String, Value)>> {
    proptest::collection::vec((key_strategy(), value_strategy()), 0..=8).prop_map(|pairs| {
        let mut seen = std::collections::HashSet::new();
        pairs
            .into_iter()
            .filter(|(k, _)| seen.insert(k.clone()))
            .collect()
    })
}

fn as_object(pairs: &[(String, Value)]) -> Value {
    Value::Object(pairs.iter().cloned().collect::<Map<_, _>>())
}

proptest! {
    #[test]
    fn write_block_round_trips_through_metadata_block(pairs in dictionary()) {
        let block = write_block(&pairs).unwrap();
        prop_assert_eq!(read(&block), as_object(&pairs));
    }

    #[test]
    fn rewrite_scalar_round_trips_and_moves_nothing_else(
        pairs in dictionary(),
        key in key_strategy(),
        value in value_strategy(),
    ) {
        let source = format!("{}\n\n= Body\n", write_block(&pairs).unwrap());
        let (after, edits) = rewrite_scalar(&source, &key, &value).unwrap();
        let mut expected = pairs.clone();
        match expected.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => expected.push((key, value)),
        }
        prop_assert_eq!(read(&after), as_object(&expected));
        prop_assert!(differs_only_by(&source, &after, &edits));
    }
}

// ── editing in place ─────────────────────────────────────────────────

#[test]
fn rewrite_scalar_changes_only_the_value_span() {
    let (after, edits) = rewrite_scalar(NOTE, "title", &json!("Bias–variance split")).unwrap();
    let before_span = scalar_span(&metadata_block(NOTE).unwrap(), "title").unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].range, before_span);
    assert_eq!(edits[0].replacement, "\"Bias–variance split\"");
    assert!(differs_only_by(NOTE, &after, &edits));
    assert!(after.contains("  // kept as written\n"));
    assert!(after.starts_with("// the surrogate model's note\n"));
    assert_eq!(read(&after)["title"], json!("Bias–variance split"));
    assert_eq!(read(&after)["draft"], json!(false));
}

#[test]
fn a_same_line_comment_survives_a_rewrite() {
    let source = "#metadata((\n  type: \"x\", // keep\n  n: 1, // and this\n)) <cdno>\n";
    let (after, edits) = rewrite_scalar(source, "type", &json!("y")).unwrap();
    assert_eq!(
        after,
        "#metadata((\n  type: \"y\", // keep\n  n: 1, // and this\n)) <cdno>\n"
    );
    assert_eq!(edits[0].range, 20..23);
    assert!(differs_only_by(source, &after, &edits));
}

#[test]
fn the_exact_check_rejects_a_change_outside_the_edit() {
    let (after, edits) = rewrite_scalar(NOTE, "draft", &json!(true)).unwrap();
    assert!(differs_only_by(NOTE, &after, &edits));
    let moved = after.replacen("= Surrogate model", "= Other", 1);
    assert!(!differs_only_by(NOTE, &moved, &edits));
}

#[test]
fn a_value_can_change_kind() {
    let (after, _) = rewrite_scalar(NOTE, "draft", &json!(3)).unwrap();
    assert_eq!(read(&after)["draft"], json!(3));
    let (after, _) = rewrite_scalar(NOTE, "tags", &json!(["a", "b"])).unwrap();
    assert_eq!(read(&after)["tags"], json!(["a", "b"]));
}

#[test]
fn rewrite_scalar_inserts_a_missing_key_before_the_close() {
    let (after, edits) = rewrite_scalar(NOTE, "owner", &json!("me")).unwrap();
    assert!(
        after.contains("  draft: false,\n  owner: \"me\",\n)) <cdno>"),
        "{after}"
    );
    assert!(differs_only_by(NOTE, &after, &edits));
    assert_eq!(read(&after)["owner"], json!("me"));
}

#[test]
fn insertion_without_a_trailing_comma_adds_the_comma() {
    let multi = "#metadata((\n  type: \"x\"\n)) <cdno>\n";
    let (after, _) = rewrite_scalar(multi, "k", &json!(1)).unwrap();
    assert_eq!(after, "#metadata((\n  type: \"x\",\n  k: 1\n)) <cdno>\n");
    assert_eq!(read(&after), json!({"type": "x", "k": 1}));

    let one_line = "#metadata((type: \"x\")) <cdno>\n";
    let (after, _) = rewrite_scalar(one_line, "k", &json!(1)).unwrap();
    assert_eq!(after, "#metadata((type: \"x\", k: 1)) <cdno>\n");

    let one_line_comma = "#metadata((type: \"x\",)) <cdno>\n";
    let (after, _) = rewrite_scalar(one_line_comma, "k", &json!(1)).unwrap();
    assert_eq!(read(&after), json!({"type": "x", "k": 1}));
}

#[test]
fn inserting_into_an_empty_dictionary_replaces_the_colon() {
    for source in [
        "#metadata((:)) <cdno>\n",
        "#metadata((: )) <cdno>\n",
        "#metadata((\n  :\n)) <cdno>\n",
    ] {
        let (after, edits) = rewrite_scalar(source, "type", &json!("evidence")).unwrap();
        assert_eq!(read(&after), json!({"type": "evidence"}), "{after}");
        assert!(!after.contains(":)") && !after.contains("(:"), "{after}");
        assert!(after.ends_with(")) <cdno>\n"));
        assert!(differs_only_by(source, &after, &edits));
    }
    let (after, _) = rewrite_scalar("#metadata((:)) <cdno>\n", "type", &json!("e")).unwrap();
    assert_eq!(after, "#metadata((\n  type: \"e\",\n)) <cdno>\n");
}

#[test]
fn a_keyword_key_is_inserted_as_a_string_key() {
    let (after, _) = rewrite_scalar(NOTE, "in", &json!("x")).unwrap();
    assert!(after.contains("  \"in\": \"x\","), "{after}");
    assert_eq!(read(&after)["in"], json!("x"));
}

#[test]
fn an_edit_of_a_source_without_a_block_is_the_readers_error() {
    assert_eq!(
        rewrite_scalar("= Heading\n", "k", &json!(1)).unwrap_err(),
        TypstError::NoBlock
    );
}

#[test]
fn scalar_span_addresses_the_value() {
    let block = metadata_block(NOTE).unwrap();
    assert_eq!(&NOTE[scalar_span(&block, "draft").unwrap()], "false");
    assert_eq!(scalar_span(&block, "absent"), None);
}

// ── lists ────────────────────────────────────────────────────────────

#[test]
fn append_to_a_one_element_array_keeps_it_an_array() {
    let (after, edits) = append_list_value(NOTE, "tags", "error-budget").unwrap();
    assert_eq!(edits[0].replacement, "(\"surrogate\", \"error-budget\")");
    assert!(differs_only_by(NOTE, &after, &edits));

    let parenthesised = NOTE.replace("(\"surrogate\",)", "(\"surrogate\")");
    let (after, _) = append_list_value(&parenthesised, "tags", "b").unwrap();
    assert_eq!(read(&after)["tags"], json!(["surrogate", "b"]));
    assert!(after.contains("tags: (\"surrogate\", \"b\"),"));
}

#[test]
fn append_inserts_an_absent_key_and_skips_a_present_item() {
    let (after, _) = append_list_value(NOTE, "aliases", "sm").unwrap();
    assert_eq!(read(&after)["aliases"], json!(["sm"]));
    assert!(after.contains("aliases: (\"sm\",),"));

    let (same, edits) = append_list_value(NOTE, "tags", "surrogate").unwrap();
    assert_eq!(same, NOTE);
    assert!(edits.is_empty());
}

#[test]
fn append_to_a_value_that_is_not_a_list_is_refused() {
    assert!(matches!(
        append_list_value(NOTE, "title", "x"),
        Err(TypstError::Unwritable { .. })
    ));
}

#[test]
fn remove_last_item_leaves_the_empty_array() {
    let (after, edits) = remove_list_value(NOTE, "tags", "surrogate").unwrap();
    assert_eq!(edits[0].replacement, "()");
    assert_eq!(read(&after)["tags"], json!([]));
    assert!(differs_only_by(NOTE, &after, &edits));
}

#[test]
fn remove_leaves_the_others_and_ignores_a_missing_item_or_key() {
    let source = "#metadata((tags: (\"a\", \"b\", \"c\"))) <cdno>\n";
    let (after, _) = remove_list_value(source, "tags", "b").unwrap();
    assert_eq!(read(&after)["tags"], json!(["a", "c"]));
    for (key, item) in [("tags", "z"), ("other", "a")] {
        let (same, edits) = remove_list_value(source, key, item).unwrap();
        assert_eq!(same, source);
        assert!(edits.is_empty());
    }
}

#[test]
fn a_crlf_source_gets_crlf_line_endings_on_insertion() {
    let with_comma = "#metadata((\r\n  type: \"x\",\r\n)) <cdno>\r\n";
    let (after, edits) = rewrite_scalar(with_comma, "k", &json!(1)).unwrap();
    assert_eq!(
        after,
        "#metadata((\r\n  type: \"x\",\r\n  k: 1,\r\n)) <cdno>\r\n"
    );
    assert!(differs_only_by(with_comma, &after, &edits));

    let no_comma = "#metadata((\r\n  type: \"x\"\r\n)) <cdno>\r\n";
    let (after, _) = rewrite_scalar(no_comma, "k", &json!(1)).unwrap();
    assert_eq!(
        after,
        "#metadata((\r\n  type: \"x\",\r\n  k: 1\r\n)) <cdno>\r\n"
    );

    let empty = "#metadata((:)) <cdno>\r\n";
    let (after, _) = rewrite_scalar(empty, "k", &json!(1)).unwrap();
    assert_eq!(after, "#metadata((\r\n  k: 1,\r\n)) <cdno>\r\n");
}

#[test]
fn write_block_refuses_a_repeated_key() {
    let result = write_block(&fields(&[("a", json!("x")), ("a", json!("y"))]));
    assert!(matches!(result, Err(TypstError::Unwritable { .. })));
}
