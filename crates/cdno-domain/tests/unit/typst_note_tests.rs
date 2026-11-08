//! The Typst block writer, the field transport and the creation-time body check (RFC 0008 §5.7).

use cdno_core::path::VaultPath;
use cdno_domain::error::DomainError;
use cdno_domain::vault::typst_note::{
    BodyRejection, assemble_typst_note, check_typst_body, render_metadata_block,
};
use serde_json::{Value, json};

fn fields(pairs: &[(&str, Value)]) -> Vec<(String, Value)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

const RFC_BODY: &str = "#import \"@preview/some-template:0.3.0\": report
#show: report.with(title: \"Error budget derivation\")

= Setting

The surrogate's error splits as in [[concepts/bias-variance-split]] …

$ E[(hat(f) - f)^2] = \"bias\"^2 + \"variance\" + sigma^2 $
";

#[test]
fn render_metadata_block_orders_engine_fields_first() {
    let block = render_metadata_block(
        "evidence",
        &fields(&[
            ("type", json!("evidence")),
            ("created", json!("2026-11-08")),
            (
                "source",
                json!("Error budget derivation for the surrogate model, 2026-11-08"),
            ),
            ("portfolio", json!("surrogate-error-budget")),
            ("origin", json!("[[projects/surrogate-model]]")),
        ]),
    )
    .unwrap();
    assert_eq!(
        block,
        "#metadata((
  type: \"evidence\",
  created: \"2026-11-08\",
  source: \"Error budget derivation for the surrogate model, 2026-11-08\",
  portfolio: \"surrogate-error-budget\",
  origin: \"[[projects/surrogate-model]]\",
)) <cdno>"
    );
}

#[test]
fn a_supplied_array_becomes_an_array_and_a_scalar_a_string() {
    let block = render_metadata_block(
        "concept",
        &fields(&[
            ("type", json!("concept")),
            ("tags", json!(["surrogate", "error-budget"])),
            ("one", json!(["surrogate"])),
            ("n", json!(2)),
            ("draft", json!(true)),
        ]),
    )
    .unwrap();
    assert!(
        block.contains("tags: (\"surrogate\", \"error-budget\"),"),
        "{block}"
    );
    assert!(block.contains("one: (\"surrogate\",),"), "{block}");
    assert!(block.contains("n: \"2\","), "{block}");
    assert!(block.contains("draft: \"true\","), "{block}");
}

#[test]
fn an_unwritable_value_maps_to_invalid_field_value() {
    for bad in [json!(null), json!({"a": "b"}), json!([1]), json!([["x"]])] {
        let err = render_metadata_block("concept", &fields(&[("k", bad.clone())])).unwrap_err();
        match err {
            DomainError::InvalidFieldValue {
                note_type,
                field,
                reason,
            } => {
                assert_eq!(note_type, "concept");
                assert_eq!(field, "(metadata block)");
                assert!(!reason.is_empty(), "{bad}");
            }
            other => panic!("{bad}: {other:?}"),
        }
    }
    let twice = render_metadata_block("concept", &fields(&[("k", json!("a")), ("k", json!("b"))]));
    assert!(matches!(twice, Err(DomainError::InvalidFieldValue { .. })));
}

#[test]
fn check_refuses_a_body_carrying_the_label() {
    let body = "#metadata((type: \"evidence\")) <cdno>\n\nText.\n";
    assert_eq!(check_typst_body(body), Err(BodyRejection::CarriesBlock));
}

#[test]
fn a_label_mentioned_in_a_comment_raw_or_string_is_fine() {
    for body in [
        "// the <cdno> label is the engine's\nText.\n",
        "`<cdno>` is the label.\n",
        "#let s = \"<cdno>\"\nText.\n",
    ] {
        assert_eq!(check_typst_body(body), Ok(()), "{body}");
    }
}

#[test]
fn a_real_label_node_is_refused_wherever_it_sits() {
    assert_eq!(
        check_typst_body("= Setting <cdno>\n"),
        Err(BodyRejection::CarriesBlock)
    );
}

#[test]
fn a_shebang_line_is_refused_because_the_note_will_not_parse() {
    let body = "#!/usr/bin/env typst\nText.\n";
    match check_typst_body(body) {
        Err(BodyRejection::DoesNotParse { line, .. }) => assert_eq!(line, 1),
        other => panic!("{other:?}"),
    }
    let note = assemble_typst_note("#metadata((:)) <cdno>", body);
    assert!(cdno_core::typst::parses(&note).is_err());
}

#[test]
fn link_faults_are_counted_in_the_body_after_the_separator() {
    assert_eq!(
        check_typst_body("Text.\n[[a *b* c]]\n"),
        Err(BodyRejection::LinkNeedsEscape {
            line: 2,
            leaf: "Strong".to_string()
        })
    );
}

#[test]
fn the_link_message_and_remedy_fit_each_kind_of_fault() {
    let texts = |leaf: &str| {
        let e = DomainError::LinkNeedsEscape {
            line: 1,
            leaf: leaf.into(),
        };
        (e.to_string(), e.remedy().unwrap())
    };
    let (m, r) = texts("end of run");
    assert!(
        m.contains("never closed") && r.contains("Close the link"),
        "{m} {r}"
    );
    let (m, r) = texts("Space");
    assert!(
        m.contains("wraps across a line") && r.contains("one line"),
        "{m} {r}"
    );
    let (m, r) = texts("Hash");
    assert!(
        m.contains("`Hash`") && r.contains("Backslash-escape"),
        "{m} {r}"
    );
    assert_eq!(
        check_typst_body("[[a\nb]]\n"),
        Err(BodyRejection::LinkNeedsEscape {
            line: 1,
            leaf: "Space".into()
        })
    );
}

#[test]
fn check_refuses_a_body_that_does_not_parse_with_its_line() {
    let body = "= Setting\n\nFine so far.\n\n#let x = (1 +\n";
    match check_typst_body(body) {
        Err(BodyRejection::DoesNotParse { line, message }) => {
            assert_eq!(line, 5);
            assert!(!message.is_empty());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn check_refuses_an_unescaped_link_with_its_leaf() {
    let body = "= Setting\n\nSee [[concepts/bias-variance-split#a#b]] now.\n";
    assert_eq!(
        check_typst_body(body),
        Err(BodyRejection::LinkNeedsEscape {
            line: 3,
            leaf: "Hash".to_string()
        })
    );
}

#[test]
fn check_accepts_the_rfc_example_body() {
    assert_eq!(check_typst_body(RFC_BODY), Ok(()));
}

#[test]
fn an_assembled_note_is_block_blank_line_body() {
    let block = render_metadata_block("evidence", &fields(&[("type", json!("evidence"))])).unwrap();
    let note = assemble_typst_note(&block, RFC_BODY);
    assert_eq!(note, format!("{block}\n\n{RFC_BODY}"));
    let bare = assemble_typst_note(&block, "Text.");
    assert_eq!(bare, format!("{block}\n\nText.\n"));
}

#[test]
fn every_typst_rejection_carries_a_remedy() {
    let rejections = [
        DomainError::BodyCarriesMetadataBlock,
        DomainError::BodyDoesNotParse {
            line: 3,
            message: "unclosed delimiter".into(),
        },
        DomainError::LinkNeedsEscape {
            line: 3,
            leaf: "Hash".into(),
        },
        DomainError::FormatUnsupportedForType {
            note_type: "project".into(),
        },
        DomainError::VarsUnsupportedForTypst,
        DomainError::StemTakenByOtherFormat {
            stem: "bias-variance-split".into(),
            holder: VaultPath::new("concepts/bias-variance-split.md").unwrap(),
        },
        DomainError::SectionRevisionUnavailableForTypst,
        DomainError::AssetPathOutsideFolder {
            path: "../figs/a.png".into(),
            line: 4,
        },
        DomainError::AssetPathNotLiteral {
            call: "image".into(),
            line: 4,
        },
    ];
    for e in &rejections {
        let remedy = e.remedy().unwrap_or_else(|| panic!("no remedy: {e:?}"));
        assert!(!remedy.trim().is_empty(), "{e:?}");
        assert!(!e.to_string().is_empty(), "{e:?}");
    }
    assert!(DomainError::NoFocus.remedy().is_none());
}
