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
            None,
            None,
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

// ---------------------------------------------------------------------
// `body` and `origin` on creation (RFC 0002 T6, #619)
// ---------------------------------------------------------------------

/// `create_custom_note_with_vars` with no fields or prompted vars beyond
/// `fields`, and the given `body` / `origin`.
fn create_with(
    vault: &Vault,
    type_name: &str,
    title: &str,
    fields: &HashMap<String, String>,
    body: Option<&str>,
    origin: Option<&str>,
) -> Result<VaultPath, DomainError> {
    vault.create_custom_note_with_vars(
        at(),
        type_name,
        title,
        fields,
        &HashMap::new(),
        body,
        origin,
    )
}

const WOODBURY_ORIGIN: &str = "[[journal/2026/daily/2026-09-02#Woodbury identity]]";

#[test]
fn body_fills_a_body_placeholder_in_a_custom_template() {
    let template =
        "---\ntype: concept\ncreated: {{created}}\n---\n\n# {{title}}\n\n{{body}}\n\n## See also\n";
    let (vault, store) = vault_with(
        config_with_concept(),
        &[(".cuaderno/templates/concept.md", template)],
    );
    let path = create_with(
        &vault,
        "concept",
        "Woodbury identity",
        &HashMap::new(),
        Some("  The inverse of a low-rank update.\n\nSee [[projects/alpha]].  \n"),
        None,
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    assert_eq!(
        content,
        "---\ntype: concept\ncreated: 2026-04-26\n---\n\n# Woodbury identity\n\n\
         \x20 The inverse of a low-rank update.\n\nSee [[projects/alpha]].\n\n## See also\n",
        "the first-line indent survives; only trailing whitespace is stripped"
    );
}

#[test]
fn body_placeholder_renders_empty_when_no_body_is_given() {
    // The engine leaves an unknown `{{name}}` in literally; the create path
    // must supply `body` so the placeholder vanishes instead.
    let template = "---\ntype: concept\ncreated: {{created}}\n---\n\n# {{title}}\n\n{{body}}\n";
    let (vault, store) = vault_with(
        config_with_concept(),
        &[(".cuaderno/templates/concept.md", template)],
    );
    let path =
        create_with(&vault, "concept", "Empty", &HashMap::new(), None, None).expect("create");
    let content = store.read_file(&path).unwrap();
    assert!(!content.contains("{{body}}"), "{content}");
    assert_eq!(
        content,
        "---\ntype: concept\ncreated: 2026-04-26\n---\n\n# Empty\n\n\n"
    );
}

#[test]
fn body_is_inserted_after_the_h1_when_the_template_has_no_placeholder() {
    let template = "---\ntype: concept\ncreated: {{created}}\n---\n\n# {{title}}\n\n## See also\n";
    let (vault, store) = vault_with(
        config_with_concept(),
        &[(".cuaderno/templates/concept.md", template)],
    );
    let path = create_with(
        &vault,
        "concept",
        "Woodbury identity",
        &HashMap::new(),
        Some("\nFirst paragraph.\n\n"),
        None,
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    assert_eq!(
        content,
        "---\ntype: concept\ncreated: 2026-04-26\n---\n\n# Woodbury identity\n\n\
         First paragraph.\n\n## See also\n"
    );
}

#[test]
fn body_is_inserted_after_the_h1_of_the_synthesised_note() {
    let (vault, store) = vault_with(config_with_concept(), &[]);
    let path = create_with(
        &vault,
        "concept",
        "Woodbury identity",
        &HashMap::new(),
        Some("The inverse of a low-rank update."),
        None,
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    assert!(
        content.ends_with("\n# Woodbury identity\n\nThe inverse of a low-rank update.\n"),
        "{content}"
    );
    // The H1 still names the note, and the note lints clean.
    let report = vault.lint_all_notes().unwrap();
    assert!(report.is_clean(), "issues: {:?}", report.issues);
}

#[test]
fn body_is_appended_when_the_template_has_no_h1() {
    let template = "---\ntype: concept\ncreated: {{created}}\n---\n\nIntro line.\n\n";
    let (vault, store) = vault_with(
        config_with_concept(),
        &[(".cuaderno/templates/concept.md", template)],
    );
    let path = create_with(
        &vault,
        "concept",
        "No heading",
        &HashMap::new(),
        Some("Body text."),
        None,
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    assert!(
        content.ends_with("\nIntro line.\n\nBody text.\n"),
        "{content}"
    );
}

#[test]
fn blank_body_and_blank_origin_count_as_absent() {
    let (with_blank, blank_store) = vault_with(config_with_concept(), &[]);
    let blank = create_with(
        &with_blank,
        "concept",
        "Same",
        &HashMap::new(),
        Some("  \n\t "),
        Some("   "),
    )
    .expect("blank body and origin are absent, not errors");
    let (without, plain_store) = vault_with(config_with_concept(), &[]);
    let plain =
        create_with(&without, "concept", "Same", &HashMap::new(), None, None).expect("create");
    assert_eq!(
        blank_store.read_file(&blank).unwrap(),
        plain_store.read_file(&plain).unwrap()
    );
    assert!(!blank_store.read_file(&blank).unwrap().contains("origin"));

    // A blank origin on a type that does not declare it is absent too, so it
    // is not refused.
    let (person_vault, _s) = vault_with(config_with_person(), &[]);
    create_with(
        &person_vault,
        "person",
        "Ada",
        &fields(&[("name", "Ada")]),
        None,
        Some(" "),
    )
    .expect("a blank origin is not an undeclared field");
}

#[test]
fn origin_lands_in_frontmatter_as_a_plain_string() {
    let (vault, store) = vault_with(config_with_concept(), &[]);
    let origin = "[[journal/2026/daily/2026-09-02#Woodbury identity]] [[journal/2026/daily/2026-09-24#Low-rank refit]]";
    let path = create_with(
        &vault,
        "concept",
        "Woodbury identity",
        &HashMap::new(),
        None,
        Some(origin),
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&content).unwrap();
    assert_eq!(
        fm.optional_field::<String>("origin").unwrap().as_deref(),
        Some(origin),
        "{content}"
    );
}

#[test]
fn origin_on_a_type_that_does_not_declare_it_is_refused() {
    let (vault, store) = vault_with(config_with_person(), &[]);
    let err = create_with(
        &vault,
        "person",
        "Ada",
        &fields(&[("name", "Ada")]),
        Some("Body."),
        Some(WOODBURY_ORIGIN),
    )
    .expect_err("person does not declare origin");
    match err {
        DomainError::UnknownField { note_type, field } => {
            assert_eq!(note_type, "person");
            assert_eq!(field, "origin");
        }
        other => panic!("expected UnknownField, got {other:?}"),
    }
    assert!(!store.exists(&vp("people/ada.md")).unwrap());
    assert!(
        !store
            .exists(&vp("journal/2026/daily/2026-04-26.md"))
            .unwrap(),
        "a refused create writes no log line"
    );
}

#[test]
fn origin_parameter_wins_over_an_origin_field() {
    let (vault, store) = vault_with(config_with_concept(), &[]);
    let path = create_with(
        &vault,
        "concept",
        "Woodbury identity",
        &fields(&[("origin", "[[projects/loser]]")]),
        None,
        Some(WOODBURY_ORIGIN),
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    assert!(content.contains("Woodbury identity]]"), "{content}");
    assert!(!content.contains("loser"), "{content}");
}

#[test]
fn origin_with_a_heading_anchor_resolves_to_its_daily_note_after_reindex() {
    const DAILY: &str = "---\ntype: daily\ndate: 2026-09-02\n---\n\n# 2026-09-02\n\n## Notes\n\n### Woodbury identity\n\nA low-rank inverse.\n";
    let (vault, store) = vault_with(
        config_with_concept(),
        &[("journal/2026/daily/2026-09-02.md", DAILY)],
    );
    let path = create_with(
        &vault,
        "concept",
        "Woodbury identity",
        &HashMap::new(),
        Some("Promoted from the daily note."),
        Some(WOODBURY_ORIGIN),
    )
    .expect("create");

    // A fresh index over the same store: reconciliation rebuilds every edge
    // from the markdown alone.
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (fresh, _report) = Vault::new(
        Arc::clone(&store),
        Arc::clone(&index),
        config_with_concept(),
    )
    .expect("Vault::new");
    let edge = index
        .find_outgoing_links(&path)
        .unwrap()
        .into_iter()
        .find(|l| l.target_raw.starts_with("journal/2026/daily/2026-09-02"))
        .expect("origin is indexed as a link edge");
    assert_eq!(
        edge.resolved_path,
        Some(vp("journal/2026/daily/2026-09-02.md")),
        "the anchored origin link resolves to the daily note (T0)"
    );
    let view = fresh
        .read_note(&vp("journal/2026/daily/2026-09-02.md"))
        .unwrap();
    assert!(view.backlinks.contains(&path), "{:?}", view.backlinks);
}

// ---------------------------------------------------------------------
// Frontmatter reconciliation after rendering (#648 review: A1, A2, B2, B7)
// ---------------------------------------------------------------------

const DAILY_0902: &str = "---\ntype: daily\ndate: 2026-09-02\n---\n\n# 2026-09-02\n\n## Notes\n\n### Woodbury identity\n\nA low-rank inverse.\n";
const DAILY_0924: &str = "---\ntype: daily\ndate: 2026-09-24\n---\n\n# 2026-09-24\n\n## Notes\n\n### Low-rank refit\n\nRefit.\n";
const TWO_ORIGINS: &str = "[[journal/2026/daily/2026-09-02#Woodbury identity]] [[journal/2026/daily/2026-09-24#Low-rank refit]]";

fn frontmatter_of(content: &str) -> cdno_core::frontmatter::Frontmatter {
    cdno_core::frontmatter::Frontmatter::parse(content)
        .unwrap_or_else(|e| panic!("frontmatter parses ({e:?}):\n{content}"))
        .0
}

/// Every outgoing edge of `path`, as its resolved target, from a fresh index
/// rebuilt from the markdown alone.
fn resolved_targets_after_reindex(store: &Arc<dyn VaultStore>, path: &VaultPath) -> Vec<String> {
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let _fresh = Vault::new(Arc::clone(store), Arc::clone(&index), config_with_concept())
        .expect("Vault::new");
    index
        .find_outgoing_links(path)
        .unwrap()
        .into_iter()
        .filter_map(|l| l.resolved_path.map(|p| p.to_string()))
        .collect()
}

#[test]
fn origin_reaches_the_frontmatter_when_the_template_lacks_the_placeholder() {
    let template = "---\ntype: concept\ncreated: {{created}}\n---\n\n# {{title}}\n\n{{body}}\n";
    let (vault, store) = vault_with(
        config_with_concept(),
        &[(".cuaderno/templates/concept.md", template)],
    );
    let path = create_with(
        &vault,
        "concept",
        "Woodbury identity",
        &HashMap::new(),
        Some("Body."),
        Some(WOODBURY_ORIGIN),
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    assert_eq!(
        frontmatter_of(&content)
            .optional_field::<String>("origin")
            .unwrap()
            .as_deref(),
        Some(WOODBURY_ORIGIN),
        "{content}"
    );
    assert!(
        content.ends_with("---\n\n# Woodbury identity\n\nBody.\n"),
        "the body after the frontmatter is untouched:\n{content}"
    );
}

fn create_through_unquoted_origin_template(origin: &str) -> (Arc<dyn VaultStore>, VaultPath) {
    let template = "---\ntype: concept\ncreated: {{created}}\norigin: {{origin}}\n---\n\n# {{title}}\n\n{{body}}\n";
    let (vault, store) = vault_with(
        config_with_concept(),
        &[
            (".cuaderno/templates/concept.md", template),
            ("journal/2026/daily/2026-09-02.md", DAILY_0902),
            ("journal/2026/daily/2026-09-24.md", DAILY_0924),
        ],
    );
    let path = create_with(
        &vault,
        "concept",
        "Woodbury identity",
        &HashMap::new(),
        Some("Promoted."),
        Some(origin),
    )
    .expect("create");
    (store, path)
}

#[test]
fn unquoted_origin_placeholder_with_one_link_parses_back_as_the_string_and_resolves() {
    let (store, path) = create_through_unquoted_origin_template(WOODBURY_ORIGIN);
    let content = store.read_file(&path).unwrap();
    let fm = frontmatter_of(&content);
    assert_eq!(
        fm.optional_field::<String>("origin").unwrap().as_deref(),
        Some(WOODBURY_ORIGIN),
        "not a nested flow sequence:\n{content}"
    );
    // Key order is kept: the repaired `origin` stays where the template put it.
    let keys: Vec<&str> = content
        .lines()
        .skip(1)
        .take_while(|l| *l != "---")
        .filter_map(|l| l.split(':').next())
        .collect();
    assert_eq!(keys, ["type", "created", "origin"], "{content}");
    assert!(content.ends_with("---\n\n# Woodbury identity\n\nPromoted.\n"));
    assert_eq!(
        resolved_targets_after_reindex(&store, &path),
        ["journal/2026/daily/2026-09-02.md"]
    );
}

#[test]
fn unquoted_origin_placeholder_with_two_links_parses_back_as_the_string_and_resolves() {
    // Pasted raw, `origin: [[a]] [[b]]` is not even YAML; the create used to
    // fail with a parse error.
    let (store, path) = create_through_unquoted_origin_template(TWO_ORIGINS);
    let content = store.read_file(&path).unwrap();
    let fm = frontmatter_of(&content);
    assert_eq!(
        fm.optional_field::<String>("origin").unwrap().as_deref(),
        Some(TWO_ORIGINS),
        "{content}"
    );
    assert_eq!(
        fm.optional_field::<String>("created").unwrap().as_deref(),
        Some("2026-04-26")
    );
    assert!(content.ends_with("---\n\n# Woodbury identity\n\nPromoted.\n"));
    let mut targets = resolved_targets_after_reindex(&store, &path);
    targets.sort();
    assert_eq!(
        targets,
        [
            "journal/2026/daily/2026-09-02.md",
            "journal/2026/daily/2026-09-24.md"
        ]
    );
}

#[test]
fn a_template_without_frontmatter_gets_type_and_the_supplied_fields() {
    let template = "# {{title}}\n\n{{body}}\n";
    let (vault, store) = vault_with(
        config_with_concept(),
        &[(".cuaderno/templates/concept.md", template)],
    );
    let path = create_with(
        &vault,
        "concept",
        "Degenerate",
        &fields(&[("tags", "maths")]),
        Some("Body."),
        Some(WOODBURY_ORIGIN),
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    assert_eq!(
        content,
        "---\ntype: concept\ntags: maths\norigin: '[[journal/2026/daily/2026-09-02#Woodbury identity]]'\n---\n\n# Degenerate\n\nBody.\n",
        "type first, then the supplied fields in declared order"
    );
}

#[test]
fn a_template_that_renders_every_field_correctly_is_byte_identical() {
    // A YAML comment and a double-quoted scalar would not survive a
    // re-serialisation, so their presence proves the text was left alone.
    let template = "---\ntype: concept # the type\ncreated: {{created}}\ntags: {{tags}}\norigin: \"{{origin}}\"\n---\n\n# {{title}}\n\n{{body}}\n";
    let (vault, store) = vault_with(
        config_with_concept(),
        &[(".cuaderno/templates/concept.md", template)],
    );
    let path = create_with(
        &vault,
        "concept",
        "Faithful",
        &fields(&[("tags", "maths")]),
        Some("Body."),
        Some(WOODBURY_ORIGIN),
    )
    .expect("create");
    assert_eq!(
        store.read_file(&path).unwrap(),
        "---\ntype: concept # the type\ncreated: 2026-04-26\ntags: maths\norigin: \"[[journal/2026/daily/2026-09-02#Woodbury identity]]\"\n---\n\n# Faithful\n\nBody.\n"
    );
}

#[test]
fn a_wrong_type_is_repaired_and_other_keys_keep_their_order() {
    let template = "---\ncreated: {{created}}\ntype: other\nextra: kept\n---\n\n# {{title}}\n";
    let (vault, store) = vault_with(
        config_with_concept(),
        &[(".cuaderno/templates/concept.md", template)],
    );
    let path = create_with(
        &vault,
        "concept",
        "Retyped",
        &fields(&[("tags", "maths")]),
        None,
        None,
    )
    .expect("create");
    assert_eq!(
        store.read_file(&path).unwrap(),
        "---\ncreated: 2026-04-26\ntype: concept\nextra: kept\ntags: maths\n---\n\n# Retyped\n"
    );
}

#[test]
fn a_body_that_looks_like_frontmatter_cannot_drop_the_type() {
    // B7: a template that is only `{{body}}` lets the body's own `---` block
    // become the note's frontmatter; reconciliation restores `type`.
    let (vault, store) = vault_with(
        config_with_concept(),
        &[(".cuaderno/templates/concept.md", "{{body}}\n")],
    );
    let path = create_with(
        &vault,
        "concept",
        "Hijack",
        &HashMap::new(),
        Some("---\nfoo: bar\n---\n\nText."),
        None,
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    assert_eq!(
        frontmatter_of(&content)
            .optional_field::<String>("type")
            .unwrap()
            .as_deref(),
        Some("concept"),
        "{content}"
    );
}

// ---------------------------------------------------------------------
// `{{body}}` precedence (#648 review: A4, B1, C2)
// ---------------------------------------------------------------------

const BODY_TEMPLATE: &str =
    "---\ntype: concept\ncreated: {{created}}\n---\n\n# {{title}}\n\n{{body}}\n";

/// `concept` with an optional declared `body` field, a `[variables] body`,
/// or a `[variables.prompt] body`, as the flags ask.
fn config_with_body_sources(field: bool, static_var: bool, prompt: bool) -> VaultConfig {
    let mut ty = concept();
    if field {
        ty.optional.push("body".to_owned());
    }
    let mut config = VaultConfig::default();
    config.note_types.insert("concept".to_owned(), ty);
    if static_var {
        config
            .variables
            .static_vars
            .insert("body".to_owned(), "from static".to_owned());
    }
    if prompt {
        config
            .variables
            .prompt
            .insert("body".to_owned(), "Body?".to_owned());
    }
    config
}

fn create_body_source(
    config: VaultConfig,
    fields: &HashMap<String, String>,
    prompted: &HashMap<String, String>,
    body: Option<&str>,
) -> Result<String, DomainError> {
    let (vault, store) = vault_with(config, &[(".cuaderno/templates/concept.md", BODY_TEMPLATE)]);
    let path = vault.create_custom_note_with_vars(
        at(),
        "concept",
        "Sourced",
        fields,
        prompted,
        body,
        None,
    )?;
    Ok(store.read_file(&path).unwrap())
}

#[test]
fn body_parameter_wins_over_every_other_body_source() {
    let content = create_body_source(
        config_with_body_sources(true, true, true),
        &fields(&[("body", "from field")]),
        &fields(&[("body", "from var")]),
        Some("from parameter"),
    )
    .expect("create");
    assert!(
        content.ends_with("# Sourced\n\nfrom parameter\n"),
        "{content}"
    );
}

#[test]
fn a_declared_body_field_fills_the_placeholder_without_a_body_parameter() {
    let content = create_body_source(
        config_with_body_sources(true, true, false),
        &fields(&[("body", "from field")]),
        &HashMap::new(),
        None,
    )
    .expect("create");
    assert!(content.ends_with("# Sourced\n\nfrom field\n"), "{content}");
}

#[test]
fn a_vault_variable_body_fills_the_placeholder_without_a_body_parameter() {
    let content = create_body_source(
        config_with_body_sources(false, true, false),
        &HashMap::new(),
        &HashMap::new(),
        None,
    )
    .expect("create");
    assert!(content.ends_with("# Sourced\n\nfrom static\n"), "{content}");
}

#[test]
fn a_prompted_body_fills_the_placeholder_without_a_body_parameter() {
    let content = create_body_source(
        config_with_body_sources(false, false, true),
        &HashMap::new(),
        &fields(&[("body", "from var")]),
        None,
    )
    .expect("create");
    assert!(content.ends_with("# Sourced\n\nfrom var\n"), "{content}");
}

#[test]
fn an_unanswered_body_prompt_is_reported_rather_than_rendered_empty() {
    let err = create_body_source(
        config_with_body_sources(false, false, true),
        &HashMap::new(),
        &HashMap::new(),
        None,
    )
    .expect_err("the prompt is unanswered");
    match err {
        DomainError::UnresolvedPrompts { names, .. } => assert_eq!(names, ["body"]),
        other => panic!("expected UnresolvedPrompts, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// The title heading, verbatim bodies and trimmed origins
// (#648 review: A3, B3, B4)
// ---------------------------------------------------------------------

#[test]
fn a_leading_title_h1_in_the_body_is_dropped_on_the_placeholder_path() {
    let (vault, store) = vault_with(
        config_with_concept(),
        &[(".cuaderno/templates/concept.md", BODY_TEMPLATE)],
    );
    let path = create_with(
        &vault,
        "concept",
        "Woodbury identity",
        &HashMap::new(),
        Some("\n# Woodbury identity\n\n\nThe inverse.\n"),
        None,
    )
    .expect("create");
    assert_eq!(
        store.read_file(&path).unwrap(),
        "---\ntype: concept\ncreated: 2026-04-26\n---\n\n# Woodbury identity\n\nThe inverse.\n"
    );
}

#[test]
fn a_leading_title_h1_in_the_body_is_dropped_on_the_insert_after_h1_path() {
    let (vault, store) = vault_with(config_with_concept(), &[]);
    let path = create_with(
        &vault,
        "concept",
        "Woodbury identity",
        &HashMap::new(),
        Some("# Woodbury identity #\n\nThe inverse.\n"),
        None,
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    assert_eq!(
        content.matches("# Woodbury identity").count(),
        1,
        "{content}"
    );
    assert!(
        content.ends_with("\n# Woodbury identity\n\nThe inverse.\n"),
        "{content}"
    );
}

#[test]
fn a_leading_h1_that_is_not_the_title_is_kept() {
    let (vault, store) = vault_with(config_with_concept(), &[]);
    let path = create_with(
        &vault,
        "concept",
        "Woodbury identity",
        &HashMap::new(),
        Some("# Something else\n\nText.\n"),
        None,
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    assert!(
        content.ends_with("# Woodbury identity\n\n# Something else\n\nText.\n"),
        "{content}"
    );
}

#[test]
fn an_indented_code_block_opening_the_body_survives() {
    let (vault, store) = vault_with(config_with_concept(), &[]);
    let path = create_with(
        &vault,
        "concept",
        "Code",
        &HashMap::new(),
        Some("\n\n    fn main() {}\n    // indented code   \n\n\n"),
        None,
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    assert!(
        content.ends_with("# Code\n\n    fn main() {}\n    // indented code\n"),
        "leading/trailing blank lines and trailing whitespace go, the indent stays:\n{content}"
    );
}

#[test]
fn origin_is_stored_trimmed() {
    let (vault, store) = vault_with(config_with_concept(), &[]);
    let path = create_with(
        &vault,
        "concept",
        "Padded",
        &HashMap::new(),
        None,
        Some("  [[x/a]]  "),
    )
    .expect("create");
    let content = store.read_file(&path).unwrap();
    assert_eq!(
        frontmatter_of(&content)
            .optional_field::<String>("origin")
            .unwrap()
            .as_deref(),
        Some("[[x/a]]"),
        "{content}"
    );
}

// ---------------------------------------------------------------------
// `insert_body_after_h1` fences and headings (#648 review: A5, B5, C1)
// ---------------------------------------------------------------------

fn insert_after_h1_with(template: &str) -> String {
    let (vault, store) = vault_with(
        config_with_concept(),
        &[(".cuaderno/templates/concept.md", template)],
    );
    let path = create_with(
        &vault,
        "concept",
        "Woodbury identity",
        &HashMap::new(),
        Some("BODY"),
        None,
    )
    .expect("create");
    store.read_file(&path).unwrap()
}

#[test]
fn a_backtick_fenced_heading_before_the_h1_is_not_the_insertion_point() {
    let content = insert_after_h1_with(
        "---\ntype: concept\ncreated: {{created}}\n---\n\n```md\n# not a heading\n```\n\n# {{title}}\n\n## See also\n",
    );
    assert!(
        content.ends_with(
            "```md\n# not a heading\n```\n\n# Woodbury identity\n\nBODY\n\n## See also\n"
        ),
        "{content}"
    );
}

#[test]
fn a_tilde_fenced_heading_before_the_h1_is_not_the_insertion_point() {
    // A backtick line inside a `~~~` fence does not close it.
    let content = insert_after_h1_with(
        "---\ntype: concept\ncreated: {{created}}\n---\n\n~~~\n```\n# not a heading\n~~~\n\n# {{title}}\n\n## See also\n",
    );
    assert!(
        content.ends_with(
            "~~~\n```\n# not a heading\n~~~\n\n# Woodbury identity\n\nBODY\n\n## See also\n"
        ),
        "{content}"
    );
}

#[test]
fn an_h1_indented_by_up_to_three_spaces_is_the_insertion_point() {
    let content = insert_after_h1_with(
        "---\ntype: concept\ncreated: {{created}}\n---\n\n   # {{title}}\n\n## See also\n",
    );
    assert!(
        content.ends_with("   # Woodbury identity\n\nBODY\n\n## See also\n"),
        "{content}"
    );
}
