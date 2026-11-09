//! The Typst block writer, the field transport and the creation-time body check (RFC 0008 §5.7).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use cdno_core::config::{
    CustomNoteType, FieldSpec, FieldType, SchemaExtension, TypstConfig, VaultConfig,
};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::paths::NoteFormat;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::SearchFilters;
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::vault::typst_note::{
    BodyRejection, assemble_typst_note, check_typst_body, render_metadata_block,
};
use chrono::{NaiveDate, NaiveDateTime};
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
fn values_are_written_as_their_own_type() {
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
    assert!(block.contains("n: 2,"), "{block}");
    assert!(block.contains("draft: true,"), "{block}");
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
// create_typst_custom_note (T26)
// ---------------------------------------------------------------------

fn at() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 11, 8)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap()
}

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn spec(ty: FieldType) -> FieldSpec {
    toml::from_str(&format!("type = \"{}\"", ty.as_str())).unwrap()
}

fn concept_config() -> VaultConfig {
    let mut config = VaultConfig::default();
    config.note_types.insert(
        "concept".to_owned(),
        CustomNoteType {
            folder: "concepts".to_owned(),
            required: vec!["created".to_owned()],
            optional: vec![
                "tags".to_owned(),
                "origin".to_owned(),
                "priority".to_owned(),
                "ratio".to_owned(),
                "settled".to_owned(),
                "stage".to_owned(),
                "reviewed".to_owned(),
            ],
            template: None,
            append_only: false,
            title_field: None,
            date_field: None,
        },
    );
    let mut schema = SchemaExtension::default();
    schema
        .fields
        .insert("priority".into(), spec(FieldType::Int));
    schema.fields.insert("ratio".into(), spec(FieldType::Float));
    schema
        .fields
        .insert("settled".into(), spec(FieldType::Bool));
    schema.fields.insert(
        "stage".into(),
        toml::from_str("type = \"string\"\nvalues = [\"draft\", \"settled\"]").unwrap(),
    );
    schema
        .fields
        .insert("reviewed".into(), spec(FieldType::Date));
    config.schemas.insert("concept".to_owned(), schema);
    config
}

fn vault(config: VaultConfig) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(Arc::clone(&store), index, config).expect("Vault::new");
    (vault, store)
}

fn string_fields(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

const BODY: &str = "The error splits as bias squared plus variance.\n\n$ sigma^2 $";

#[test]
fn create_typst_concept_writes_block_body_and_logs() {
    let (vault, store) = vault(concept_config());
    let path = vault
        .create_typst_custom_note(
            at(),
            "concept",
            "Bias-variance split",
            &HashMap::new(),
            Some(BODY),
            Some("[[projects/surrogate-model]]"),
            None,
        )
        .unwrap();
    assert_eq!(path.to_string(), "concepts/bias-variance-split.typ");
    let content = store.read_file(&path).unwrap();
    assert_eq!(
        content,
        format!(
            "#metadata((\n  type: \"concept\",\n  title: \"Bias-variance split\",\n  \
             created: \"2026-11-08\",\n  origin: \"[[projects/surrogate-model]]\",\n)) \
             <cdno>\n\n{BODY}\n"
        )
    );
    assert!(!content.contains("= Bias-variance split"));
    let daily = vault.read_daily_note(at().date()).unwrap().markdown;
    assert!(
        daily.contains(
            "concept created [[concepts/bias-variance-split]] \u{2014} Bias-variance split"
        ),
        "{daily}"
    );
    assert!(!daily.contains(".typ"), "{daily}");
    assert!(vault.list_custom_notes("concept").unwrap().contains(&path));
}

#[test]
fn create_typst_concept_refuses_a_type_that_does_not_admit_typst() {
    let mut config = concept_config();
    config.typst.note_types = Some(vec![]);
    let (vault, store) = vault(config);
    let err = vault
        .create_typst_custom_note(at(), "concept", "X", &HashMap::new(), None, None, None)
        .unwrap_err();
    assert!(matches!(err, DomainError::FormatUnsupportedForType { .. }));
    assert!(!store.exists(&vp("concepts/x.typ")).unwrap());
}

#[test]
fn create_typst_concept_refuses_vars() {
    let (vault, store) = vault(concept_config());
    let err = vault
        .create_custom_note_in_format(
            at(),
            NoteFormat::Typst,
            "concept",
            "Bias-variance split",
            &HashMap::new(),
            &string_fields(&[("who", "me")]),
            None,
            None,
            None,
        )
        .unwrap_err();
    assert!(matches!(err, DomainError::VarsUnsupportedForTypst));
    assert!(
        !store
            .exists(&vp("concepts/bias-variance-split.typ"))
            .unwrap()
    );
}

#[test]
fn concept_stem_exclusivity_both_ways() {
    let (vault, store) = vault(concept_config());
    // A file the index never saw, so only the file check can catch it.
    store
        .write_file(
            &vp("concepts/bias-variance-split.md"),
            "---\ntype: concept\n---\n",
        )
        .unwrap();
    let err = vault
        .create_typst_custom_note(
            at(),
            "concept",
            "Bias-variance split",
            &HashMap::new(),
            None,
            None,
            None,
        )
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::StemTakenByOtherFormat { stem, holder }
            if stem == "bias-variance-split"
                && holder.to_string() == "concepts/bias-variance-split.md"),
        "{err:?}"
    );

    store
        .write_file(&vp("concepts/surrogate-bias.typ"), "= Unlabelled\n")
        .unwrap();
    let err = vault
        .create_custom_note(at(), "concept", "Surrogate bias", &HashMap::new())
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::StemTakenByOtherFormat { stem, .. }
            if stem == "surrogate-bias"),
        "{err:?}"
    );
    assert!(!store.exists(&vp("concepts/surrogate-bias.md")).unwrap());
}

#[test]
fn a_blank_title_is_refused() {
    let (vault, _store) = vault(concept_config());
    let err = vault
        .create_typst_custom_note(at(), "concept", "   ", &HashMap::new(), None, None, None)
        .unwrap_err();
    assert!(matches!(err, DomainError::EmptyField { field: "title" }));
}

#[test]
fn declared_int_float_and_bool_fields_are_written_as_their_type() {
    let (vault, store) = vault(concept_config());
    let path = vault
        .create_typst_custom_note(
            at(),
            "concept",
            "Bias-variance split",
            &string_fields(&[("priority", "3"), ("ratio", "0.5"), ("settled", "true")]),
            None,
            None,
            None,
        )
        .unwrap();
    let content = store.read_file(&path).unwrap();
    assert!(content.contains("  priority: 3,\n"), "{content}");
    assert!(content.contains("  ratio: 0.5,\n"), "{content}");
    assert!(content.contains("  settled: true,\n"), "{content}");
    let report = vault
        .lint_all_notes(NaiveDate::from_ymd_opt(2026, 11, 8).unwrap())
        .unwrap();
    let rows: Vec<_> = report.issues.iter().filter(|i| i.path == path).collect();
    assert!(rows.is_empty(), "{rows:?}");
}

#[test]
fn a_value_that_does_not_fit_its_declared_type_is_refused() {
    let (vault, store) = vault(concept_config());
    let err = vault
        .create_typst_custom_note(
            at(),
            "concept",
            "Bias-variance split",
            &string_fields(&[("priority", "high")]),
            None,
            None,
            None,
        )
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::InvalidFieldValue { field, .. } if field == "priority"),
        "{err:?}"
    );
    assert!(
        !store
            .exists(&vp("concepts/bias-variance-split.typ"))
            .unwrap()
    );
}

#[test]
fn a_value_outside_its_enum_or_a_bad_date_is_refused() {
    let (vault, store) = vault(concept_config());
    for (key, value) in [("stage", "archived"), ("reviewed", "2026-13-40")] {
        let err = vault
            .create_typst_custom_note(
                at(),
                "concept",
                "Bias-variance split",
                &string_fields(&[(key, value)]),
                None,
                None,
                None,
            )
            .unwrap_err();
        assert!(
            matches!(&err, DomainError::InvalidFieldValue { field, .. } if field == key),
            "{key}: {err:?}"
        );
    }
    assert!(
        !store
            .exists(&vp("concepts/bias-variance-split.typ"))
            .unwrap()
    );
}

#[test]
fn create_typst_refuses_a_bad_body_and_writes_nothing() {
    let (vault, store) = vault(concept_config());
    for (body, expect) in [
        ("#metadata((type: \"concept\")) <cdno>\n", "carries"),
        ("#let x = (\n", "parse"),
        ("See [[concepts/a#b]].\n", "link"),
    ] {
        let err = vault
            .create_typst_custom_note(
                at(),
                "concept",
                "Bias-variance split",
                &HashMap::new(),
                Some(body),
                None,
                None,
            )
            .unwrap_err();
        let ok = match expect {
            "carries" => matches!(err, DomainError::BodyCarriesMetadataBlock),
            "parse" => matches!(err, DomainError::BodyDoesNotParse { .. }),
            _ => matches!(err, DomainError::LinkNeedsEscape { .. }),
        };
        assert!(ok, "{body}: {err:?}");
    }
    assert!(
        !store
            .exists(&vp("concepts/bias-variance-split.typ"))
            .unwrap()
    );
}

// ---------------------------------------------------------------------
// file_typst_evidence (T19)
// ---------------------------------------------------------------------

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
        DomainError::StemTakenByOtherFormat { stem, holder } => {
            assert_eq!(stem, "2026-11-08-error-budget-derivation");
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
