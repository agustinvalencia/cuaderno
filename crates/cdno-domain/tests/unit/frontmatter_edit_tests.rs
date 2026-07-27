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
fn a_key_already_carrying_a_nested_block_is_replaced_wholesale() {
    // The caller passes the COMPLETE new value, so the old block's
    // continuation lines go with it. Their extent is not a guess: a
    // continuation is a line `declared_key` does not recognise, the same
    // definition `normalise` uses to move a key's line-group.
    let note = "---\ntype: tracking\ndetail:\n  - subject: harmony\n    minutes: 25\ndate: 2026-04-06\n---\n\n# Body\n";
    let out = merge_fields_into_frontmatter(
        note,
        &fields(serde_json::json!({"detail": [{"subject": "scales"}]})),
    )
    .unwrap();

    let (fm, body) = cdno_core::frontmatter::Frontmatter::parse(&out).unwrap();
    let detail = fm.as_json();
    let detail = detail["detail"].as_array().unwrap();
    assert_eq!(detail.len(), 1, "the old records are gone: {out}");
    assert_eq!(detail[0]["subject"], serde_json::json!("scales"));
    // Neighbouring keys and the body are untouched.
    assert!(out.contains("date: 2026-04-06"), "{out}");
    assert_eq!(body, "\n# Body\n");
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

// ---------------------------------------------------------------------
// Untrusted keys and values (review findings on #494)
// ---------------------------------------------------------------------

#[test]
fn a_single_key_mapping_is_still_written_as_a_block() {
    // Block-vs-inline follows the value's SHAPE, not whether its
    // serialisation happens to fit on one line: `{"duration": 45}` serialises
    // to `duration: 45`, which inlined would read `session: duration: 45` —
    // invalid YAML, and the whole write dies on a scanner error.
    let out = merge_fields_into_frontmatter(
        NOTE,
        &fields(serde_json::json!({"session": {"duration": 45}})),
    )
    .unwrap();

    assert!(out.contains("session:\n  duration: 45\n"), "{out}");
    let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&out).unwrap();
    assert_eq!(fm.as_json()["session"]["duration"], serde_json::json!(45));
}

#[test]
fn a_single_element_sequence_is_still_written_as_a_block() {
    let out =
        merge_fields_into_frontmatter(NOTE, &fields(serde_json::json!({"detail": [{"a": 1}]})))
            .unwrap();
    let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&out).unwrap();
    assert_eq!(fm.as_json()["detail"].as_array().unwrap().len(), 1);
}

#[test]
fn an_empty_collection_stays_inline() {
    // It has no block form; `key:\n  []` would be a needless second line.
    for (value, expected) in [
        (serde_json::json!([]), "detail: []"),
        (serde_json::json!({}), "detail: {}"),
    ] {
        let out =
            merge_fields_into_frontmatter(NOTE, &fields(serde_json::json!({"detail": value})))
                .unwrap();
        assert!(out.contains(expected), "expected `{expected}` in:\n{out}");
        cdno_core::frontmatter::Frontmatter::parse(&out).expect("must parse");
    }
}

#[test]
fn a_key_needing_quotes_is_quoted_rather_than_reinterpreted() {
    // The keys are caller-supplied. `#reps` interpolated raw is a YAML
    // comment — the metric would vanish while the write reported success.
    for key in ["#reps", "true", "a: b", "- dash"] {
        let out =
            merge_fields_into_frontmatter(NOTE, &fields(serde_json::json!({key: 10}))).unwrap();
        let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&out)
            .unwrap_or_else(|e| panic!("`{key}` must produce parseable YAML: {e}\n{out}"));
        assert_eq!(
            fm.as_json().get(key),
            Some(&serde_json::json!(10)),
            "`{key}` must survive as its own key: {out}"
        );
    }
}

#[test]
fn a_key_carrying_a_line_break_is_refused() {
    // Interpolating it would smuggle a second frontmatter line past the
    // caller's own validation — a metric named `x: 1\nweight` writing a
    // `weight` the schema check never saw.
    match merge_fields_into_frontmatter(NOTE, &fields(serde_json::json!({"x: 1\nweight": "heavy"})))
    {
        Err(DomainError::UnrepresentableFrontmatterValue { field, .. }) => {
            assert!(field.contains("weight"), "field: {field}")
        }
        other => panic!("expected UnrepresentableFrontmatterValue, got {other:?}"),
    }
}

#[test]
fn a_block_scalar_takes_its_continuation_lines_with_it() {
    // `notes: |` carries a non-empty inline value AND continues onto the next
    // lines. They must go when it is replaced - orphaning them either breaks
    // the document or, when they look like mapping entries, silently absorbs
    // them into the replacement value.
    let note =
        "---\ntype: tracking\nnotes: |\n  warm-up\n  main set\ndate: 2026-04-06\n---\n\n# Body\n";
    let out = merge_fields_into_frontmatter(note, &fields(serde_json::json!({"notes": "quick"})))
        .unwrap();

    let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&out).unwrap();
    assert_eq!(fm.as_json()["notes"], serde_json::json!("quick"));
    assert!(!out.contains("warm-up"), "orphaned line survived: {out}");
    assert!(out.contains("date: 2026-04-06"), "{out}");
}

#[test]
fn a_crlf_document_is_merged_with_its_own_line_endings() {
    // `Frontmatter::parse` accepts CRLF, so a merge that only understood LF
    // would report "missing frontmatter" for a note the parser reads fine —
    // and a user-authored template is exactly what arrives CRLF-terminated.
    let note =
        "---\r\ntype: tracking\r\nactivity: body\r\ndate: 2026-04-06\r\n---\r\n\r\n# Body\r\n";
    let out =
        merge_fields_into_frontmatter(note, &fields(serde_json::json!({"weight": 82.5}))).unwrap();

    assert!(out.contains("weight: 82.5\r\n"), "{out:?}");
    assert!(
        !out.contains("weight: 82.5\n\r"),
        "no mixed endings: {out:?}"
    );
    let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&out).unwrap();
    assert_eq!(fm.as_json().get("weight"), Some(&serde_json::json!(82.5)));
}

#[test]
fn a_quoted_existing_key_is_replaced_rather_than_duplicated() {
    let note = "---\ntype: tracking\n\"weight\": 80\ndate: 2026-04-06\n---\n\n# Body\n";
    let out =
        merge_fields_into_frontmatter(note, &fields(serde_json::json!({"weight": 82.5}))).unwrap();

    assert_eq!(
        out.matches("weight").count(),
        1,
        "the existing key must be replaced, not duplicated: {out}"
    );
    let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&out).unwrap();
    assert_eq!(fm.as_json().get("weight"), Some(&serde_json::json!(82.5)));
}

#[test]
fn a_key_broken_by_a_unicode_line_separator_is_refused() {
    // YAML counts U+2028/U+2029 as line breaks, and the emitter folds a key
    // containing one across two physical lines — so an input-side `\n`/`\r`
    // check passes it through and yields an unparseable block returned as Ok.
    for key in ["a\u{2028}type", "a\u{2029}type", "\u{2028}"] {
        match merge_fields_into_frontmatter(NOTE, &fields(serde_json::json!({key: "hijacked"}))) {
            Err(DomainError::UnrepresentableFrontmatterValue { .. }) => {}
            Ok(out) => panic!("`{key:?}` must be refused, got:\n{out}"),
            other => panic!("expected UnrepresentableFrontmatterValue, got {other:?}"),
        }
    }
}

#[test]
fn a_multiline_string_in_a_crlf_document_keeps_crlf_throughout() {
    // A string carrying a newline is emitted as a literal block, so even the
    // inline branch spans lines. Splicing it verbatim would leave LF-separated
    // lines inside an otherwise CRLF note.
    let note =
        "---\r\ntype: tracking\r\nactivity: body\r\ndate: 2026-04-06\r\n---\r\n\r\n# Body\r\n";
    let out =
        merge_fields_into_frontmatter(note, &fields(serde_json::json!({"notes": "line1\nline2"})))
            .unwrap();

    assert!(
        !out.replace("\r\n", "").contains('\n'),
        "no bare LF may survive in a CRLF document: {out:?}"
    );
    // And the value still round-trips — YAML normalises breaks back to `\n`.
    let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&out).unwrap();
    assert_eq!(
        fm.as_json().get("notes"),
        Some(&serde_json::json!("line1\nline2"))
    );
}

#[test]
fn a_multiline_string_round_trips_in_an_lf_document() {
    let out =
        merge_fields_into_frontmatter(NOTE, &fields(serde_json::json!({"notes": "line1\nline2"})))
            .unwrap();
    let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&out).unwrap();
    assert_eq!(
        fm.as_json().get("notes"),
        Some(&serde_json::json!("line1\nline2"))
    );
}

#[test]
fn a_block_scalar_containing_a_blank_line_is_replaced_whole() {
    // A blank line does not end a block scalar. Stopping there orphans the
    // rest, which then reads back as part of the REPLACEMENT value — the
    // "orphaned text absorbed into the replacement" failure, silently.
    let note =
        "---\ntype: tracking\nnotes: |\n  warm up\n\n  main set\nweight: 80\n---\n\n# Body\n";
    let out = merge_fields_into_frontmatter(note, &fields(serde_json::json!({"notes": "quick"})))
        .unwrap();

    let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&out).unwrap();
    assert_eq!(
        fm.as_json()["notes"],
        serde_json::json!("quick"),
        "no orphan may be absorbed: {out}"
    );
    assert!(!out.contains("main set"), "orphaned line survived: {out}");
    // The key after the block is untouched.
    assert_eq!(fm.as_json()["weight"], serde_json::json!(80));
}

#[test]
fn a_blank_line_between_keys_is_left_where_it_is() {
    // The look-past-blanks rule must not swallow a blank that merely
    // separates two top-level keys.
    let note = "---\ntype: tracking\nweight: 80\n\ndate: 2026-04-10\n---\n\n# Body\n";
    let out =
        merge_fields_into_frontmatter(note, &fields(serde_json::json!({"weight": 82.5}))).unwrap();

    assert!(out.contains("weight: 82.5\n\ndate: 2026-04-10"), "{out:?}");
}
