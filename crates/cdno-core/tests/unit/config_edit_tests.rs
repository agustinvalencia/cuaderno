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
fn set_schema_field_writes_the_setter_flags_from_the_spec() {
    // The Config form edits `settable`/`log_on_change` (#375), so the writer
    // must EMIT them when the spec opts in. A mocked-IPC frontend test can't
    // prove this — only the real writer round-trip can, so the toggles are
    // genuinely persisted rather than silently dropped.
    let spec = FieldSpec {
        settable: Some(true),
        log_on_change: Some(true),
        ..field_spec(FieldType::String)
    };
    let out = set_schema_field("", "project", "stage", &spec).expect("write field");

    assert!(
        out.contains("settable = true"),
        "settable must be written: {out}"
    );
    assert!(
        out.contains("log_on_change = true"),
        "log_on_change must be written: {out}"
    );
}

#[test]
fn set_schema_field_clears_a_setter_flag_the_spec_no_longer_sets() {
    // Unchecking a toggle sends the flag as absent; the writer must REMOVE the
    // on-disk key (an absent and a `false` setter flag both mean "off"), or the
    // toggle would silently revert on the next re-parse. `field_spec` leaves
    // both flags `None`.
    let existing = "\
[schemas.project.fields.stage]
type = \"string\"
settable = true
log_on_change = true
";
    let out = set_schema_field(existing, "project", "stage", &field_spec(FieldType::String))
        .expect("edit stage");

    assert!(
        !out.contains("settable"),
        "cleared settable must be removed: {out}"
    );
    assert!(
        !out.contains("log_on_change"),
        "cleared log_on_change must be removed: {out}"
    );
}

#[test]
fn set_schema_field_removes_an_explicit_false_setter_flag() {
    // `Some(false)` is "off", the same as absent — the writer normalises it to
    // a removed key rather than writing `= false`. The form only ever sends
    // `true`/`None`, but a Raw-authored `settable = false` can reach the writer
    // via a lift, so the `Some(false)` arm must behave.
    let spec = FieldSpec {
        settable: Some(false),
        log_on_change: Some(false),
        ..field_spec(FieldType::String)
    };
    let existing = "\
[schemas.project.fields.stage]
type = \"string\"
settable = true
log_on_change = true
";
    let out = set_schema_field(existing, "project", "stage", &spec).expect("edit stage");

    assert!(
        !out.contains("settable"),
        "an explicit-false settable is removed, not written: {out}"
    );
    assert!(
        !out.contains("log_on_change"),
        "an explicit-false log_on_change is removed, not written: {out}"
    );
}

#[test]
fn set_schema_field_leaves_list_untouched_and_keeps_siblings() {
    // `list` stays reserved and unimplemented, so the writer never touches it:
    // a hand-authored `list` survives even when the spec doesn't carry it. The
    // sibling field is preserved too.
    let existing = "\
[schemas.project.fields.stage]
type = \"string\"
list = false

[schemas.project.fields.owner]
type = \"string\"
";
    let out = set_schema_field(existing, "project", "stage", &field_spec(FieldType::Date))
        .expect("edit stage");

    assert!(out.contains("type = \"date\""));
    assert!(
        out.contains("list = false"),
        "the unimplemented `list` key must survive untouched: {out}"
    );
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

#[test]
fn an_inline_table_note_type_is_refused_not_duplicated() {
    // A note type authored as an INLINE table is an `Item::Value`, not a
    // `[header]` table — editing it must refuse (NotATable) rather than
    // append a second `[note_types.person]` and silently split the type.
    let inline = "note_types.person = { folder = \"people\" }\n";
    let err = set_note_type(inline, "person", &note_type("folks"))
        .expect_err("an inline-table type must be refused, never duplicated");
    assert!(matches!(err, ConfigEditError::NotATable(_)));
}

#[test]
fn set_schema_field_edits_a_dotted_key_field_in_place() {
    // A field authored with dotted keys (no `[header]`) is edited in place:
    // the type flips and a hand-set reserved key on the same field survives,
    // with no duplicate table materialised. Uses `list` — the reserved key the
    // writer still leaves untouched (`settable`/`log_on_change` are now
    // form-controlled, #375).
    let dotted = "[schemas.project.fields]\nstage.type = \"string\"\nstage.list = false\n";
    let out = set_schema_field(dotted, "project", "stage", &field_spec(FieldType::Date))
        .expect("edit dotted-key field");

    assert!(out.contains("\"date\""), "the type flipped to date: {out}");
    assert!(!out.contains("\"string\""), "the old type is gone: {out}");
    assert!(
        out.contains("list = false"),
        "the hand-set reserved `list` key survives: {out}"
    );
    // No second field table was materialised — the whole candidate still
    // parses and carries exactly the one `stage` field.
    let doc: toml::Value = toml::from_str(&out).expect("candidate parses");
    let fields = doc["schemas"]["project"]["fields"]
        .as_table()
        .expect("fields table");
    assert_eq!(fields.len(), 1, "still exactly one field: {out}");
}

#[test]
fn set_schema_field_writes_a_date_default_as_a_quoted_string() {
    // The `Datetime` arm of `default_item` must render a QUOTED string, not
    // a bare TOML date — a bare date would re-parse as a `Datetime`, which
    // `validate_schemas` then rejects. Quoting keeps the candidate both
    // parseable and valid (dates are authored as quoted `YYYY-MM-DD`).
    let dt: toml::value::Datetime = "2026-07-09".parse().expect("parse date");
    let mut spec = field_spec(FieldType::Date);
    spec.default = Some(toml::Value::Datetime(dt));

    let out = set_schema_field("", "project", "due", &spec).expect("date default");
    assert!(
        out.contains("default = \"2026-07-09\""),
        "a date default is a quoted string, not a bare date: {out}"
    );
    // It round-trips as a string value, not a datetime.
    let doc: toml::Value = toml::from_str(&out).expect("candidate parses");
    let default = &doc["schemas"]["project"]["fields"]["due"]["default"];
    assert!(default.is_str(), "default is a string scalar: {default:?}");
}

#[test]
fn edits_a_table_between_two_siblings_leaving_both_intact() {
    // Editing the MIDDLE of three sibling note types must leave the one
    // before and the one after byte-identical — no reordering, no bleed.
    let three = "\
[note_types.alpha]
folder = \"alpha\"

[note_types.beta]
folder = \"beta\"

[note_types.gamma]
folder = \"gamma\"
";
    let out = set_note_type(three, "beta", &note_type("beta-renamed")).expect("edit middle");

    assert!(out.contains("folder = \"beta-renamed\""));
    // Both neighbours survive verbatim, and alpha still precedes gamma.
    assert!(out.contains("[note_types.alpha]\nfolder = \"alpha\""));
    assert!(out.contains("[note_types.gamma]\nfolder = \"gamma\""));
    let alpha_at = out.find("alpha").expect("alpha");
    let gamma_at = out.find("gamma").expect("gamma");
    assert!(alpha_at < gamma_at, "sibling order preserved");
}

#[test]
fn a_variables_block_survives_a_note_type_edit() {
    // The `[variables]` block (which the form never edits) must be preserved
    // byte-for-byte across a note-type edit — it is Raw-only, so a form save
    // must not disturb it.
    let with_vars = "\
[variables]
author = \"A. Writer\"

[variables.prompt]
collaborators = \"Who worked on this?\"

[note_types.person]
folder = \"people\"
";
    let out = set_note_type(with_vars, "person", &note_type("folks")).expect("edit");

    assert!(out.contains("folder = \"folks\""));
    // Every variables line is intact.
    assert!(out.contains("[variables]\nauthor = \"A. Writer\""));
    assert!(out.contains("[variables.prompt]\ncollaborators = \"Who worked on this?\""));
}
