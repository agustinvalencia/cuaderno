//! The surgical config writer (#365, PR5b): `toml_edit`-backed edits that
//! touch ONLY the one `[note_types.<name>]` / `[schemas.<type>.fields.<name>]`
//! table and preserve every other comment, table, and key order. These
//! prove the string-in/string-out contract the Config form relies on — the
//! save gate never sees a whole-document re-serialise.

use cdno_core::config::{CustomNoteType, FieldSpec, FieldType};
use cdno_core::config_edit::{
    remove_note_type, remove_schema_field, set_note_type, set_schema_field,
};
use cdno_core::error::ConfigEditError;

/// A note type with only the required `folder` set — the minimal shape a
/// `set_note_type` writes (no arrays, no optional scalars, not append-only).
fn note_type(folder: &str) -> CustomNoteType {
    CustomNoteType {
        folder: folder.to_string(),
        required: Vec::new(),
        optional: Vec::new(),
        template: None,
        append_only: false,
        title_field: None,
        date_field: None,
    }
}

/// A field spec of the given type with everything else absent — the
/// minimal `set_schema_field` shape.
fn field_spec(ty: FieldType) -> FieldSpec {
    FieldSpec {
        ty,
        default: None,
        required: false,
        values: None,
        list: None,
        settable: None,
        log_on_change: None,
    }
}

/// A hand-annotated config with a comment and one existing custom type, so
/// a preservation test can prove the surrounding bytes survive an edit.
const ANNOTATED: &str = "\
# vault config — hand-annotated
[vault]
name = \"Demo\"

# people we work with
[note_types.person]
folder = \"people\"
";

#[test]
fn set_note_type_appends_and_preserves_comments_and_other_tables() {
    let out = set_note_type(ANNOTATED, "widget", &note_type("widgets")).expect("set");

    // Every original byte still leads the document — the comment, the
    // [vault] table, and the existing [note_types.person] are untouched.
    assert!(
        out.starts_with(ANNOTATED),
        "the original document must be preserved verbatim as a prefix; got:\n{out}"
    );
    // The new table was appended with just its folder.
    assert!(out.contains("[note_types.widget]"));
    assert!(out.contains("folder = \"widgets\""));
    // No spurious bare parent header.
    assert!(!out.contains("\n[note_types]\n"));
}

#[test]
fn set_note_type_edits_only_the_named_table_in_place() {
    // Re-point `person`'s folder; `[vault]` and the comments must not move.
    let out = set_note_type(ANNOTATED, "person", &note_type("folks")).expect("set");

    assert!(out.contains("folder = \"folks\""));
    assert!(!out.contains("folder = \"people\""));
    // Untouched context survives.
    assert!(out.contains("# vault config — hand-annotated"));
    assert!(out.contains("# people we work with"));
    assert!(out.contains("name = \"Demo\""));
    // The header comment above the edited table is preserved (in-place
    // mutation, not a wholesale table replacement).
    let person_at = out.find("[note_types.person]").expect("person header");
    let comment_at = out.find("# people we work with").expect("header comment");
    assert!(
        comment_at < person_at,
        "the header comment stays above the table"
    );
}

#[test]
fn set_note_type_writes_the_full_minimal_key_set() {
    let ty = CustomNoteType {
        folder: "reading".to_string(),
        required: vec!["author".to_string()],
        optional: vec!["rating".to_string(), "isbn".to_string()],
        template: Some("reading.md".to_string()),
        append_only: true,
        title_field: Some("author".to_string()),
        date_field: None,
    };
    let out = set_note_type("", "reading", &ty).expect("set");

    assert!(out.contains("[note_types.reading]"));
    assert!(out.contains("folder = \"reading\""));
    assert!(out.contains("required = [\"author\"]"));
    assert!(out.contains("optional = [\"rating\", \"isbn\"]"));
    assert!(out.contains("template = \"reading.md\""));
    assert!(out.contains("append_only = true"));
    assert!(out.contains("title_field = \"author\""));
    // `date_field` is None — it must NOT appear.
    assert!(!out.contains("date_field"));
}

#[test]
fn set_note_type_removes_keys_the_model_no_longer_sets() {
    // Start with a fully-populated type, then re-set it to the minimal
    // shape: every optional key must be dropped, folder kept.
    let full = CustomNoteType {
        folder: "widgets".to_string(),
        required: vec!["x".to_string()],
        optional: vec!["y".to_string()],
        template: Some("widget.md".to_string()),
        append_only: true,
        title_field: Some("x".to_string()),
        date_field: None,
    };
    let populated = set_note_type("", "widget", &full).expect("set full");
    let trimmed = set_note_type(&populated, "widget", &note_type("widgets")).expect("re-set");

    assert!(trimmed.contains("folder = \"widgets\""));
    for dropped in [
        "required",
        "optional",
        "template",
        "append_only",
        "title_field",
    ] {
        assert!(
            !trimmed.contains(dropped),
            "`{dropped}` should have been removed"
        );
    }
}

#[test]
fn set_schema_field_writes_type_default_required_and_values() {
    let mut spec = field_spec(FieldType::String);
    spec.default = Some(toml::Value::String("idea".to_string()));
    spec.required = true;
    spec.values = Some(vec![
        "idea".to_string(),
        "active".to_string(),
        "done".to_string(),
    ]);

    let out = set_schema_field("", "project", "stage", &spec).expect("set field");

    assert!(out.contains("[schemas.project.fields.stage]"));
    assert!(out.contains("type = \"string\""));
    assert!(out.contains("default = \"idea\""));
    assert!(out.contains("required = true"));
    assert!(out.contains("values = [\"idea\", \"active\", \"done\"]"));
    // No bare intermediate headers leak.
    assert!(!out.contains("\n[schemas]\n"));
    assert!(!out.contains("\n[schemas.project]\n"));
}

#[test]
fn set_schema_field_writes_scalar_defaults_by_type() {
    let mut int_spec = field_spec(FieldType::Int);
    int_spec.default = Some(toml::Value::Integer(3));
    let out = set_schema_field("", "project", "priority", &int_spec).expect("int");
    assert!(
        out.contains("default = 3"),
        "int default is a bare number: {out}"
    );

    let mut bool_spec = field_spec(FieldType::Bool);
    bool_spec.default = Some(toml::Value::Boolean(false));
    let out = set_schema_field("", "project", "blocked", &bool_spec).expect("bool");
    assert!(
        out.contains("default = false"),
        "bool default is a bare bool: {out}"
    );
}

#[test]
fn set_schema_field_preserves_reserved_keys_and_sibling_fields() {
    // A field hand-authored with a reserved `settable` key, plus a sibling
    // field. Editing `stage`'s type must keep `settable` and the sibling.
    let existing = "\
[schemas.project.fields.stage]
type = \"string\"
settable = true

[schemas.project.fields.owner]
type = \"string\"
";
    let out = set_schema_field(existing, "project", "stage", &field_spec(FieldType::Date))
        .expect("edit stage");

    assert!(out.contains("type = \"date\""));
    // Reserved key on the edited field survives (the form never touches it).
    assert!(
        out.contains("settable = true"),
        "reserved key must survive: {out}"
    );
    // The sibling field is untouched.
    assert!(out.contains("[schemas.project.fields.owner]"));
}

#[test]
fn set_schema_field_preserves_extra_required_on_the_schema() {
    // A `[schemas.project]` that already declares `extra_required` keeps
    // that header + key when a typed field is added underneath it.
    let existing = "\
[schemas.project]
extra_required = [\"sponsor\"]
";
    let out = set_schema_field(existing, "project", "stage", &field_spec(FieldType::String))
        .expect("add field");

    assert!(out.contains("[schemas.project]"));
    assert!(out.contains("extra_required = [\"sponsor\"]"));
    assert!(out.contains("[schemas.project.fields.stage]"));
}

#[test]
fn remove_note_type_drops_only_that_table() {
    let two = "\
[note_types.person]
folder = \"people\"

[note_types.widget]
folder = \"widgets\"
";
    let out = remove_note_type(two, "person").expect("remove");

    assert!(!out.contains("[note_types.person]"));
    assert!(!out.contains("folder = \"people\""));
    // The other type is untouched.
    assert!(out.contains("[note_types.widget]"));
    assert!(out.contains("folder = \"widgets\""));
}

#[test]
fn remove_of_an_absent_table_is_a_noop_success() {
    // Removing a type that isn't declared returns the document unchanged.
    let out = remove_note_type(ANNOTATED, "ghost").expect("idempotent remove");
    assert_eq!(out, ANNOTATED);

    // Same for an absent schema field (missing schema entirely).
    let out = remove_schema_field(ANNOTATED, "project", "stage").expect("idempotent");
    assert_eq!(out, ANNOTATED);
}

#[test]
fn remove_schema_field_drops_only_that_field() {
    let two = "\
[schemas.project.fields.stage]
type = \"string\"

[schemas.project.fields.owner]
type = \"string\"
";
    let out = remove_schema_field(two, "project", "stage").expect("remove");

    assert!(!out.contains("fields.stage"));
    assert!(out.contains("[schemas.project.fields.owner]"));
}

#[test]
fn set_then_remove_round_trips_to_the_original() {
    // Adding a type then removing it returns the document to its original
    // text (the implicit `note_types` parent renders nothing when empty).
    let added = set_note_type(ANNOTATED, "widget", &note_type("widgets")).expect("add");
    assert_ne!(added, ANNOTATED);
    let back = remove_note_type(&added, "widget").expect("remove");
    assert_eq!(
        back, ANNOTATED,
        "set then remove must restore the original text"
    );
}

#[test]
fn a_parse_error_is_reported_not_clobbered() {
    // An unterminated table header is a hard parse error — the editor
    // never writes over an unparseable buffer.
    let broken = "[note_types.person\nfolder = \"people\"\n";
    let err = set_note_type(broken, "widget", &note_type("widgets"))
        .expect_err("a broken buffer must not be silently rewritten");
    assert!(matches!(err, ConfigEditError::Parse(_)));
}

#[test]
fn a_wrong_shaped_key_is_refused_not_overwritten() {
    // `note_types` is a scalar, not a table — refusing to clobber it is
    // the NotATable guard.
    let wrong = "note_types = 5\n";
    let err = set_note_type(wrong, "widget", &note_type("widgets"))
        .expect_err("a scalar where a table is needed must be refused");
    assert!(matches!(err, ConfigEditError::NotATable(_)));
}
