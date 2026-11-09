//! The Typst block writer, the field transport and the creation-time body check (RFC 0008 §5.7).

use std::collections::BTreeMap;
use std::sync::Arc;

use cdno_core::config::{TypstConfig, VaultConfig};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::error::DomainError;
use cdno_domain::vault::typst_note::{
    BodyRejection, assemble_typst_note, check_typst_body, render_metadata_block,
};
use cdno_domain::{SearchFilters, Vault};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
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

// ---------------------------------------------------------------------
// file_typst_evidence (T19)
// ---------------------------------------------------------------------

fn at() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 11, 8)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 30, 0).unwrap())
}

const SOURCE: &str = "Error budget derivation";
const ORIGIN: &str = "projects/surrogate-model";
const DAILY: &str = "journal/2026/daily/2026-11-08.md";

fn evidence_vault(config: VaultConfig) -> (Vault, Arc<dyn VaultStore>, Arc<dyn VaultIndex>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _) =
        Vault::new(Arc::clone(&store), Arc::clone(&index), config).expect("Vault::new");
    vault
        .create_portfolio(at(), "Where does the surrogate error come from?", None)
        .expect("portfolio");
    (vault, store, index)
}

fn snapshot(store: &Arc<dyn VaultStore>) -> BTreeMap<String, String> {
    store
        .walk_dir(&VaultPath::root())
        .unwrap()
        .into_iter()
        .map(|p| (p.to_string(), store.read_file(&p).unwrap_or_default()))
        .collect()
}

fn portfolio_slug(store: &Arc<dyn VaultStore>) -> String {
    store
        .walk_dir(&VaultPath::root())
        .unwrap()
        .into_iter()
        .find_map(|p| {
            p.to_string()
                .strip_prefix("portfolios/")
                .and_then(|r| r.strip_suffix("/_index.md"))
                .map(str::to_string)
        })
        .expect("portfolio slug")
}

#[test]
fn file_typst_evidence_writes_block_then_body_and_logs() {
    let (vault, store, index) = evidence_vault(VaultConfig::default());
    let portfolio = portfolio_slug(&store);
    let path = vault
        .file_typst_evidence(at(), &portfolio, SOURCE, ORIGIN, RFC_BODY)
        .unwrap();
    let expected_path = format!("portfolios/{portfolio}/2026-11-08-error-budget-derivation.typ");
    assert_eq!(path.to_string(), expected_path);

    let block = render_metadata_block(
        "evidence",
        &fields(&[
            ("type", json!("evidence")),
            ("created", json!("2026-11-08")),
            ("source", json!(SOURCE)),
            ("portfolio", json!(portfolio)),
            ("origin", json!("[[projects/surrogate-model]]")),
        ]),
    )
    .unwrap();
    assert_eq!(
        store.read_file(&path).unwrap(),
        assemble_typst_note(&block, RFC_BODY)
    );

    let daily = store.read_file(&VaultPath::new(DAILY).unwrap()).unwrap();
    let line = daily
        .lines()
        .find(|l| l.contains("evidence created"))
        .expect("created line");
    assert!(
        line.contains(&format!(
            "[[portfolios/{portfolio}/2026-11-08-error-budget-derivation]] \u{2014} {SOURCE}"
        )),
        "{line}"
    );
    assert!(!line.contains(".typ"), "{line}");

    let row = index
        .list_by_type("evidence")
        .unwrap()
        .into_iter()
        .find(|e| e.path == path)
        .expect("indexed");
    assert_eq!(row.frontmatter["portfolio"], json!(portfolio));
    assert_eq!(row.frontmatter["source"], json!(SOURCE));
    assert_eq!(
        row.frontmatter["origin"],
        json!("[[projects/surrogate-model]]")
    );
}

#[test]
fn the_new_note_is_in_portfolio_contents_and_search_at_once() {
    let (vault, store, _) = evidence_vault(VaultConfig::default());
    let portfolio = portfolio_slug(&store);
    let path = vault
        .file_typst_evidence(at(), &portfolio, SOURCE, ORIGIN, RFC_BODY)
        .unwrap();
    let contents = vault.get_portfolio_contents(&portfolio).unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].0, path);
    let hits = vault
        .search("bias variance", &SearchFilters::default(), 10)
        .unwrap();
    assert!(hits.iter().any(|h| h.path == path), "{hits:?}");
}

#[test]
fn a_body_with_an_unescaped_hash_is_refused_and_nothing_is_written() {
    let (vault, store, _) = evidence_vault(VaultConfig::default());
    let portfolio = portfolio_slug(&store);
    let before = snapshot(&store);
    assert!(before.keys().any(|k| k.ends_with("_index.md")));
    let err = vault
        .file_typst_evidence(
            at(),
            &portfolio,
            SOURCE,
            ORIGIN,
            "= Setting\n\nSee [[concepts/bias-variance-split#anchor]].\n",
        )
        .unwrap_err();
    assert!(
        matches!(err, DomainError::LinkNeedsEscape { .. }),
        "{err:?}"
    );
    assert_eq!(snapshot(&store), before);
}

#[test]
fn a_typ_whose_stem_matches_an_md_is_refused() {
    let (vault, store, _) = evidence_vault(VaultConfig::default());
    let portfolio = portfolio_slug(&store);
    vault
        .file_evidence(at(), &portfolio, SOURCE, ORIGIN, "Notes.")
        .unwrap();
    let before = snapshot(&store);
    let err = vault
        .file_typst_evidence(at(), &portfolio, SOURCE, ORIGIN, RFC_BODY)
        .unwrap_err();
    match &err {
        DomainError::StemTakenByOtherFormat { holder, .. } => {
            assert_eq!(
                holder.to_string(),
                format!("portfolios/{portfolio}/2026-11-08-error-budget-derivation.md")
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(err.to_string().contains("error-budget-derivation.md"));
    assert_eq!(snapshot(&store), before);
}

#[test]
fn an_md_whose_stem_matches_a_typ_is_refused() {
    let (vault, store, _) = evidence_vault(VaultConfig::default());
    let portfolio = portfolio_slug(&store);
    vault
        .file_typst_evidence(at(), &portfolio, SOURCE, ORIGIN, RFC_BODY)
        .unwrap();
    let before = snapshot(&store);
    let err = vault
        .file_evidence(at(), &portfolio, SOURCE, ORIGIN, "Notes.")
        .unwrap_err();
    match &err {
        DomainError::StemTakenByOtherFormat { holder, .. } => {
            assert!(holder.to_string().ends_with("error-budget-derivation.typ"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(snapshot(&store), before);

    let artefact = std::env::temp_dir().join("cdno-t19-figure.png");
    std::fs::write(&artefact, b"png").unwrap();
    let err = vault
        .file_attachment(at(), &portfolio, &artefact, SOURCE, ORIGIN, "Figure.")
        .unwrap_err();
    std::fs::remove_file(&artefact).ok();
    assert!(
        matches!(err, DomainError::StemTakenByOtherFormat { .. }),
        "{err:?}"
    );
    assert_eq!(snapshot(&store), before);
}

#[test]
fn typst_off_refuses_with_format_unsupported() {
    let config = VaultConfig {
        typst: TypstConfig {
            note_types: Some(Vec::new()),
        },
        ..VaultConfig::default()
    };
    let (vault, store, _) = evidence_vault(config);
    let portfolio = portfolio_slug(&store);
    let before = snapshot(&store);
    let err = vault
        .file_typst_evidence(at(), &portfolio, SOURCE, ORIGIN, RFC_BODY)
        .unwrap_err();
    assert!(
        matches!(err, DomainError::FormatUnsupportedForType { .. }),
        "{err:?}"
    );
    assert_eq!(snapshot(&store), before);
}
