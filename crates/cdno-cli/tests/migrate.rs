//! End-to-end tests for `cdno migrate [--dry-run] [--json]` (RFC 0006 §8.4,
//! plan T9): wiring only. What a migration plans, fences and stamps is the
//! domain's, tested in `cdno-domain`'s `migrate_tests`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::{TempDir, tempdir};

fn cdno() -> Command {
    let mut cmd = Command::cargo_bin("cdno").expect("cdno binary built");
    // Hermetic: a stray ambient export must never route a test at a real vault.
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd
}

/// A fresh vault whose `[vault] method_version` line is replaced by `line`
/// (`None` removes it, leaving the vault unstamped).
fn vault_with_method_line(line: Option<&str>) -> TempDir {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let config_path = dir.path().join(".cuaderno/config.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    let stamped = format!("method_version = \"{}\"", cdno_domain::METHOD_VERSION);
    assert!(
        config.contains(&stamped),
        "init stamps the vault:\n{config}"
    );
    let replaced = match line {
        Some(line) => config.replace(&stamped, line),
        None => config.replace(&format!("{stamped}\n"), ""),
    };
    fs::write(&config_path, replaced).unwrap();
    dir
}

/// The config's `method_version = …` lines (comments mention the key too).
fn method_lines(dir: &Path) -> Vec<String> {
    fs::read_to_string(dir.join(".cuaderno/config.toml"))
        .unwrap()
        .lines()
        .filter(|line| line.starts_with("method_version"))
        .map(str::to_owned)
        .collect()
}

/// Every file under `root` with its bytes, leaving out the index cache and
/// the lock file, which opening a vault may touch and which hold nothing
/// the markdown does not.
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            let name = relative.to_string_lossy();
            if name.starts_with(".cuaderno/index.db") || name == ".cuaderno/.lock" {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(relative, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// The daily notes under `journal/` that carry a `vault migrated` entry,
/// with their text.
fn daily_notes_with_migration(root: &Path) -> Vec<String> {
    tree(root)
        .into_iter()
        .filter(|(path, _)| path.starts_with("journal"))
        .map(|(_, bytes)| String::from_utf8(bytes).unwrap())
        .filter(|text| text.contains("vault migrated"))
        .collect()
}

fn stdout_of(dir: &Path, args: &[&str]) -> String {
    let output = cdno()
        .current_dir(dir)
        .args(args)
        .assert()
        .success()
        .get_output()
        .clone();
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn dry_run_prints_the_plan_and_writes_nothing() {
    let dir = vault_with_method_line(None);
    // Open once so the index exists before the snapshot.
    cdno()
        .current_dir(dir.path())
        .arg("lint")
        .assert()
        .success();
    let before = tree(dir.path());

    let stdout = stdout_of(dir.path(), &["migrate", "--dry-run"]);

    assert_eq!(tree(dir.path()), before, "a dry run writes nothing");
    let binary = cdno_domain::METHOD_VERSION;
    for needle in [
        "Dry run: nothing written.".to_owned(),
        "vault:      unstamped (read as 1.0)".to_owned(),
        format!("this cdno:  RLM {binary}"),
        "migrations: none: additive".to_owned(),
        format!("would write method_version = \"{binary}\" under [vault]"),
        "would append to the daily note for".to_owned(),
        "vault migrated".to_owned(),
        "was: unstamped (read as 1.0)".to_owned(),
        format!("now: {binary}"),
        "read-through: none".to_owned(),
    ] {
        assert!(stdout.contains(&needle), "missing {needle:?} in:\n{stdout}");
    }
}

#[test]
fn migrate_stamps_an_unstamped_vault_1_0_and_logs_once() {
    let dir = vault_with_method_line(None);
    assert!(method_lines(dir.path()).is_empty());

    let stdout = stdout_of(dir.path(), &["migrate"]);
    assert!(
        stdout.contains("Migrated the vault to RLM 1.0."),
        "unexpected output:\n{stdout}"
    );

    assert_eq!(method_lines(dir.path()), ["method_version = \"1.0\""]);

    let notes = daily_notes_with_migration(dir.path());
    assert_eq!(notes.len(), 1, "one daily note carries the entry");
    let note = &notes[0];
    assert_eq!(note.matches("vault migrated").count(), 1, "{note}");
    assert!(
        note.contains("vault migrated\n  was: unstamped (read as 1.0)\n  now: 1.0"),
        "unexpected entry:\n{note}"
    );

    // Run again: already at the binary's version, nothing more is logged.
    let again = stdout_of(dir.path(), &["migrate"]);
    assert_eq!(again, "vault is at RLM 1.0; nothing to do\n");
    let notes = daily_notes_with_migration(dir.path());
    assert_eq!(notes[0].matches("vault migrated").count(), 1);
}

#[test]
fn migrate_on_a_stamped_vault_is_a_no_op() {
    // `cdno init` stamps the binary's version.
    let dir = vault_with_method_line(Some(&format!(
        "method_version = \"{}\"",
        cdno_domain::METHOD_VERSION
    )));
    cdno()
        .current_dir(dir.path())
        .arg("lint")
        .assert()
        .success();
    let before = tree(dir.path());

    for args in [&["migrate", "--dry-run"][..], &["migrate"][..]] {
        let output = cdno()
            .current_dir(dir.path())
            .args(args)
            .assert()
            .success()
            .get_output()
            .clone();
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!(
                "vault is at RLM {}; nothing to do\n",
                cdno_domain::METHOD_VERSION
            )
        );
        assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    }
    assert_eq!(tree(dir.path()), before, "a no-op writes nothing");
}

#[test]
fn migrate_json_carries_the_same_content() {
    let dir = vault_with_method_line(None);
    let binary = cdno_domain::METHOD_VERSION.to_string();

    let planned: serde_json::Value =
        serde_json::from_str(&stdout_of(dir.path(), &["migrate", "--dry-run", "--json"]))
            .expect("--dry-run --json is JSON");
    assert_eq!(planned["dry_run"], true);
    assert_eq!(planned["outcome"], "planned");
    assert_eq!(planned["vault"], serde_json::Value::Null);
    assert_eq!(planned["vault_described"], "unstamped (read as 1.0)");
    assert_eq!(planned["binary"], binary.as_str());
    assert_eq!(planned["migrations"], serde_json::json!([]));
    assert_eq!(
        planned["config_line"],
        format!("method_version = \"{binary}\"")
    );
    assert_eq!(
        planned["log_entry"],
        format!("vault migrated\n  was: unstamped (read as 1.0)\n  now: {binary}")
    );
    assert_eq!(planned["read_through"], serde_json::json!([]));
    assert!(method_lines(dir.path()).is_empty(), "dry run");

    // The real run reports the same plan as done.
    let migrated: serde_json::Value =
        serde_json::from_str(&stdout_of(dir.path(), &["migrate", "--json"])).expect("JSON");
    assert_eq!(migrated["dry_run"], false);
    assert_eq!(migrated["outcome"], "migrated");
    for key in [
        "vault",
        "vault_described",
        "binary",
        "migrations",
        "config_line",
        "log_entry",
        "read_through",
    ] {
        assert_eq!(migrated[key], planned[key], "{key}");
    }

    // And then there is nothing to do.
    let done: serde_json::Value =
        serde_json::from_str(&stdout_of(dir.path(), &["migrate", "--json"])).expect("JSON");
    assert_eq!(done["outcome"], "nothing_to_do");
    assert_eq!(done["vault"], binary.as_str());
    assert_eq!(done["config_line"], serde_json::Value::Null);
    assert_eq!(done["log_entry"], serde_json::Value::Null);
}

#[test]
fn migrate_on_a_newer_vault_exits_non_zero() {
    let binary = cdno_domain::METHOD_VERSION;
    let newer = format!("{}.{}", binary.major, binary.minor + 1);
    let dir = vault_with_method_line(Some(&format!("method_version = \"{newer}\"")));
    cdno()
        .current_dir(dir.path())
        .arg("lint")
        .assert()
        .success();
    let before = tree(dir.path());

    for args in [&["migrate", "--dry-run"][..], &["migrate"][..]] {
        let output = cdno()
            .current_dir(dir.path())
            .args(args)
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(output.stdout.is_empty(), "nothing on stdout");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains(&format!("RLM {newer}")) && stderr.contains(&format!("RLM {binary}")),
            "the refusal names both versions:\n{stderr}"
        );
    }
    assert_eq!(tree(dir.path()), before, "a refused run writes nothing");
}
