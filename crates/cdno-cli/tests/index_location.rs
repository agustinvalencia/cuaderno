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
fn a_corrupt_sqlite_index_at_the_override_self_heals_on_open() {
    let vault = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let index = elsewhere.path().join("index.db");
    init(vault.path());
    // Recognisably an index (the SQLite header) but unreadable past it.
    let mut corrupt = b"SQLite format 3\0".to_vec();
    corrupt.extend(std::iter::repeat_n(0xFFu8, 2048));
    std::fs::write(&index, &corrupt).unwrap();

    cdno(vault.path())
        .env(paths::INDEX_PATH_ENV, &index)
        .arg("commitments")
        .assert()
        .success();
    assert_ne!(std::fs::read(&index).unwrap(), corrupt);
    assert_eq!(index_files_in_meta_dir(vault.path()), Vec::<String>::new());
}

/// A mistyped override that names some other file: neither the open-time
/// self-heal nor `reindex` may delete it.
#[test]
fn a_file_that_is_not_an_index_is_refused_and_left_byte_identical() {
    let vault = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let other = elsewhere.path().join("surrogate-model.notes");
    let content = b"# Surrogate model\n\nNot an index.\n";
    init(vault.path());
    std::fs::write(&other, content).unwrap();

    for verb in ["commitments", "reindex"] {
        cdno(vault.path())
            .env(paths::INDEX_PATH_ENV, &other)
            .arg(verb)
            .timeout(Duration::from_secs(15))
            .assert()
            .failure()
            .stderr(predicate::str::contains("not a cuaderno index"))
            .stderr(predicate::str::contains("nothing was deleted"));
        assert_eq!(std::fs::read(&other).unwrap(), content, "{verb}");
    }
}

/// A mistyped override that names another application's SQLite database:
/// refused by every verb, `reindex --rebind` included, never switched to
/// WAL, migrated or deleted.
#[test]
fn another_applications_sqlite_database_is_refused_and_left_byte_identical() {
    let vault = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let other = elsewhere.path().join("bookmarks.sqlite");
    init(vault.path());
    {
        let conn = rusqlite::Connection::open(&other).unwrap();
        conn.execute_batch(
            "CREATE TABLE bookmarks (url TEXT PRIMARY KEY, title TEXT);
             INSERT INTO bookmarks VALUES ('https://example.org', 'Example');",
        )
        .unwrap();
    }
    let before = std::fs::read(&other).unwrap();

    for args in [&["commitments"][..], &["reindex"], &["reindex", "--rebind"]] {
        cdno(vault.path())
            .env(paths::INDEX_PATH_ENV, &other)
            .args(args)
            .timeout(Duration::from_secs(15))
            .assert()
            .failure()
            .stderr(predicate::str::contains("not a cuaderno index"));
        assert_eq!(std::fs::read(&other).unwrap(), before, "{args:?}");
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut sidecar = other.as_os_str().to_owned();
            sidecar.push(suffix);
            assert!(!Path::new(&sidecar).exists(), "{args:?} created {suffix}");
        }
    }
}

#[test]
fn an_override_naming_a_note_file_is_refused_before_anything_is_opened() {
    let vault = tempdir().unwrap();
    init(vault.path());
    let note = vault.path().join("projects").join("surrogate-model.md");
    std::fs::create_dir_all(note.parent().unwrap()).unwrap();
    std::fs::write(&note, "# Surrogate model\n").unwrap();

    cdno(vault.path())
        .env(paths::INDEX_PATH_ENV, &note)
        .arg("reindex")
        .timeout(Duration::from_secs(15))
        .assert()
        .failure()
        .stderr(predicate::str::contains("names a note file"));
    assert_eq!(
        std::fs::read_to_string(&note).unwrap(),
        "# Surrogate model\n"
    );
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

// ---------------------------------------------------------------------
// One override, two vaults: the index is bound to the first (#998)
// ---------------------------------------------------------------------

fn create_project(vault: &Path, index: &Path, title: &str) {
    cdno(vault)
        .env(paths::INDEX_PATH_ENV, index)
        .args(["project", "create", "--title", title, "--context", "work"])
        .arg("--no-interactive")
        .assert()
        .success();
}

fn search(vault: &Path, index: &Path, query: &str) -> assert_cmd::assert::Assert {
    cdno(vault)
        .env(paths::INDEX_PATH_ENV, index)
        .args(["search", query, "--no-interactive"])
        .timeout(Duration::from_secs(15))
        .assert()
}

#[test]
fn a_second_vault_on_the_same_override_is_refused_and_the_first_is_untouched() {
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let index = elsewhere.path().join("index.db");
    init(a.path());
    init(b.path());
    create_project(a.path(), &index, "Surrogate model");

    let a_root = a.path().canonicalize().unwrap();
    let b_root = b.path().canonicalize().unwrap();
    search(b.path(), &index, "surrogate")
        .failure()
        .stderr(predicate::str::contains(a_root.to_string_lossy()))
        .stderr(predicate::str::contains(b_root.to_string_lossy()))
        .stderr(predicate::str::contains("cdno reindex"));

    // The first vault's index still answers for it.
    search(a.path(), &index, "surrogate")
        .success()
        .stdout(predicate::str::contains("surrogate-model"));
}

#[test]
fn plain_reindex_refuses_an_index_bound_to_another_vault() {
    // The other vault may still be served from this file: rebinding takes
    // an explicit `--rebind`.
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let index = elsewhere.path().join("index.db");
    init(a.path());
    init(b.path());
    create_project(a.path(), &index, "Surrogate model");

    let a_root = a.path().canonicalize().unwrap();
    let b_root = b.path().canonicalize().unwrap();
    cdno(b.path())
        .env(paths::INDEX_PATH_ENV, &index)
        .arg("reindex")
        .timeout(Duration::from_secs(15))
        .assert()
        .failure()
        .stderr(predicate::str::contains(a_root.to_string_lossy()))
        .stderr(predicate::str::contains(b_root.to_string_lossy()))
        .stderr(predicate::str::contains("cdno reindex --rebind"))
        .stderr(predicate::str::contains("first stop whatever serves"));

    // Untouched: still A's index.
    search(a.path(), &index, "surrogate")
        .success()
        .stdout(predicate::str::contains("surrogate-model"));
}

#[test]
fn reindex_rebind_gives_an_overridden_index_to_this_vault_and_says_so() {
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let index = elsewhere.path().join("index.db");
    init(a.path());
    init(b.path());
    create_project(a.path(), &index, "Surrogate model");

    let a_root = a.path().canonicalize().unwrap();
    cdno(b.path())
        .env(paths::INDEX_PATH_ENV, &index)
        .args(["reindex", "--rebind"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "Index: {} (from CUADERNO_INDEX_PATH)",
            index.display()
        )))
        .stdout(predicate::str::contains(format!(
            "Rebound the index from the vault at {}",
            a_root.display()
        )));

    // Now bound to B: B opens it, A is refused.
    search(b.path(), &index, "surrogate").success();
    search(a.path(), &index, "surrogate")
        .failure()
        .stderr(predicate::str::contains("cdno reindex"));
}

#[test]
fn reindex_of_the_bound_vault_does_not_claim_a_rebind() {
    let vault = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let index = elsewhere.path().join("index.db");
    init(vault.path());
    create_project(vault.path(), &index, "Surrogate model");

    cdno(vault.path())
        .env(paths::INDEX_PATH_ENV, &index)
        .arg("reindex")
        .assert()
        .success()
        .stdout(predicate::str::contains("Rebound").not());
}

#[test]
fn an_existing_index_with_no_recorded_vault_is_bound_on_first_use() {
    // A default-location index copied out to an override records no vault;
    // the first open through the override binds it.
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let index = elsewhere.path().join("index.db");
    init(a.path());
    init(b.path());
    cdno(a.path()).arg("commitments").assert().success();
    std::fs::copy(a.path().join(paths::INDEX_DB), &index).unwrap();

    search(a.path(), &index, "anything").success();
    search(b.path(), &index, "anything")
        .failure()
        .stderr(predicate::str::contains("belongs to the vault at"));
}

#[test]
fn reindex_at_the_default_location_does_not_print_an_index_line() {
    let vault = tempdir().unwrap();
    init(vault.path());
    cdno(vault.path())
        .arg("reindex")
        .assert()
        .success()
        .stdout(predicate::str::contains("Index:").not());
}

/// The workspace `.cargo/config.toml` forces the variable empty for every
/// process cargo runs, so a contributor's exported value never reaches the
/// suite. This pins that the setting is in effect.
#[test]
fn the_test_process_sees_the_index_variable_forced_empty() {
    assert_eq!(
        std::env::var_os(paths::INDEX_PATH_ENV).as_deref(),
        Some(OsStr::new("")),
        "`.cargo/config.toml` must set CUADERNO_INDEX_PATH to \"\" with force = true"
    );
}
