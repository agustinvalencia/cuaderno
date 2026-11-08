use cdno_core::typst::{TypstError, metadata_block};
use serde_json::json;

const EXAMPLE: &str = r#"#metadata((
  type: "concept",
  title: "Bias–variance split",
  created: "2026-11-08",
  origin: "[[journal/2026/daily/2026-11-02#Error budget sketch]]",
  tags: ("surrogate", "error-budget"),
  draft: true,
)) <cdno>

= Surrogate model
"#;

fn err(source: &str) -> TypstError {
    metadata_block(source).unwrap_err()
}

fn not_literal(source: &str) -> (usize, String) {
    match err(source) {
        TypstError::NotLiteral { line, what } => (line, what),
        other => panic!("expected NotLiteral, got {other:?}"),
    }
}

#[test]
fn metadata_block_reads_strings_arrays_and_booleans() {
    let block = metadata_block(EXAMPLE).unwrap();
    assert_eq!(
        block.value,
        json!({
            "type": "concept",
            "title": "Bias–variance split",
            "created": "2026-11-08",
            "origin": "[[journal/2026/daily/2026-11-02#Error budget sketch]]",
            "tags": ["surrogate", "error-budget"],
            "draft": true,
        })
    );
}

#[test]
fn the_spans_address_the_source() {
    let block = metadata_block(EXAMPLE).unwrap();
    let text = &EXAMPLE[block.span.clone()];
    assert!(text.starts_with("#metadata(("));
    assert!(text.ends_with("<cdno>"));

    let keys: Vec<_> = block.entries.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(
        keys,
        ["type", "title", "created", "origin", "tags", "draft"]
    );
    let tags = &block.entries[4];
    assert_eq!(&EXAMPLE[tags.key_span.clone()], "tags");
    assert_eq!(
        &EXAMPLE[tags.value_span.clone()],
        r#"("surrogate", "error-budget")"#
    );
    assert_eq!(&EXAMPLE[block.insert_at..block.insert_at + 2], "))");
}

#[test]
fn string_keys_are_read_and_a_comment_inside_the_dictionary_is_skipped() {
    let source = "#metadata((\n  // why\n  \"type\": /* kind */ \"evidence\",\n)) <cdno>\n";
    let block = metadata_block(source).unwrap();
    assert_eq!(block.value, json!({"type": "evidence"}));
    assert_eq!(&source[block.entries[0].key_span.clone()], "\"type\"");
    assert_eq!(&source[block.entries[0].value_span.clone()], "\"evidence\"");
}

#[test]
fn an_empty_dictionary_is_read_and_inserts_before_its_parenthesis() {
    let source = "#metadata((:)) <cdno>\n";
    let block = metadata_block(source).unwrap();
    assert_eq!(block.value, json!({}));
    assert_eq!(block.insert_at, source.find(")").unwrap());
}

#[test]
fn one_element_array_and_parenthesised_string_read_alike() {
    let array = metadata_block("#metadata((tags: (\"x\",))) <cdno>").unwrap();
    let parenthesised = metadata_block("#metadata((tags: (\"x\"))) <cdno>").unwrap();
    assert_eq!(array.value, json!({"tags": ["x"]}));
    assert_eq!(array.value, parenthesised.value);
}

#[test]
fn string_escapes_are_decoded() {
    let block = metadata_block(r#"#metadata((title: "a \"b\" \\ c")) <cdno>"#).unwrap();
    assert_eq!(block.value, json!({"title": r#"a "b" \ c"#}));
}

#[test]
fn a_leading_comment_is_allowed_before_the_block() {
    let source = "// A note.\n/* More. */\n\n#metadata((type: \"evidence\")) <cdno>\n";
    let block = metadata_block(source).unwrap();
    assert_eq!(block.span.start, source.find('#').unwrap());
}

#[test]
fn a_label_on_the_next_line_is_the_blocks() {
    for source in [
        "#metadata((type: \"evidence\"))\n<cdno>\n",
        "#metadata((type: \"evidence\"))<cdno>\n",
        "#metadata((type: \"evidence\")) /* c */ <cdno>\n",
        "#metadata((type: \"evidence\"))\n\n<cdno>\n",
    ] {
        let block = metadata_block(source).unwrap();
        assert_eq!(block.value, json!({"type": "evidence"}), "{source}");
        assert!(source[block.span].ends_with("<cdno>"), "{source}");
    }
}

#[test]
fn no_block() {
    assert_eq!(err("= Surrogate model\n"), TypstError::NoBlock);
    assert_eq!(err("// <cdno>\n"), TypstError::NoBlock);
    assert_eq!(err("#metadata((x: \"y\"))\n"), TypstError::NoBlock);
    assert_eq!(err("<cdno>\n"), TypstError::NoBlock);
}

#[test]
fn label_broken() {
    let broken = TypstError::LabelBroken { line: 2 };
    assert_eq!(err("\n#metadata((type: \"evidence\"))\n"), broken);
    assert_eq!(err("\n#metadata((type: \"evidence\")) <cdn>\n"), broken);
    assert_eq!(err("\n#metadata((\"type\": \"evidence\"))\n"), broken);
    assert_eq!(err("\n#metadata((type: \"evidence\")); <cdno>\n"), broken);
    assert_eq!(err("\n#metadata((type: \"evidence\")) x <cdno>\n"), broken);
}

#[test]
fn nested() {
    assert_eq!(
        err("#[\n  #metadata((type: \"evidence\")) <cdno>\n]\n"),
        TypstError::Nested { line: 2 }
    );
    assert_eq!(
        err("#let f() = [\n\n  #metadata((type: \"evidence\")) <cdno>\n]\n"),
        TypstError::Nested { line: 3 }
    );
}

#[test]
fn not_first() {
    assert_eq!(
        err("= Heading\n\n#metadata((type: \"evidence\")) <cdno>\n"),
        TypstError::NotFirst { line: 3 }
    );
}

#[test]
fn a_shebang_is_not_whitespace_or_a_comment() {
    assert_eq!(
        err("#!/usr/bin/env typst\n#metadata((type: \"evidence\")) <cdno>\n"),
        TypstError::NotFirst { line: 2 }
    );
}

#[test]
fn another_metadata_call_before_the_block_is_not_first() {
    assert_eq!(
        err("#metadata((x: 1)) <other>\n\n#metadata((type: \"evidence\")) <cdno>\n"),
        TypstError::NotFirst { line: 3 }
    );
}

#[test]
fn duplicate() {
    assert_eq!(
        err("#metadata((type: \"a\")) <cdno>\n\n#metadata((type: \"b\")) <cdno>\n"),
        TypstError::Duplicate { line: 3 }
    );
}

#[test]
fn a_nested_second_block_is_a_duplicate() {
    assert_eq!(
        err("#metadata((type: \"a\")) <cdno>\n#[\n#metadata((type: \"b\")) <cdno>\n]\n"),
        TypstError::Duplicate { line: 3 }
    );
}

#[test]
fn other_metadata_labels_after_the_block_are_ignored() {
    let source =
        "#metadata((type: \"a\")) <cdno>\n\n#metadata((x: 1)) <other>\n#metadata((type: \"b\"))\n";
    assert_eq!(metadata_block(source).unwrap().value, json!({"type": "a"}));
}

#[test]
fn not_a_dictionary() {
    let expected = TypstError::NotADictionary { line: 1 };
    assert_eq!(err("#metadata(\"x\") <cdno>"), expected);
    assert_eq!(err("#metadata(()) <cdno>"), expected);
    assert_eq!(err("#metadata() <cdno>"), expected);
    assert_eq!(err("#metadata((a: \"b\"), (c: \"d\")) <cdno>"), expected);
    assert_eq!(err("#metadata(x: (a: \"b\")) <cdno>"), expected);
    assert_eq!(err("#metadata(bindings) <cdno>"), expected);
}

#[test]
fn not_literal_nested_dictionary() {
    let (line, what) = not_literal("#metadata((\n  a: (b: \"c\"),\n)) <cdno>");
    assert_eq!((line, what.as_str()), (2, "dictionary"));
}

#[test]
fn not_literal_names_the_node_kind() {
    for (value, expected) in [
        ("x", "identifier"),
        ("f()", "function call"),
        ("\"a\" + \"b\"", "binary expression"),
        ("1pt", "numeric value"),
        ("none", "`none`"),
        ("((\"x\"))", "group"),
    ] {
        let source = format!("#metadata((a: {value})) <cdno>");
        assert_eq!(not_literal(&source), (1, expected.to_string()), "{value}");
    }
}

#[test]
fn not_literal_array_items_report_their_own_line() {
    let (line, what) = not_literal("#metadata((a: (\n  \"x\",\n  1,\n))) <cdno>");
    assert_eq!((line, what.as_str()), (3, "integer"));
    let (line, what) = not_literal("#metadata((a: (..b))) <cdno>");
    assert_eq!((line, what.as_str()), (1, "spread"));
}

#[test]
fn not_literal_spread_and_computed_key() {
    let (_, what) = not_literal("#metadata((a: \"b\", ..c)) <cdno>");
    assert_eq!(what, "spread");
    let (_, what) = not_literal("#metadata((a + b: \"c\")) <cdno>");
    assert_eq!(what, "binary expression");
}

#[test]
fn a_repeated_key_is_refused_rather_than_read() {
    let (line, what) = not_literal("#metadata((\n  a: \"x\",\n  a: \"y\",\n)) <cdno>");
    assert_eq!((line, what.as_str()), (3, "a syntax error"));
    let (line, _) = not_literal("#metadata((a: \"x\",\n\"a\": \"y\")) <cdno>");
    assert_eq!(line, 2);
}

#[test]
fn a_very_deep_tree_does_not_overflow_the_walk() {
    let chain = format!("#a{}", ".b".repeat(200_000));
    assert_eq!(err(&chain), TypstError::NoBlock);
    let errors = format!("$#(1 +){}$", "!".repeat(200_000));
    assert_eq!(err(&errors), TypstError::NoBlock);
}

#[test]
fn integers_and_floats_read_as_json_numbers() {
    let source = "#metadata((a: 3, b: -4, c: 1.5, d: -0.25, e: 1e3, f: 0xff, g: -0x10)) <cdno>";
    let block = metadata_block(source).unwrap();
    assert_eq!(
        block.value,
        json!({"a": 3, "b": -4, "c": 1.5, "d": -0.25, "e": 1000.0, "f": 255, "g": -16})
    );
    assert!(block.value["a"].is_i64() && block.value["c"].is_f64());
    let negative = &block.entries[1];
    assert_eq!(&source[negative.value_span.clone()], "-4");
}

#[test]
fn an_integer_beyond_i64_is_a_float_as_in_typst() {
    let block = metadata_block("#metadata((a: 9223372036854775808)) <cdno>").unwrap();
    assert!(block.value["a"].is_f64());
    let (_, what) = not_literal("#metadata((a: 1e999)) <cdno>");
    assert_eq!(what, "float");
}

#[test]
fn only_a_leading_minus_makes_a_number_a_literal() {
    for expr in ["+3", "--3", "-a", "1 + 2", "3pt", "50%", "-(3)"] {
        let source = format!("#metadata((a: {expr})) <cdno>");
        assert!(
            matches!(err(&source), TypstError::NotLiteral { .. }),
            "{expr}"
        );
    }
}

#[test]
fn a_metadata_call_that_is_not_a_markup_hash_call_is_not_a_broken_block() {
    // Inside code there is no `#`, so this was never written as a block.
    assert_eq!(
        err("#{ metadata((type: \"concept\")) }\n"),
        TypstError::NoBlock
    );
}

#[test]
fn the_top_level_block_is_chosen_over_an_earlier_nested_one() {
    let source = "#[ #metadata((type: \"a\")) <cdno> ]\n#metadata((type: \"b\")) <cdno>\n";
    assert_eq!(err(source), TypstError::NotFirst { line: 2 });
}
