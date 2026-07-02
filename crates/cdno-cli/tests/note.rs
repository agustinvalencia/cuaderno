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
