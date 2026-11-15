//! `CUADERNO_INDEX_PATH`: a per-machine index location (#998).
//!
//! Subprocess runs of the real binary, so the variable is read the way a
//! user's shell supplies it. Each test sets or removes it explicitly; none
//! inherits the developer's environment.

use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

use assert_cmd::Command;
use cdno_core::paths;
use predicates::prelude::*;
use tempfile::tempdir;

fn cdno(vault: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd.env_remove(paths::INDEX_PATH_ENV);
    cmd.arg("--vault").arg(vault);
    cmd
}

fn init(vault: &Path) {
    Command::cargo_bin("cdno")
        .unwrap()
        .env_remove("CUADERNO_VAULT_PATH")
        .env_remove(paths::INDEX_PATH_ENV)
        .arg("init")
        .arg(vault)
        .assert()
        .success();
}

/// Every file under `<vault>/.cuaderno/` whose name starts `index`: the
/// database and any sidecar or sibling it could have.
fn index_files_in_meta_dir(vault: &Path) -> Vec<String> {
    std::fs::read_dir(vault.join(paths::CUADERNO_DIR))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("index"))
        .collect()
}

#[test]
fn with_the_override_no_index_file_is_created_in_the_vault_and_verbs_work() {
    let vault = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    // A parent that does not exist yet: the open creates it.
    let index = elsewhere.path().join("cache").join("surrogate.db");
    init(vault.path());

    cdno(vault.path())
        .env(paths::INDEX_PATH_ENV, &index)
        .args([
            "project",
            "create",
            "--title",
            "Surrogate model",
            "--context",
            "work",
            "--no-interactive",
        ])
        .assert()
        .success();
    cdno(vault.path())
        .env(paths::INDEX_PATH_ENV, &index)
        .args(["log", "Tuned the surrogate kernel"])
        .assert()
        .success();
    cdno(vault.path())
        .env(paths::INDEX_PATH_ENV, &index)
        .args(["search", "surrogate", "--no-interactive"])
        .assert()
        .success()
        .stdout(predicate::str::contains("surrogate-model"));
    cdno(vault.path())
        .env(paths::INDEX_PATH_ENV, &index)
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("surrogate-model"));

    assert!(index.is_file(), "the index lives at the override");
    assert_eq!(
        index_files_in_meta_dir(vault.path()),
        Vec::<String>::new(),
        "no index file may be created under .cuaderno/"
    );
}

#[test]
fn reindex_rebuilds_the_index_at_the_override() {
    let vault = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let index = elsewhere.path().join("index.db");
    init(vault.path());
    std::fs::write(
        vault.path().join("zettel.md"),
        "---\ntype: zettel\ntitle: Surrogate model\n---\n# Surrogate model\n",
    )
    .unwrap();

    for _ in 0..2 {
        cdno(vault.path())
            .env(paths::INDEX_PATH_ENV, &index)
            .arg("reindex")
            .assert()
            .success()
            .stdout(predicate::str::contains("Reindexed 1 note(s)"));
    }
    assert!(index.is_file());
    assert_eq!(index_files_in_meta_dir(vault.path()), Vec::<String>::new());
}

#[test]
fn reindex_leaves_the_default_file_alone_when_the_override_is_set() {
    // The deletion targets the resolved file, never `.cuaderno/index.db`.
    let vault = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    init(vault.path());
    let default = vault.path().join(paths::INDEX_DB);
    std::fs::write(&default, b"left over from before the override").unwrap();

    cdno(vault.path())
        .env(paths::INDEX_PATH_ENV, elsewhere.path().join("index.db"))
        .arg("reindex")
        .assert()
        .success();
    assert_eq!(
        std::fs::read(&default).unwrap(),
        b"left over from before the override"
    );
}

#[test]
fn a_corrupt_index_at_the_override_self_heals_on_open() {
    let vault = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let index = elsewhere.path().join("index.db");
    init(vault.path());
    std::fs::write(&index, b"corrupt garbage").unwrap();

    cdno(vault.path())
        .env(paths::INDEX_PATH_ENV, &index)
        .arg("commitments")
        .assert()
        .success();
    assert_ne!(std::fs::read(&index).unwrap(), b"corrupt garbage");
    assert_eq!(index_files_in_meta_dir(vault.path()), Vec::<String>::new());
}

#[test]
fn a_relative_override_is_refused_and_creates_nothing() {
    let vault = tempdir().unwrap();
    let cwd = tempdir().unwrap();
    init(vault.path());

    for verb in ["commitments", "reindex"] {
        cdno(vault.path())
            .current_dir(cwd.path())
            .env(paths::INDEX_PATH_ENV, "cache/index.db")
            .arg(verb)
            // Bounded: a refusal is immediate, and a regression must fail,
            // not hang (a killed run also misses the stderr checks below).
            .timeout(Duration::from_secs(15))
            .assert()
            .failure()
            .stderr(predicate::str::contains("CUADERNO_INDEX_PATH"))
            .stderr(predicate::str::contains("absolute"));
    }
    assert!(!cwd.path().join("cache").exists(), "nothing in the cwd");
    assert!(!vault.path().join("cache").exists(), "nothing in the vault");
    assert_eq!(index_files_in_meta_dir(vault.path()), Vec::<String>::new());
}

#[test]
fn an_override_naming_a_directory_is_refused() {
    let vault = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    init(vault.path());

    cdno(vault.path())
        .env(paths::INDEX_PATH_ENV, elsewhere.path())
        .arg("commitments")
        .timeout(Duration::from_secs(15))
        .assert()
        .failure()
        .stderr(predicate::str::contains("names a directory"));
}

#[test]
fn unset_or_blank_means_the_default_location() {
    for value in [None, Some(""), Some("   ")] {
        let vault = tempdir().unwrap();
        init(vault.path());
        let mut cmd = cdno(vault.path());
        if let Some(v) = value {
            cmd.env(paths::INDEX_PATH_ENV, OsStr::new(v));
        }
        cmd.arg("commitments").assert().success();
        assert!(
            vault.path().join(paths::INDEX_DB).is_file(),
            "{value:?} must leave the index at .cuaderno/index.db"
        );
    }
}
