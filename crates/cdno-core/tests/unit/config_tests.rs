use cdno_core::config::{StateOverflow, VaultConfig};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_config(dir: &Path, content: &str) {
    let config_dir = dir.join(".cuaderno");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.toml"), content).unwrap();
}

#[test]
fn load_returns_defaults_when_no_config_file() {
    let dir = TempDir::new().unwrap();
    let config = VaultConfig::load(dir.path()).unwrap();

    assert_eq!(config.vault.name, "My Vault");
    assert_eq!(config.vault.max_active_projects, 5);
    assert_eq!(config.vault.max_state_chars, 500);
    assert_eq!(config.vault.state_overflow, StateOverflow::Reject);
    assert!(config.schemas.is_empty());
    assert!(config.variables.static_vars.is_empty());
    assert!(config.variables.prompt.is_empty());
}

#[test]
fn load_parses_full_config() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[vault]
name = "Research Lab"
max_active_projects = 3

[schemas.project]
extra_required = ["collaborators", "funding_source"]

[schemas.evidence]
extra_required = []

[variables]
author = "A. Researcher"
institution = "University of Examples"
orcid = "0000-0000-0000-0000"

[variables.prompt]
collaborators = "Who are the collaborators?"
experiment_id = "Experiment identifier?"
"#,
    );

    let config = VaultConfig::load(dir.path()).unwrap();

    assert_eq!(config.vault.name, "Research Lab");
    assert_eq!(config.vault.max_active_projects, 3);

    let project_schema = config.schema_for("project").unwrap();
    assert_eq!(
        project_schema.extra_required,
        vec!["collaborators", "funding_source"]
    );

    let evidence_schema = config.schema_for("evidence").unwrap();
    assert!(evidence_schema.extra_required.is_empty());

    // Real TOML with no `[note_types]` deserialises to an empty map (back-compat).
    assert!(config.note_types.is_empty());
    assert!(config.validate_note_types().is_ok());

    assert_eq!(config.resolve_variable("author"), Some("A. Researcher"));
    assert_eq!(
        config.resolve_variable("institution"),
        Some("University of Examples")
    );
    assert_eq!(config.resolve_variable("nonexistent"), None);

    assert_eq!(
        config.prompt_for_variable("collaborators"),
        Some("Who are the collaborators?")
    );
    assert_eq!(config.prompt_for_variable("author"), None);
}

#[test]
fn load_parses_state_limit_keys() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[vault]
name = "Research Lab"
max_state_chars = 120
state_overflow = "warn"
"#,
    );

    let config = VaultConfig::load(dir.path()).unwrap();
    assert_eq!(config.vault.max_state_chars, 120);
    assert_eq!(config.vault.state_overflow, StateOverflow::Warn);
    // Unrelated `[vault]` keys still fall back to their defaults.
    assert_eq!(config.vault.max_active_projects, 5);
}

#[test]
fn state_limit_keys_default_when_absent_from_a_present_vault_table() {
    // A `[vault]` table that omits the new keys — the shape every
    // pre-upgrade vault has — must still get the shipped defaults.
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[vault]
name = "Legacy Vault"
max_active_projects = 4
"#,
    );

    let config = VaultConfig::load(dir.path()).unwrap();
    assert_eq!(config.vault.max_state_chars, 500);
    assert_eq!(config.vault.state_overflow, StateOverflow::Reject);
}

#[test]
fn load_rejects_unknown_state_overflow_value() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[vault]
state_overflow = "explode"
"#,
    );

    // An out-of-set enum value is a hard deserialize error, loud on an
    // older binary rather than silently misparsed.
    assert!(VaultConfig::load(dir.path()).is_err());
}

#[test]
fn load_parses_minimal_config() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[vault]
name = "Minimal"
"#,
    );

    let config = VaultConfig::load(dir.path()).unwrap();

    assert_eq!(config.vault.name, "Minimal");
    assert_eq!(config.vault.max_active_projects, 5);
    assert!(config.schemas.is_empty());
}

#[test]
fn load_returns_error_for_invalid_toml() {
    let dir = TempDir::new().unwrap();
    write_config(dir.path(), "this is not valid toml [[[");

    let result = VaultConfig::load(dir.path());
    assert!(result.is_err());
}

#[test]
fn extra_required_fields_returns_empty_for_unknown_type() {
    let config = VaultConfig::default();
    assert!(config.extra_required_fields("project").is_empty());
}

#[test]
fn absent_note_types_table_is_empty_and_valid() {
    // Back-compat: a vault with no `[note_types]` deserialises to an empty map
    // and passes structural validation, so nothing changes for existing vaults.
    let config = VaultConfig::default();
    assert!(config.note_types.is_empty());
    assert!(config.custom_type("person").is_none());
    assert!(config.validate_note_types().is_ok());
}

#[test]
fn parses_a_custom_note_type() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[note_types.person]
folder = "people"
required = ["name"]
optional = ["role", "org", "created"]
template = "person.md"
append_only = false
title_field = "name"
date_field = "created"
"#,
    );

    let config = VaultConfig::load(dir.path()).unwrap();
    let person = config.custom_type("person").expect("person type");
    assert_eq!(person.folder, "people");
    assert_eq!(person.required, vec!["name"]);
    assert_eq!(person.optional, vec!["role", "org", "created"]);
    assert_eq!(person.template.as_deref(), Some("person.md"));
    assert!(!person.append_only);
    assert_eq!(person.title_field.as_deref(), Some("name"));
    assert_eq!(person.date_field.as_deref(), Some("created"));
    assert!(config.validate_note_types().is_ok());
}

#[test]
fn custom_note_type_defaults_are_lenient() {
    // Only `folder` is required; everything else defaults.
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[note_types.book]
folder = "library"
"#,
    );

    let config = VaultConfig::load(dir.path()).unwrap();
    let book = config.custom_type("book").expect("book type");
    assert_eq!(book.folder, "library");
    assert!(book.required.is_empty());
    assert!(book.optional.is_empty());
    assert!(book.template.is_none());
    assert!(!book.append_only);
    assert!(book.title_field.is_none());
    assert!(book.date_field.is_none());
}

#[test]
fn validate_rejects_empty_folder() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[note_types.person]
folder = ""
"#,
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_note_types().is_err());
}

#[test]
fn validate_rejects_vault_escaping_folder() {
    for folder in ["/etc", "../escape", "a/../../b"] {
        let dir = TempDir::new().unwrap();
        write_config(
            dir.path(),
            &format!("[note_types.person]\nfolder = \"{folder}\"\n"),
        );
        let config = VaultConfig::load(dir.path()).unwrap();
        assert!(
            config.validate_note_types().is_err(),
            "folder `{folder}` should be rejected"
        );
    }
}

#[test]
fn validate_rejects_two_types_sharing_a_folder() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[note_types.person]
folder = "people"

[note_types.contact]
folder = "people"
"#,
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_note_types().is_err());
}

#[test]
fn validate_rejects_template_with_path_separator() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[note_types.person]
folder = "people"
template = "sub/person.md"
"#,
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_note_types().is_err());
}

#[test]
fn validate_rejects_folder_with_surrounding_whitespace() {
    let dir = TempDir::new().unwrap();
    write_config(dir.path(), "[note_types.person]\nfolder = \" people \"\n");
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_note_types().is_err());
}

#[test]
fn validate_rejects_backslash_folder() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        "[note_types.person]\nfolder = \"people\\\\..\\\\..\\\\etc\"\n",
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_note_types().is_err());
}

#[test]
fn validate_rejects_folder_colliding_with_a_builtin() {
    // A custom type may not claim a built-in top-level folder (it would drop
    // notes alongside built-in notes) — checked for the folder and a nested path.
    for folder in ["projects", "questions", "journal", "projects/vip"] {
        let dir = TempDir::new().unwrap();
        write_config(
            dir.path(),
            &format!("[note_types.custom]\nfolder = \"{folder}\"\n"),
        );
        let config = VaultConfig::load(dir.path()).unwrap();
        assert!(
            config.validate_note_types().is_err(),
            "folder `{folder}` should collide with a built-in"
        );
    }
}

#[test]
fn validate_rejects_title_or_date_field_not_declared() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        "[note_types.person]\nfolder = \"people\"\nrequired = [\"name\"]\ntitle_field = \"naem\"\n",
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_note_types().is_err());
}

#[test]
fn validate_accepts_title_and_date_fields_that_are_declared() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        "[note_types.person]\nfolder = \"people\"\nrequired = [\"name\"]\noptional = [\"met_on\"]\ntitle_field = \"name\"\ndate_field = \"met_on\"\n",
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_note_types().is_ok());
}

#[test]
fn validate_accepts_append_only_true() {
    // The flag is accepted; enforcement is deferred to a later phase.
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        "[note_types.log]\nfolder = \"logs\"\nappend_only = true\n",
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.custom_type("log").unwrap().append_only);
    assert!(config.validate_note_types().is_ok());
}

// ---------------------------------------------------------------------
// [schemas.<type>.fields] typed frontmatter fields (#301)
// ---------------------------------------------------------------------

use cdno_core::config::FieldType;

#[test]
fn parses_typed_schema_fields() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[schemas.daily.fields.meds]
type = "bool"
default = false

[schemas.daily.fields.mood]
type = "string"
values = ["low", "ok", "good"]
default = "ok"

[schemas.daily.fields.steps]
type = "int"
required = true
default = 0
"#,
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    let daily = config.schema_for("daily").unwrap();

    let meds = &daily.fields["meds"];
    assert_eq!(meds.ty, FieldType::Bool);
    assert_eq!(meds.default, Some(toml::Value::Boolean(false)));
    assert!(!meds.required);

    let mood = &daily.fields["mood"];
    assert_eq!(mood.ty, FieldType::String);
    assert_eq!(
        mood.values.as_deref(),
        Some(&["low".to_string(), "ok".to_string(), "good".to_string()][..])
    );

    let steps = &daily.fields["steps"];
    assert_eq!(steps.ty, FieldType::Int);
    assert!(steps.required);

    assert!(config.validate_schemas().is_ok());
}

#[test]
fn deny_unknown_fields_rejects_a_field_key_typo() {
    // A mistyped key (`defualt`) is a hard parse error, not a silent no-op.
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        "[schemas.daily.fields.meds]\ntype = \"bool\"\ndefualt = false\n",
    );
    assert!(VaultConfig::load(dir.path()).is_err());
}

#[test]
fn unknown_field_type_is_rejected_at_parse() {
    // A future `float`/`datetime` fails loudly on an older cdno rather than
    // being misparsed.
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        "[schemas.daily.fields.temp]\ntype = \"float\"\n",
    );
    assert!(VaultConfig::load(dir.path()).is_err());
}

#[test]
fn values_on_a_non_string_field_is_rejected() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        "[schemas.daily.fields.meds]\ntype = \"bool\"\nvalues = [\"a\", \"b\"]\n",
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_schemas().is_err());
}

#[test]
fn a_default_that_matches_its_type_passes() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[schemas.daily.fields.since]
type = "date"
default = "2026-01-01"
"#,
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_schemas().is_ok());
}

#[test]
fn a_default_that_mismatches_its_type_is_rejected() {
    // `int` field with a string default.
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        "[schemas.daily.fields.steps]\ntype = \"int\"\ndefault = \"lots\"\n",
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_schemas().is_err());
}

#[test]
fn a_default_outside_the_values_set_is_rejected() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[schemas.daily.fields.mood]
type = "string"
values = ["low", "ok", "good"]
default = "elated"
"#,
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_schemas().is_err());
}

#[test]
fn a_malformed_date_default_is_rejected() {
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        "[schemas.daily.fields.since]\ntype = \"date\"\ndefault = \"nope\"\n",
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_schemas().is_err());
}

#[test]
fn list_true_is_rejected_as_unimplemented() {
    // The list shape is reserved but not yet implemented in P1.
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        "[schemas.evidence.fields.tags]\ntype = \"string\"\nlist = true\n",
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    assert!(config.validate_schemas().is_err());
}

#[test]
fn reserved_setter_keys_parse_but_are_unused() {
    // `settable`/`log_on_change` are reserved for the Phase-2 setter — parsed
    // now (deny_unknown_fields would otherwise reject them) but inert in P1.
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[schemas.daily.fields.meds]
type = "bool"
settable = true
log_on_change = true
"#,
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    let meds = &config.schema_for("daily").unwrap().fields["meds"];
    assert_eq!(meds.settable, Some(true));
    assert_eq!(meds.log_on_change, Some(true));
    assert!(config.validate_schemas().is_ok());
}

#[test]
fn extra_required_still_parses_and_stays_lint_only() {
    // A bare `extra_required` desugars into the typed field view as an untyped,
    // NON-required string field — so folding it in never introduces
    // create-time-error semantics.
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        "[schemas.project]\nextra_required = [\"owner\"]\n",
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    let schema = config.schema_for("project").unwrap();
    // The legacy accessor is unchanged.
    assert_eq!(config.extra_required_fields("project"), &["owner"]);

    let declared = schema.declared_fields();
    let owner = &declared["owner"];
    assert_eq!(owner.ty, FieldType::String);
    assert!(
        !owner.required,
        "desugared extra_required must stay lint-only"
    );
    assert!(config.validate_schemas().is_ok());
}

#[test]
fn an_explicit_field_wins_over_a_colliding_extra_required() {
    // On a name collision the explicit typed block wins (real type, may set
    // required); the desugared extra_required entry is overwritten.
    let dir = TempDir::new().unwrap();
    write_config(
        dir.path(),
        r#"
[schemas.project]
extra_required = ["owner"]

[schemas.project.fields.owner]
type = "string"
required = true
"#,
    );
    let config = VaultConfig::load(dir.path()).unwrap();
    let declared = config.schema_for("project").unwrap().declared_fields();
    assert_eq!(declared.len(), 1);
    assert!(
        declared["owner"].required,
        "the explicit required=true block wins over the lint-only desugar"
    );
}
