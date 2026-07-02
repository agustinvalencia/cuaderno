//! Tests for the generic create/list path for config-defined custom note
//! types (`Vault::create_custom_note*`, `list_custom_notes`).

use std::collections::HashMap;
use std::sync::Arc;

use cdno_core::config::{CustomNoteType, VaultConfig};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use chrono::{NaiveDate, NaiveDateTime};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn at() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 4, 26)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap()
}

/// A `person` custom type: folder `people`, required `name`, optional `role`.
fn person() -> CustomNoteType {
    CustomNoteType {
        folder: "people".to_owned(),
        required: vec!["name".to_owned()],
        optional: vec!["role".to_owned()],
        template: None,
        append_only: false,
        title_field: None,
        date_field: None,
    }
}

fn vault_with(config: VaultConfig, seed: &[(&str, &str)]) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in seed {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _r) = Vault::new(Arc::clone(&store), index, config).expect("Vault::new");
    (vault, store)
}

fn config_with_person() -> VaultConfig {
    let mut config = VaultConfig::default();
    config.note_types.insert("person".to_owned(), person());
    config
}

fn fields(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn creates_a_note_without_a_template_by_synthesising_one() {
    let (vault, store) = vault_with(config_with_person(), &[]);
    let path = vault
        .create_custom_note(
            at(),
            "person",
            "Ada Lovelace",
            &fields(&[("name", "Ada"), ("role", "advisor")]),
        )
        .expect("create");
    assert_eq!(path, vp("people/ada-lovelace.md"));

    let content = store.read_file(&path).unwrap();
    assert!(content.contains("type: person"), "{content}");
    assert!(content.contains("name: Ada"), "{content}");
    assert!(content.contains("role: advisor"), "{content}");
    assert!(content.contains("# Ada Lovelace"), "{content}");
    // The synthesised note must lint clean (required field present, known type).
    let report = vault.lint_all_notes().unwrap();
    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn omits_an_unset_optional_field_in_the_synthesised_note() {
    let (vault, store) = vault_with(config_with_person(), &[]);
    let path = vault
        .create_custom_note(at(), "person", "Ada", &fields(&[("name", "Ada")]))
        .expect("create");
    let content = store.read_file(&path).unwrap();
    assert!(content.contains("name: Ada"));
    assert!(
        !content.contains("role:"),
        "unset optional should be absent:\n{content}"
    );
}

#[test]
fn renders_a_custom_template_when_present() {
    let template = "---\ntype: person\nname: {{name}}\nrole: {{role}}\n---\n# {{title}}\n\nMet via {{role}}.\n";
    let (vault, store) = vault_with(
        config_with_person(),
        &[(".cuaderno/templates/person.md", template)],
    );
    let path = vault
        .create_custom_note(
            at(),
            "person",
            "Ada",
            &fields(&[("name", "Ada"), ("role", "mentor")]),
        )
        .expect("create");
    let content = store.read_file(&path).unwrap();
    assert!(
        content.contains("Met via mentor."),
        "template body rendered:\n{content}"
    );
    assert!(content.contains("name: Ada"), "{content}");
}

#[test]
fn rejects_a_missing_required_field() {
    let (vault, _store) = vault_with(config_with_person(), &[]);
    let err = vault
        .create_custom_note(at(), "person", "Nameless", &fields(&[("role", "x")]))
        .expect_err("should reject");
    assert!(matches!(err, DomainError::MissingRequiredField { field, .. } if field == "name"));
}

#[test]
fn rejects_an_undeclared_field() {
    let (vault, _store) = vault_with(config_with_person(), &[]);
    let err = vault
        .create_custom_note(
            at(),
            "person",
            "Ada",
            &fields(&[("name", "Ada"), ("hobby", "chess")]),
        )
        .expect_err("should reject");
    assert!(matches!(err, DomainError::UnknownField { field, .. } if field == "hobby"));
}

#[test]
fn rejects_an_empty_title() {
    let (vault, _store) = vault_with(config_with_person(), &[]);
    let err = vault
        .create_custom_note(at(), "person", "   ", &fields(&[("name", "Ada")]))
        .expect_err("should reject");
    assert!(matches!(err, DomainError::EmptyField { field } if field == "title"));
}

#[test]
fn rejects_a_duplicate_slug() {
    let (vault, _store) = vault_with(config_with_person(), &[]);
    vault
        .create_custom_note(at(), "person", "Ada", &fields(&[("name", "Ada")]))
        .expect("first");
    let err = vault
        .create_custom_note(at(), "person", "Ada", &fields(&[("name", "Ada II")]))
        .expect_err("duplicate slug");
    assert!(matches!(err, DomainError::Store(_)));
}

#[test]
fn refuses_a_builtin_type() {
    // The generic path is for custom types only; a built-in gets a distinct,
    // steering error (not "unknown note type").
    let (vault, _store) = vault_with(config_with_person(), &[]);
    let err = vault
        .create_custom_note(at(), "project", "My Project", &fields(&[]))
        .expect_err("should refuse built-in");
    assert!(
        matches!(&err, DomainError::BuiltinTypeNotCustom { note_type } if note_type == "project"),
        "expected BuiltinTypeNotCustom, got {err:?}"
    );
}

#[test]
fn refuses_an_unregistered_type() {
    let (vault, _store) = vault_with(config_with_person(), &[]);
    let err = vault
        .create_custom_note(at(), "gadget", "Widget", &fields(&[]))
        .expect_err("should refuse unknown");
    assert!(matches!(err, DomainError::UnknownNoteType { .. }));
}

#[test]
fn lists_custom_notes_by_path() {
    let (vault, _store) = vault_with(config_with_person(), &[]);
    vault
        .create_custom_note(at(), "person", "Ada", &fields(&[("name", "Ada")]))
        .unwrap();
    vault
        .create_custom_note(at(), "person", "Grace", &fields(&[("name", "Grace")]))
        .unwrap();

    let paths: Vec<String> = vault
        .list_custom_notes("person")
        .unwrap()
        .iter()
        .map(|p| p.to_string())
        .collect();
    assert_eq!(paths, vec!["people/ada.md", "people/grace.md"]);
}

#[test]
fn synthesised_frontmatter_is_yaml_safe() {
    // Field values with YAML metacharacters must round-trip as strings — no
    // parse crash, no type coercion, no `---` document injection.
    let (vault, store) = vault_with(config_with_person(), &[]);
    let tricky = "Head: Research #1\n---\ninjected: pwned";
    let path = vault
        .create_custom_note(
            at(),
            "person",
            "Edge",
            &fields(&[("name", "Ada"), ("role", tricky)]),
        )
        .expect("create with a tricky value");

    // The note reconciles and lints clean (required `name` genuinely present).
    let report = vault.lint_all_notes().unwrap();
    assert!(report.is_clean(), "issues: {:?}", report.issues);

    // The value survives verbatim as a string — no injected `injected:` key.
    let content = store.read_file(&path).unwrap();
    let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&content).unwrap();
    assert_eq!(
        fm.optional_field::<String>("role").unwrap().as_deref(),
        Some(tricky)
    );
    assert!(
        !content.contains("injected: pwned\n---\n---"),
        "no document injection:\n{content}"
    );
}

#[test]
fn bool_and_number_valued_fields_stay_strings() {
    let (vault, store) = vault_with(config_with_person(), &[]);
    let path = vault
        .create_custom_note(
            at(),
            "person",
            "Truthy",
            &fields(&[("name", "true"), ("role", "42")]),
        )
        .expect("create");
    let content = store.read_file(&path).unwrap();
    let (fm, _b) = cdno_core::frontmatter::Frontmatter::parse(&content).unwrap();
    assert_eq!(
        fm.optional_field::<String>("name").unwrap().as_deref(),
        Some("true")
    );
    assert_eq!(
        fm.optional_field::<String>("role").unwrap().as_deref(),
        Some("42")
    );
}

#[test]
fn renders_prompted_vars_from_a_custom_template() {
    // The `_with_vars` path: a custom template's `[variables.prompt]` value is
    // supplied and rendered.
    let template =
        "---\ntype: person\nname: {{name}}\n---\n# {{title}}\n\nGreeting: {{greeting}}\n";
    let mut config = config_with_person();
    config
        .variables
        .prompt
        .insert("greeting".to_owned(), "Say hi?".to_owned());
    let (vault, store) = vault_with(config, &[(".cuaderno/templates/person.md", template)]);

    let prompted: HashMap<String, String> = [("greeting".to_owned(), "hello there".to_owned())]
        .into_iter()
        .collect();
    let path = vault
        .create_custom_note_with_vars(
            at(),
            "person",
            "Ada",
            &fields(&[("name", "Ada")]),
            &prompted,
        )
        .expect("create with vars");
    assert!(
        store
            .read_file(&path)
            .unwrap()
            .contains("Greeting: hello there")
    );
}

#[test]
fn errors_on_an_unresolved_prompted_var() {
    // A custom template referencing a prompt var with no value supplied errors.
    let template = "---\ntype: person\nname: {{name}}\n---\n# {{title}}\n\n{{greeting}}\n";
    let mut config = config_with_person();
    config
        .variables
        .prompt
        .insert("greeting".to_owned(), "Say hi?".to_owned());
    let (vault, _store) = vault_with(config, &[(".cuaderno/templates/person.md", template)]);

    let err = vault
        .create_custom_note(at(), "person", "Ada", &fields(&[("name", "Ada")]))
        .expect_err("should error on unresolved prompt");
    match err {
        DomainError::UnresolvedPrompts { names, .. } => assert_eq!(names, vec!["greeting"]),
        other => panic!("expected UnresolvedPrompts, got {other:?}"),
    }
}

#[test]
fn all_punctuation_title_falls_back_to_untitled_slug() {
    let (vault, _store) = vault_with(config_with_person(), &[]);
    let path = vault
        .create_custom_note(at(), "person", "!!!@@@###", &fields(&[("name", "X")]))
        .expect("create");
    assert_eq!(path, vp("people/untitled.md"));
}

#[test]
fn list_refuses_a_builtin_or_unknown_type() {
    let (vault, _store) = vault_with(config_with_person(), &[]);
    assert!(matches!(
        vault.list_custom_notes("project"),
        Err(DomainError::BuiltinTypeNotCustom { .. })
    ));
    assert!(matches!(
        vault.list_custom_notes("gadget"),
        Err(DomainError::UnknownNoteType { .. })
    ));
}
