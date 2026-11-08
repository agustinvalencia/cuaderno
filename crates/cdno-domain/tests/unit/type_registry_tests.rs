//! Tests for the note-type registry: resolution of built-in vs config-defined
//! custom types, the reserved-name guard, and the config-derived frontmatter
//! order.

use cdno_core::config::{CustomNoteType, FieldSpec, FieldType, SchemaExtension, VaultConfig};
use cdno_domain::note_type::NoteType;
use cdno_domain::{NoteTypeDescriptor, NoteTypeKind, TYPST_REFUSED_TYPES, TypeRegistry};

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

/// An `int` field with a declared default, for the describe-payload tests.
fn int_field(default: i64) -> FieldSpec {
    FieldSpec {
        ty: FieldType::Int,
        default: Some(toml::Value::Integer(default)),
        required: true,
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
    let mut config = config_with_person();
    assert!(TypeRegistry::validate(&mut config).is_ok());
}

#[test]
fn validate_rejects_a_type_shadowing_a_builtin() {
    // A custom type may not reuse a built-in name.
    let mut config = VaultConfig::default();
    config
        .note_types
        .insert("project".to_owned(), custom("my-projects", &[], &[]));
    let err = TypeRegistry::validate(&mut config).expect_err("should reject reserved name");
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
            TypeRegistry::validate(&mut config).is_err(),
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
    assert!(TypeRegistry::validate(&mut config).is_err());
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
    let mut config = config_with_schema_field("project", "type");
    let err = TypeRegistry::validate(&mut config).expect_err("should reject `type`");
    assert!(format!("{err}").contains("type"), "{err}");
}

#[test]
fn validate_rejects_a_schema_field_named_after_the_date_period_key() {
    // The calendar types' identity key is engine-owned: daily→date,
    // weekly→week, monthly→month. Each is derived from `frontmatter_order`, so
    // the block is hard, not a hardcoded name list.
    for (note_type, key) in [("daily", "date"), ("weekly", "week"), ("monthly", "month")] {
        let mut config = config_with_schema_field(note_type, key);
        assert!(
            TypeRegistry::validate(&mut config).is_err(),
            "`{key}` must be reserved on `{note_type}`"
        );
    }
}

#[test]
fn validate_allows_a_schema_field_colliding_with_a_non_identity_supplied_key() {
    // A field colliding with a non-identity supplied placeholder (daily
    // supplies `weekday`) only WARNS — it must not block vault-open.
    let mut config = config_with_schema_field("daily", "weekday");
    assert!(
        TypeRegistry::validate(&mut config).is_ok(),
        "a non-identity supplied-key collision warns, never blocks"
    );
}

#[test]
fn validate_allows_a_novel_schema_field_on_a_builtin() {
    // The common case: a genuinely new field (daily `meds`) is fine.
    let mut config = config_with_schema_field("daily", "meds");
    assert!(TypeRegistry::validate(&mut config).is_ok());
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
    assert!(TypeRegistry::validate(&mut config).is_err());
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
        vec!["title", "slug", "created", "date", "body", "name", "role"]
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
    assert_eq!(
        ph,
        vec!["title", "slug", "created", "date", "body", "venue"]
    );
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
        vec!["title", "slug", "created", "date", "body"]
    );
}

#[test]
fn describe_all_lists_builtins_then_custom_with_schema_overlay() {
    // A custom `person` type plus a typed field overlaid onto the built-in
    // `project` via `[schemas.project.fields.priority]`.
    let mut config = config_with_person();
    let mut schema = SchemaExtension::default();
    schema.fields.insert("priority".to_owned(), int_field(1));
    config.schemas.insert("project".to_owned(), schema);
    let reg = TypeRegistry::new(&config);

    let all = reg.describe_all();
    // The 12 built-ins (in `NoteType::ALL` order) then the one custom type.
    assert_eq!(all.len(), NoteType::ALL.len() + 1);
    for (info, nt) in all.iter().zip(NoteType::ALL.iter()) {
        assert_eq!(info.name, nt.as_str());
        assert_eq!(info.kind, NoteTypeKind::Builtin);
    }

    let person = all.last().unwrap();
    assert_eq!(person.name, "person");
    assert_eq!(person.kind, NoteTypeKind::Custom);
    assert_eq!(person.folder.as_deref(), Some("people"));
    assert_eq!(person.required, vec!["name".to_owned()]);
    assert_eq!(person.optional, vec!["role".to_owned()]);
    assert!(person.supplied_placeholders.contains(&"name".to_owned()));
    assert!(person.fields.is_empty());

    // The typed field overlays onto the built-in `project`.
    let project = all.iter().find(|i| i.name == "project").unwrap();
    assert_eq!(project.kind, NoteTypeKind::Builtin);
    let priority = project
        .fields
        .iter()
        .find(|f| f.name == "priority")
        .expect("the overlaid typed field is present on the built-in");
    assert_eq!(priority.ty, FieldType::Int);
    assert!(priority.required);
    // The toml default is rendered to its scalar string, never a `toml::Value`.
    assert_eq!(priority.default.as_deref(), Some("1"));
}

#[test]
fn describe_returns_none_for_an_unknown_type() {
    let config = VaultConfig::default();
    assert!(TypeRegistry::new(&config).describe("not-a-type").is_none());
}

#[test]
fn describe_reports_append_only_for_the_append_only_builtins() {
    // The business rule: daily, weekly, evidence, tracking are append-only.
    let config = VaultConfig::default();
    let reg = TypeRegistry::new(&config);
    for name in ["daily", "weekly", "evidence", "tracking"] {
        assert!(
            reg.describe(name).unwrap().append_only,
            "{name} is append-only"
        );
    }
    for name in ["project", "action", "question", "portfolio"] {
        assert!(
            !reg.describe(name).unwrap().append_only,
            "{name} is not append-only"
        );
    }
}

#[test]
fn describe_renders_a_bool_default_and_preserves_allowed_values() {
    let mut config = VaultConfig::default();
    let mut schema = SchemaExtension::default();
    schema.fields.insert(
        "shipped".to_owned(),
        FieldSpec {
            ty: FieldType::Bool,
            default: Some(toml::Value::Boolean(false)),
            required: false,
            values: None,
            list: None,
            settable: None,
            log_on_change: None,
        },
    );
    schema.fields.insert(
        "stage".to_owned(),
        FieldSpec {
            ty: FieldType::String,
            default: None,
            required: false,
            values: Some(vec!["draft".to_owned(), "final".to_owned()]),
            list: None,
            settable: None,
            log_on_change: None,
        },
    );
    config.schemas.insert("project".to_owned(), schema);
    let reg = TypeRegistry::new(&config);
    let project = reg.describe("project").unwrap();

    let shipped = project.fields.iter().find(|f| f.name == "shipped").unwrap();
    assert_eq!(shipped.ty, FieldType::Bool);
    assert_eq!(shipped.default.as_deref(), Some("false"));

    let stage = project.fields.iter().find(|f| f.name == "stage").unwrap();
    assert_eq!(
        stage.values,
        Some(vec!["draft".to_owned(), "final".to_owned()])
    );
}

/// Captures every `tracing` event as its message plus `name=value` fields.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<(tracing::Level, String)>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Captured {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        struct Text(String);
        impl tracing::field::Visit for Text {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.push_str(&format!("{}={value:?} ", field.name()));
            }
        }
        let mut text = Text(String::new());
        event.record(&mut text);
        self.0
            .lock()
            .unwrap()
            .push((*event.metadata().level(), text.0));
    }
}

/// Validate a config declaring `commitment.due` as `spec`, returning the
/// WARN events logged. Thread-local subscriber, so parallel tests don't mix.
fn shadow_warnings(spec: FieldSpec) -> Vec<String> {
    use tracing_subscriber::layer::SubscriberExt;
    let mut config = VaultConfig::default();
    config
        .schemas
        .entry("commitment".to_owned())
        .or_default()
        .fields
        .insert("due".to_owned(), spec);
    let captured = Captured::default();
    let subscriber = tracing_subscriber::registry().with(captured.clone());
    tracing::subscriber::with_default(subscriber, || {
        TypeRegistry::validate(&mut config).expect("config is valid");
    });
    let events = captured.0.lock().unwrap();
    events
        .iter()
        .filter(|(l, _)| *l == tracing::Level::WARN)
        .map(|(_, m)| m.clone())
        .collect()
}

#[test]
fn a_placeholder_field_declaring_only_type_and_settable_does_not_warn() {
    let spec = FieldSpec {
        ty: FieldType::Date,
        settable: Some(true),
        ..string_field()
    };
    assert!(shadow_warnings(spec).is_empty());
}

#[test]
fn a_placeholder_field_declaring_only_required_or_values_does_not_warn() {
    let required = FieldSpec {
        required: true,
        ..string_field()
    };
    let values = FieldSpec {
        values: Some(vec!["a".into()]),
        ..string_field()
    };
    assert!(shadow_warnings(required).is_empty());
    assert!(shadow_warnings(values).is_empty());
}

#[test]
fn a_placeholder_field_declaring_a_default_warns_once() {
    let spec = FieldSpec {
        ty: FieldType::Date,
        default: Some(toml::Value::String("2026-01-01".into())),
        settable: Some(true),
        ..string_field()
    };
    let warnings = shadow_warnings(spec);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("note_type=commitment"), "{warnings:?}");
    assert!(warnings[0].contains("field=due"), "{warnings:?}");
}

fn with_typst(mut config: VaultConfig, names: Option<&[&str]>) -> VaultConfig {
    config.typst.note_types = names.map(|n| n.iter().map(|s| (*s).to_owned()).collect());
    config
}

fn config_with_concept() -> VaultConfig {
    let mut config = VaultConfig::default();
    config
        .note_types
        .insert("concept".to_owned(), custom("concepts", &[], &[]));
    config
}

#[test]
fn typst_default_is_evidence_plus_concept_when_declared() {
    let plain = VaultConfig::default();
    assert_eq!(TypeRegistry::new(&plain).typst_types(), vec!["evidence"]);

    let with_concept = config_with_concept();
    let registry = TypeRegistry::new(&with_concept);
    assert_eq!(registry.typst_types(), vec!["evidence", "concept"]);
    assert!(registry.admits_typst("evidence"));
    assert!(registry.admits_typst("concept"));
    assert!(!registry.admits_typst("project"));
}

#[test]
fn a_vault_without_concept_opens_with_no_typst_table() {
    let mut config = VaultConfig::default();
    assert!(config.typst.note_types.is_none());
    assert!(TypeRegistry::validate(&mut config).is_ok());
}

#[test]
fn an_empty_list_turns_typst_off() {
    let mut config = with_typst(config_with_concept(), Some(&[]));
    assert!(TypeRegistry::validate(&mut config).is_ok());
    let registry = TypeRegistry::new(&config);
    assert!(registry.typst_types().is_empty());
    assert!(!registry.admits_typst("evidence"));
    assert!(!registry.admits_typst("concept"));
    assert!(registry.typst_folders().is_empty());
}

#[test]
fn the_refused_set_is_every_builtin_but_evidence() {
    let mut refused: Vec<&str> = TYPST_REFUSED_TYPES.to_vec();
    refused.sort_unstable();
    let mut expected: Vec<&str> = NoteType::ALL
        .iter()
        .map(|t| t.as_str())
        .filter(|n| *n != "evidence")
        .collect();
    expected.sort_unstable();
    assert_eq!(refused, expected);
}

#[test]
fn an_empty_name_is_unknown() {
    let mut config = with_typst(VaultConfig::default(), Some(&[""]));
    let err = TypeRegistry::validate(&mut config).expect_err("empty name");
    assert!(
        err.to_string().contains("is not a known note type"),
        "{err}"
    );
}

#[test]
fn an_explicit_list_naming_a_refused_type_fails_validation() {
    for refused in TYPST_REFUSED_TYPES {
        let mut config = with_typst(VaultConfig::default(), Some(&["evidence", refused]));
        let err = TypeRegistry::validate(&mut config)
            .expect_err(&format!("`{refused}` should be refused"));
        let msg = err.to_string();
        assert!(msg.contains(&format!("`{refused}`")), "{msg}");
        assert!(msg.contains("evidence and custom types only"), "{msg}");
    }
}

#[test]
fn an_explicit_list_naming_an_unknown_type_fails_validation() {
    let mut config = with_typst(VaultConfig::default(), Some(&["concept"]));
    let err = TypeRegistry::validate(&mut config).expect_err("unknown type");
    assert!(
        err.to_string().contains("`concept` is not a known"),
        "{err}"
    );
}

#[test]
fn an_explicit_list_naming_known_types_validates() {
    let mut config = with_typst(config_with_person(), Some(&["evidence", "person"]));
    assert!(TypeRegistry::validate(&mut config).is_ok());
    assert!(TypeRegistry::new(&config).admits_typst("person"));
}

#[test]
fn typst_folders_follow_the_types() {
    let config = config_with_concept();
    assert_eq!(
        TypeRegistry::new(&config).typst_folders(),
        vec!["concepts".to_owned(), "portfolios".to_owned()]
    );
    let only_concept = with_typst(config_with_concept(), Some(&["concept"]));
    assert_eq!(
        TypeRegistry::new(&only_concept).typst_folders(),
        vec!["concepts".to_owned()]
    );
}

#[test]
fn describe_reports_admits_typst() {
    let config = config_with_concept();
    let registry = TypeRegistry::new(&config);
    assert!(registry.describe("evidence").unwrap().admits_typst);
    assert!(registry.describe("concept").unwrap().admits_typst);
    assert!(!registry.describe("project").unwrap().admits_typst);
}
