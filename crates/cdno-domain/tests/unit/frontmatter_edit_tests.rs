//! Unit tests for `merge_fields_into_frontmatter` (#481) — the line-based
//! merge that puts structured metrics into an already-rendered note.
//!
//! The load-bearing property is *byte preservation*: every line the caller did
//! not name must survive exactly, because the merge runs over a note the
//! template engine just produced and anything it reformats is churn a user
//! never asked for.

use cdno_domain::error::DomainError;
use cdno_domain::vault::merge_fields_into_frontmatter;

/// The shape the generic tracking template scaffolds.
const NOTE: &str = "---\ntype: tracking\nstewardship: health\nactivity: body\ndate: 2026-04-06\n---\n\n# Body\n\n## Notes\n";

fn fields(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    match value {
        serde_json::Value::Object(m) => m,
        other => panic!("fixture must be an object, got {other}"),
    }
}

#[test]
fn an_absent_key_is_appended_before_the_closing_delimiter() {
    let out =
        merge_fields_into_frontmatter(NOTE, &fields(serde_json::json!({"weight": 82.5}))).unwrap();

    assert!(
        out.contains("date: 2026-04-06\nweight: 82.5\n---\n"),
        "{out}"
    );
    // Everything else is untouched.
    assert!(out.starts_with("---\ntype: tracking\n"));
    assert!(out.ends_with("\n# Body\n\n## Notes\n"));
}

#[test]
fn an_existing_single_line_key_is_replaced_where_it_stands() {
    // A variant template scaffolds `routine: null`; supplying it must rewrite
    // that line rather than append a duplicate key.
    let note =
        "---\ntype: tracking\nrouting: keep\nroutine: null\ndate: 2026-04-06\n---\n\n# Body\n";
    let out =
        merge_fields_into_frontmatter(note, &fields(serde_json::json!({"routine": "upper-a"})))
            .unwrap();

    assert!(out.contains("routine: upper-a\ndate:"), "{out}");
    assert_eq!(
        out.matches("routine:").count(),
        1,
        "no duplicate key: {out}"
    );
    // A key that merely shares a prefix is not touched.
    assert!(out.contains("routing: keep"), "{out}");
}

#[test]
fn a_record_sequence_is_written_as_an_indented_block() {
    let out = merge_fields_into_frontmatter(
        NOTE,
        &fields(serde_json::json!({"detail": [{"subject": "harmony", "minutes": 25}]})),
    )
    .unwrap();

    // Keys within a record come out alphabetically — `serde_json::Map` is a
    // BTreeMap here — which is cosmetic: a series groups on a named field and
    // orders on the sequence, never on key position.
    assert!(
        out.contains("detail:\n  - minutes: 25\n    subject: harmony\n"),
        "{out}"
    );
    // It must still parse, and the body must be intact.
    let (fm, body) = cdno_core::frontmatter::Frontmatter::parse(&out).unwrap();
    assert_eq!(
        fm.as_json().get("detail").unwrap()[0]["subject"],
        serde_json::json!("harmony")
    );
    assert_eq!(body, "\n# Body\n\n## Notes\n");
}

#[test]
fn several_keys_merge_in_one_pass() {
    let out = merge_fields_into_frontmatter(
        NOTE,
        &fields(serde_json::json!({"weight": 82.5, "note": "after a rest day"})),
    )
    .unwrap();

    let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&out).unwrap();
    let json = fm.as_json();
    assert_eq!(json.get("weight"), Some(&serde_json::json!(82.5)));
    assert_eq!(
        json.get("note"),
        Some(&serde_json::json!("after a rest day"))
    );
}

#[test]
fn a_string_needing_quotes_survives_the_round_trip() {
    // Delegating to serde_yaml is the whole point: a value carrying a colon,
    // or one that would re-read as a bool, must come back as the same string.
    for value in ["18:30 start", "true", "null", "- not a list"] {
        let out = merge_fields_into_frontmatter(NOTE, &fields(serde_json::json!({"note": value})))
            .unwrap();
        let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&out).unwrap();
        assert_eq!(
            fm.as_json().get("note"),
            Some(&serde_json::json!(value)),
            "`{value}` must round-trip as a string: {out}"
        );
    }
}

#[test]
fn a_key_already_carrying_a_nested_block_errors_rather_than_guessing() {
    // Replacing it means consuming an unknown number of continuation lines;
    // guessing wrong silently drops or duplicates data.
    let note = "---\ntype: tracking\ndetail:\n  - subject: harmony\n    minutes: 25\ndate: 2026-04-06\n---\n\n# Body\n";
    match merge_fields_into_frontmatter(note, &fields(serde_json::json!({"detail": [1]}))) {
        Err(DomainError::MultilineFrontmatterField(field)) => assert_eq!(field, "detail"),
        other => panic!("expected MultilineFrontmatterField(detail), got {other:?}"),
    }
}

#[test]
fn an_empty_field_set_is_the_document_unchanged() {
    let out = merge_fields_into_frontmatter(NOTE, &serde_json::Map::new()).unwrap();
    assert_eq!(out, NOTE);
}

#[test]
fn a_document_without_frontmatter_errors() {
    match merge_fields_into_frontmatter("# Just a body\n", &fields(serde_json::json!({"a": 1}))) {
        Err(DomainError::MissingSection("frontmatter")) => {}
        other => panic!("expected MissingSection(frontmatter), got {other:?}"),
    }
}
