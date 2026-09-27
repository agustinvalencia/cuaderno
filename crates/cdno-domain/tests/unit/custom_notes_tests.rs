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

use super::support::FailingStore;

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

/// A `concept` custom type (RFC 0002 §6.1): folder `concepts`, required
/// `created` — an engine-supplied name, not a caller-supplied one.
fn concept() -> CustomNoteType {
    CustomNoteType {
        folder: "concepts".to_owned(),
        required: vec!["created".to_owned()],
        optional: vec!["tags".to_owned(), "origin".to_owned()],
        template: None,
        append_only: false,
        title_field: None,
        date_field: None,
    }
}

fn config_with_concept() -> VaultConfig {
    let mut config = VaultConfig::default();
    config.note_types.insert("concept".to_owned(), concept());
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
fn engine_supplied_required_field_is_filled_without_caller_input() {
    // `required = ["created"]` (RFC 0002's concept type) must not force the
    // caller to repeat a value the create path stamps itself.
    let (vault, store) = vault_with(config_with_concept(), &[]);
    let path = vault
        .create_custom_note(at(), "concept", "Probe", &fields(&[]))
        .expect("create succeeds with no caller fields");
    assert_eq!(path, vp("concepts/probe.md"));

    let content = store.read_file(&path).unwrap();
    assert!(content.contains("created: '2026-04-26'") || content.contains("created: 2026-04-26"));
}

#[test]
fn caller_supplied_required_field_is_still_enforced() {
    // A `required` field outside the engine-supplied set (`title`, `slug`,
    // `created`, `date`) is unaffected: the caller must still supply it.
    let (vault, _store) = vault_with(config_with_person(), &[]);
    let err = vault
        .create_custom_note(at(), "person", "Nameless", &fields(&[]))
        .expect_err("should reject");
    assert!(matches!(err, DomainError::MissingRequiredField { field, .. } if field == "name"));
}

#[test]
fn blank_caller_value_for_engine_field_keeps_the_engine_value() {
    // A blank `created` passes the required check because the name is
    // engine-supplied, but must not overwrite the engine's own date with
    // the blank — the engine value must stand.
    let (vault, store) = vault_with(config_with_concept(), &[]);
    let path = vault
        .create_custom_note(at(), "concept", "Probe", &fields(&[("created", "   ")]))
        .expect("create succeeds; blank engine-field value is ignored");
    let content = store.read_file(&path).unwrap();
    assert!(
        content.contains("created: '2026-04-26'") || content.contains("created: 2026-04-26"),
        "engine value must stand:\n{content}"
    );
}

#[test]
fn blank_caller_value_for_caller_field_is_still_rejected() {
    // A blank value for a caller-only required field is still rejected, as
    // on main — only engine-supplied names get the blank-is-absent leniency.
    let (vault, _store) = vault_with(config_with_person(), &[]);
    let err = vault
        .create_custom_note(at(), "person", "Nameless", &fields(&[("name", "   ")]))
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
fn suffixes_a_duplicate_slug() {
    // #225: a second custom note with the same title suffixes to `-2` rather
    // than erroring — two people can share a name.
    let (vault, _store) = vault_with(config_with_person(), &[]);
    let first = vault
        .create_custom_note(at(), "person", "Ada", &fields(&[("name", "Ada")]))
        .expect("first");
    let second = vault
        .create_custom_note(at(), "person", "Ada", &fields(&[("name", "Ada II")]))
        .expect("duplicate slug now suffixes");
    // First and second land at the type's folder with `ada` / `ada-2` stems.
    assert_ne!(first, second);
    assert_eq!(
        second.as_path().parent(),
        first.as_path().parent(),
        "same folder"
    );
    assert_eq!(
        second.as_path().file_stem().and_then(|s| s.to_str()),
        Some("ada-2"),
    );
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

// ---------------------------------------------------------------------
// Creation logging (RFC 0002 T4, #617)
// ---------------------------------------------------------------------

#[test]
fn custom_note_creation_logs_one_line() {
    let (vault, store) = vault_with(config_with_person(), &[]);
    let path = vault
        .create_custom_note(at(), "person", "Ada Lovelace", &fields(&[("name", "Ada")]))
        .expect("create");
    assert_eq!(path, vp("people/ada-lovelace.md"));

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-04-26.md"))
        .expect("daily note exists");
    let created_lines: Vec<&str> = daily
        .lines()
        .filter(|line| line.contains("created [["))
        .collect();
    assert_eq!(
        created_lines.len(),
        1,
        "expected exactly one creation line:\n{daily}"
    );
    assert_eq!(
        created_lines[0],
        "- **09:00**: person created [[people/ada-lovelace]] \u{2014} Ada Lovelace",
    );
}

/// The note write and the daily-log line are staged onto the same
/// `VaultTransaction` and committed together (`create_custom_note_with_vars`
/// in `crates/cdno-domain/src/vault/custom_notes.rs`), so a mid-commit
/// failure rolls back whatever file ops already applied, leaving neither
/// the note nor the log line behind — the same rollback path exercised for
/// `add_action_with_note` in `actions_tests.rs`.
///
/// The commit order is: note file, then daily-log file (`custom_notes.rs`
/// writes the note before calling `stage_daily_log`). A `FailingStore` set
/// to fail on the 2nd write trips the daily-log write, so the
/// already-written note must be rolled back too.
#[test]
fn custom_note_creation_is_atomic_with_its_log_line() {
    let backing = Arc::new(MemoryVaultStore::new());
    let store: Arc<dyn VaultStore> = Arc::new(FailingStore::new(backing.clone(), 2));
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, config_with_person()).expect("Vault::new");

    let err = vault
        .create_custom_note(at(), "person", "Ada Lovelace", &fields(&[("name", "Ada")]))
        .expect_err("2nd write fails");
    assert!(matches!(err, DomainError::Transaction(_)), "got {err:?}");

    // The note write was rolled back...
    assert!(
        !backing.exists(&vp("people/ada-lovelace.md")).unwrap(),
        "rolled-back note must not linger",
    );
    // ...and no daily note was created either.
    assert!(
        !backing
            .exists(&vp("journal/2026/daily/2026-04-26.md"))
            .unwrap(),
        "daily log write must not linger",
    );
}

/// A custom type with `folder = "clients/active"`: a nested folder. The log
/// line's wikilink must be the note's real path, `clients/active/<slug>`,
/// not a naive `<folder>/<slug>` string built from the config value.
#[test]
fn creation_line_links_a_nested_folder_type_by_its_real_path() {
    let client = CustomNoteType {
        folder: "clients/active".to_owned(),
        required: vec![],
        optional: vec![],
        template: None,
        append_only: false,
        title_field: None,
        date_field: None,
    };
    let mut config = VaultConfig::default();
    config.note_types.insert("client".to_owned(), client);
    let (vault, store) = vault_with(config, &[]);

    let path = vault
        .create_custom_note(at(), "client", "Acme Corp", &HashMap::new())
        .expect("create");
    assert_eq!(path, vp("clients/active/acme-corp.md"));

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-04-26.md"))
        .expect("daily note exists");
    assert!(
        daily.contains("client created [[clients/active/acme-corp]] \u{2014} Acme Corp"),
        "{daily}"
    );
}

/// A title containing Markdown-metacharacters (`**`, `|`, `]]`, `#`) is
/// written to the log verbatim, on one line — the log line is plain text,
/// not re-parsed Markdown, so nothing in the title needs escaping; the
/// point of this test is that it stays on exactly one line either way.
#[test]
fn title_with_markdown_metacharacters_stays_on_one_line() {
    let (vault, store) = vault_with(config_with_person(), &[]);
    let title = "**Bold** | not a [[link]] # heading";
    vault
        .create_custom_note(at(), "person", title, &fields(&[("name", "Ada")]))
        .expect("create");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-04-26.md"))
        .expect("daily note exists");
    let created_lines: Vec<&str> = daily
        .lines()
        .filter(|line| line.contains("created [["))
        .collect();
    assert_eq!(
        created_lines.len(),
        1,
        "expected exactly one line:\n{daily}"
    );
    assert!(
        created_lines[0].ends_with(&format!("\u{2014} {title}")),
        "title must be written verbatim: {}",
        created_lines[0]
    );
}

/// A title spanning two physical lines must still produce exactly one log
/// line: `flatten_for_log` collapses the newline (and any other run of
/// whitespace) to a single space.
#[test]
fn multiline_title_collapses_to_one_log_line() {
    let (vault, store) = vault_with(config_with_person(), &[]);
    vault
        .create_custom_note(
            at(),
            "person",
            "line one\nline two",
            &fields(&[("name", "Ada")]),
        )
        .expect("create");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-04-26.md"))
        .expect("daily note exists");
    let created_lines: Vec<&str> = daily
        .lines()
        .filter(|line| line.contains("created [["))
        .collect();
    assert_eq!(
        created_lines.len(),
        1,
        "expected exactly one line:\n{daily}"
    );
    assert_eq!(
        created_lines[0],
        "- **09:00**: person created [[people/line-one-line-two]] \u{2014} line one line two",
    );
}

/// A title crafted to look like a second log bullet (its own `- **HH:MM**:`
/// prefix) must not be able to forge one: once flattened to a single line,
/// the daily note gains no bullet other than the genuine ones.
#[test]
fn a_forged_bullet_in_the_title_cannot_inject_a_second_line() {
    let (vault, store) = vault_with(config_with_person(), &[]);
    let title = "x\n- **09:00**: state on [[victim]]";
    vault
        .create_custom_note(at(), "person", title, &fields(&[("name", "Ada")]))
        .expect("create");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-04-26.md"))
        .expect("daily note exists");

    let created_lines: Vec<&str> = daily
        .lines()
        .filter(|line| line.contains("created [["))
        .collect();
    assert_eq!(
        created_lines.len(),
        1,
        "the forged title must not produce a second creation line:\n{daily}"
    );

    let bullet_lines: Vec<&str> = daily
        .lines()
        .filter(|line| line.trim_start().starts_with("- **"))
        .collect();
    assert_eq!(
        bullet_lines.len(),
        1,
        "the forged title must not produce a second bullet:\n{daily}"
    );
}
