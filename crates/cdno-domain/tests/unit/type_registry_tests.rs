//! Tests for the note-type registry: resolution of built-in vs config-defined
//! custom types, the reserved-name guard, and the config-derived frontmatter
//! order.

use cdno_core::config::{CustomNoteType, FieldSpec, FieldType, SchemaExtension, VaultConfig};
use cdno_domain::note_type::NoteType;
use cdno_domain::{NoteTypeDescriptor, TypeRegistry};

/// A minimal `string` field spec (no default/values) for the reserved-field
/// tests — enough to place a declared field under a built-in type's schema.
fn string_field() -> FieldSpec {
    FieldSpec {
        ty: FieldType::String,
        default: None,
        required: false,
        values: None,
        list: None,
        settable: None,
        log_on_change: None,
    }
}

/// A config with a single `[schemas.<type>.fields.<field>]` string field.
fn config_with_schema_field(note_type: &str, field: &str) -> VaultConfig {
    let mut config = VaultConfig::default();
    let mut schema = SchemaExtension::default();
    schema.fields.insert(field.to_owned(), string_field());
    config.schemas.insert(note_type.to_owned(), schema);
    config
}

fn custom(folder: &str, required: &[&str], optional: &[&str]) -> CustomNoteType {
    CustomNoteType {
        folder: folder.to_owned(),
        required: required.iter().map(|s| (*s).to_owned()).collect(),
        optional: optional.iter().map(|s| (*s).to_owned()).collect(),
        template: None,
        append_only: false,
        title_field: None,
        date_field: None,
    }
}

fn config_with_person() -> VaultConfig {
    let mut config = VaultConfig::default();
    config
        .note_types
        .insert("person".to_owned(), custom("people", &["name"], &["role"]));
    config
}

#[test]
fn resolves_builtin_types() {
    let config = VaultConfig::default();
    let reg = TypeRegistry::new(&config);
    for nt in NoteType::ALL {
        match reg.resolve(nt.as_str()) {
            Some(NoteTypeDescriptor::Builtin(got)) => assert_eq!(got, nt),
            other => panic!("expected Builtin({nt:?}), got {other:?}"),
        }
    }
}

#[test]
fn resolves_a_custom_type() {
    let config = config_with_person();
    let reg = TypeRegistry::new(&config);
    match reg.resolve("person") {
        Some(NoteTypeDescriptor::Custom { name, def }) => {
            assert_eq!(name, "person");
            assert_eq!(def.folder, "people");
        }
        other => panic!("expected Custom, got {other:?}"),
    }
    assert!(reg.is_known("person"));
}

#[test]
fn unknown_type_resolves_to_none() {
    let config = config_with_person();
    let reg = TypeRegistry::new(&config);
    assert!(reg.resolve("persn").is_none());
    assert!(!reg.is_known("persn"));
}

#[test]
fn all_names_lists_builtins_then_custom() {
    let config = config_with_person();
    let reg = TypeRegistry::new(&config);
    let names = reg.all_names();
    for nt in NoteType::ALL {
        assert!(names.contains(&nt.as_str()), "missing built-in {nt:?}");
    }
    assert!(names.contains(&"person"));
    assert_eq!(names.len(), NoteType::ALL.len() + 1);
}

#[test]
fn validate_accepts_a_sound_custom_type() {
    let config = config_with_person();
    assert!(TypeRegistry::validate(&config).is_ok());
}

#[test]
fn validate_rejects_a_type_shadowing_a_builtin() {
    // A custom type may not reuse a built-in name.
    let mut config = VaultConfig::default();
    config
        .note_types
        .insert("project".to_owned(), custom("my-projects", &[], &[]));
    let err = TypeRegistry::validate(&config).expect_err("should reject reserved name");
    assert!(
        format!("{err}").contains("project"),
        "error should name the offending type: {err}"
    );
}

#[test]
fn every_builtin_name_is_reserved() {
    // Drift guard: the reserved set is exactly `NoteType::ALL` (via `from_str`),
    // so a newly added built-in is automatically un-shadowable — no separate
    // list to keep in sync.
    for nt in NoteType::ALL {
        let mut config = VaultConfig::default();
        config
            .note_types
            .insert(nt.as_str().to_owned(), custom("some-folder", &[], &[]));
        assert!(
            TypeRegistry::validate(&config).is_err(),
            "built-in `{}` must be reserved against custom redefinition",
            nt.as_str()
        );
    }
}

#[test]
fn validate_propagates_structural_errors() {
    // Empty folder is a core structural error surfaced through the registry.
    let mut config = VaultConfig::default();
    config
        .note_types
        .insert("person".to_owned(), custom("", &[], &[]));
    assert!(TypeRegistry::validate(&config).is_err());
}

#[test]
fn custom_frontmatter_order_is_type_then_required_then_optional() {
    let config = config_with_person();
    let reg = TypeRegistry::new(&config);
    let desc = reg.resolve("person").unwrap();
    assert_eq!(
        desc.custom_frontmatter_order().unwrap(),
        vec!["type", "name", "role"]
    );
}

#[test]
fn custom_frontmatter_order_dedupes_and_preserves_order() {
    let mut config = VaultConfig::default();
    // A field listed in both required and optional appears once, in required's
    // position; `type` declared explicitly isn't duplicated.
    config.note_types.insert(
        "person".to_owned(),
        custom("people", &["name", "org"], &["org", "role"]),
    );
    let reg = TypeRegistry::new(&config);
    let desc = reg.resolve("person").unwrap();
    assert_eq!(
        desc.custom_frontmatter_order().unwrap(),
        vec!["type", "name", "org", "role"]
    );
}

#[test]
fn builtin_descriptor_has_no_custom_order() {
    let config = VaultConfig::default();
    let reg = TypeRegistry::new(&config);
    let desc = reg.resolve("project").unwrap();
    assert!(desc.custom_frontmatter_order().is_none());
    assert!(!desc.is_custom());
}

#[test]
fn required_fields_reads_custom_declaration_and_builtin_schema() {
    // Custom type → its declared `required`.
    let config = config_with_person();
    let reg = TypeRegistry::new(&config);
    let person = reg.resolve("person").unwrap();
    assert_eq!(person.required_fields(&config), &["name".to_owned()]);

    // Built-in type → the vault's `[schemas.<type>].extra_required` (empty here).
    let project = reg.resolve("project").unwrap();
    assert!(project.required_fields(&config).is_empty());
}

#[test]
fn validate_rejects_a_schema_field_named_type() {
    // `type` is engine-written for every note — a declared field may not shadow
    // it, on any built-in type.
    let config = config_with_schema_field("project", "type");
    let err = TypeRegistry::validate(&config).expect_err("should reject `type`");
    assert!(format!("{err}").contains("type"), "{err}");
}

#[test]
fn validate_rejects_a_schema_field_named_after_the_date_period_key() {
    // The calendar types' identity key is engine-owned: daily→date,
    // weekly→week, monthly→month. Each is derived from `frontmatter_order`, so
    // the block is hard, not a hardcoded name list.
    for (note_type, key) in [("daily", "date"), ("weekly", "week"), ("monthly", "month")] {
        let config = config_with_schema_field(note_type, key);
        assert!(
            TypeRegistry::validate(&config).is_err(),
            "`{key}` must be reserved on `{note_type}`"
        );
    }
}

#[test]
fn validate_allows_a_schema_field_colliding_with_a_non_identity_supplied_key() {
    // A field colliding with a non-identity supplied placeholder (daily
    // supplies `weekday`) only WARNS — it must not block vault-open.
    let config = config_with_schema_field("daily", "weekday");
    assert!(
        TypeRegistry::validate(&config).is_ok(),
        "a non-identity supplied-key collision warns, never blocks"
    );
}

#[test]
fn validate_allows_a_novel_schema_field_on_a_builtin() {
    // The common case: a genuinely new field (daily `meds`) is fine.
    let config = config_with_schema_field("daily", "meds");
    assert!(TypeRegistry::validate(&config).is_ok());
}

#[test]
fn validate_rejects_a_case_variant_of_a_builtin() {
    // `[note_types.Project]` must be rejected: `from_str` is exact-match, so
    // without a case-insensitive guard it would resolve as a distinct type from
    // the lowercase `project` every tool writes.
    let mut config = VaultConfig::default();
    config
        .note_types
        .insert("Project".to_owned(), custom("my-projects", &[], &[]));
    assert!(TypeRegistry::validate(&config).is_err());
}

#[test]
fn all_names_orders_custom_types_deterministically() {
    // Custom names are sorted so the completion list is stable regardless of
    // the config map's iteration order.
    let mut config = VaultConfig::default();
    config
        .note_types
        .insert("zebra".to_owned(), custom("zebras", &[], &[]));
    config
        .note_types
        .insert("apple".to_owned(), custom("apples", &[], &[]));
    let reg = TypeRegistry::new(&config);
    let names = reg.all_names();
    let apple = names.iter().position(|n| *n == "apple").unwrap();
    let zebra = names.iter().position(|n| *n == "zebra").unwrap();
    assert!(apple < zebra, "custom names should be sorted: {names:?}");
    // Custom names come after every built-in.
    let last_builtin = names.iter().position(|n| *n == "inbox").unwrap();
    assert!(apple > last_builtin);
}

#[test]
fn supplied_placeholders_for_custom_is_builtins_plus_declared() {
    let config = config_with_person();
    let reg = TypeRegistry::new(&config);
    let desc = reg.resolve("person").unwrap();
    assert_eq!(
        desc.supplied_placeholders(),
        vec!["title", "slug", "created", "date", "name", "role"]
    );
}

#[test]
fn supplied_placeholders_for_builtin_matches_the_registry_list() {
    let config = VaultConfig::default();
    let reg = TypeRegistry::new(&config);
    let desc = reg.resolve("project").unwrap();
    let via_descriptor = desc.supplied_placeholders();
    let direct: Vec<String> = NoteType::Project
        .supplied_placeholders()
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(via_descriptor, direct);
}

#[test]
fn supplied_placeholders_dedupes_a_field_colliding_with_a_builtin() {
    // A custom type declaring `created`/`date` (which the create path already
    // supplies) must not double-list them.
    let mut config = VaultConfig::default();
    config.note_types.insert(
        "event".to_owned(),
        custom("events", &["created"], &["date", "venue"]),
    );
    let reg = TypeRegistry::new(&config);
    let ph = reg.resolve("event").unwrap().supplied_placeholders();
    assert_eq!(ph, vec!["title", "slug", "created", "date", "venue"]);
}

#[test]
fn supplied_placeholders_for_a_fieldless_custom_type_is_just_the_builtins() {
    let mut config = VaultConfig::default();
    config
        .note_types
        .insert("bookmark".to_owned(), custom("bookmarks", &[], &[]));
    let reg = TypeRegistry::new(&config);
    assert_eq!(
        reg.resolve("bookmark").unwrap().supplied_placeholders(),
        vec!["title", "slug", "created", "date"]
    );
}
