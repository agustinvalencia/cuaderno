//! End-to-end tests for `cdno note new` / `cdno note list` — the generic
//! create/list command for config-defined custom note types. Runs the built
//! binary via `assert_cmd` so clap dispatch and the domain create path are both
//! exercised.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn cdno() -> Command {
    let mut cmd = Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd
}

/// Init a vault and register a `person` custom type in its config.
fn init_person_vault(dir: &Path) {
    cdno().arg("init").arg(dir).assert().success();
    let cfg = dir.join(".cuaderno/config.toml");
    let mut content = fs::read_to_string(&cfg).unwrap_or_default();
    content.push_str(
        "\n[note_types.person]\nfolder = \"people\"\nrequired = [\"name\"]\noptional = [\"role\"]\n",
    );
    fs::write(&cfg, content).unwrap();
}

fn vault_arg(dir: &Path) -> String {
    dir.to_str().unwrap().to_owned()
}

#[test]
fn note_create_creates_and_note_list_finds_it() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "note",
            "create",
            "person",
            "--title",
            "Ada Lovelace",
            "--field",
            "name=Ada",
            "--field",
            "role=advisor",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("people/ada-lovelace.md"));

    let created = dir.path().join("people/ada-lovelace.md");
    assert!(created.exists());
    let content = fs::read_to_string(&created).unwrap();
    assert!(content.contains("type: person"), "{content}");
    assert!(content.contains("name: Ada"), "{content}");

    // The created note lints clean.
    cdno().args(["--vault", &v, "lint"]).assert().success();

    cdno()
        .args(["--vault", &v, "note", "list", "person"])
        .assert()
        .success()
        .stdout(predicate::str::contains("people/ada-lovelace.md"));
}

#[test]
fn note_create_json_emits_the_write_result() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault", &v, "--json", "note", "create", "person", "--title", "Ada", "--field",
            "name=Ada",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"path\""))
        .stdout(predicate::str::contains("people/ada.md"));
}

#[test]
fn note_create_rejects_a_missing_required_field() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault", &v, "note", "create", "person", "--title", "Nameless",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires field"));
}

#[test]
fn note_create_rejects_an_undeclared_field() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "note",
            "create",
            "person",
            "--title",
            "Ada",
            "--field",
            "name=Ada",
            "--field",
            "hobby=chess",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no field 'hobby'"));
}

#[test]
fn note_create_rejects_an_unregistered_type() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault", &v, "note", "create", "gadget", "--title", "Widget",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown note type"));
}

#[test]
fn note_list_reports_when_empty() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args(["--vault", &v, "note", "list", "person"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No `person` notes."));
}

// ---------------------------------------------------------------------
// --body-file and --origin (RFC 0002 T6, #619)
// ---------------------------------------------------------------------

/// A concept template with a `{{body}}` slot, written where the `concept`
/// type `cdno init` declares looks for it.
fn write_concept_template_with_body(dir: &Path) {
    fs::write(
        dir.join(".cuaderno/templates/concept.md"),
        "---\ntype: concept\ncreated: {{created}}\n---\n\n# {{title}}\n\n{{body}}\n",
    )
    .unwrap();
}

#[test]
fn note_create_writes_body_file_and_origin() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    write_concept_template_with_body(dir.path());
    let v = vault_arg(dir.path());
    let body_file = dir.path().join("body.md");
    fs::write(&body_file, "The inverse of a low-rank update.\n").unwrap();

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
            "--body-file",
            body_file.to_str().unwrap(),
            "--origin",
            "[[journal/2026/daily/2026-09-02#Woodbury identity]]",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("concepts/woodbury-identity.md"));

    let content = fs::read_to_string(dir.path().join("concepts/woodbury-identity.md")).unwrap();
    assert!(
        content.contains("# Woodbury identity\n\nThe inverse of a low-rank update.\n"),
        "{content}"
    );
    assert!(!content.contains("{{body}}"), "{content}");
    // The template has no `{{origin}}`; the frontmatter is reconciled so the
    // promotion link is still recorded, as a string.
    assert!(
        content.contains("origin: '[[journal/2026/daily/2026-09-02#Woodbury identity]]'"),
        "{content}"
    );
}

#[test]
fn note_create_writes_origin_into_frontmatter() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
            "--origin",
            "[[journal/2026/daily/2026-09-02#Woodbury identity]]",
        ])
        .assert()
        .success();

    let content = fs::read_to_string(dir.path().join("concepts/woodbury-identity.md")).unwrap();
    assert!(
        content.contains("origin: '[[journal/2026/daily/2026-09-02#Woodbury identity]]'"),
        "{content}"
    );
}

#[test]
fn note_create_without_body_file_fails_when_the_template_has_a_body_slot() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    write_concept_template_with_body(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "missing required flag: --body-file",
        ));
    assert!(!dir.path().join("concepts/woodbury-identity.md").exists());
}

#[test]
fn note_create_without_body_file_succeeds_when_the_template_has_no_body_slot() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
        ])
        .assert()
        .success();
    assert!(dir.path().join("concepts/woodbury-identity.md").exists());
}

#[test]
fn note_create_with_a_body_var_does_not_demand_body_file() {
    // A `--var body=` fills `{{body}}` itself, so `--body-file` is neither
    // prompted nor demanded under `--no-interactive`.
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    write_concept_template_with_body(dir.path());
    let v = vault_arg(dir.path());

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
            "--var",
            "body=from the var",
        ])
        .assert()
        .success();
    let content = fs::read_to_string(dir.path().join("concepts/woodbury-identity.md")).unwrap();
    assert!(
        content.ends_with("# Woodbury identity\n\nfrom the var\n"),
        "{content}"
    );
}

#[test]
fn note_create_drops_a_leading_title_heading_from_the_body_file() {
    let dir = tempdir().unwrap();
    init_person_vault(dir.path());
    write_concept_template_with_body(dir.path());
    let v = vault_arg(dir.path());
    let body_file = dir.path().join("body.md");
    fs::write(&body_file, "# Woodbury identity\n\nThe inverse.\n").unwrap();

    cdno()
        .args([
            "--vault",
            &v,
            "--no-interactive",
            "note",
            "create",
            "concept",
            "--title",
            "Woodbury identity",
            "--body-file",
            body_file.to_str().unwrap(),
        ])
        .assert()
        .success();
    let content = fs::read_to_string(dir.path().join("concepts/woodbury-identity.md")).unwrap();
    assert_eq!(
        content.matches("# Woodbury identity").count(),
        1,
        "{content}"
    );
}

#[test]
fn note_create_help_names_its_value_types_and_the_title_rule() {
    cdno()
        .args(["note", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--body-file <PATH>"))
        .stdout(predicate::str::contains("--origin <STRING>"))
        .stdout(predicate::str::contains("without the title"));
    cdno()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("`note create`"));
}
