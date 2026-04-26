//! End-to-end tests of the `cdno` binary itself: argument parsing,
//! exit codes, stderr formatting, CWD-as-default. Anything that
//! exercises `main.rs` plumbing rather than command logic.
//!
//! These tests run the built binary as a subprocess via `assert_cmd`,
//! so they exercise clap dispatch and the path-resolution code in
//! `main.rs`. Coverage of those lines is invisible to tarpaulin
//! (subprocess instrumentation isn't tracked); `main.rs` is excluded
//! from the codecov patch report for that reason. The tests still run
//! in CI as a smoke gate.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn cdno() -> Command {
    Command::cargo_bin("cdno").expect("cdno binary built")
}

#[test]
fn init_exits_nonzero_and_emits_stderr_when_cuaderno_dir_already_exists() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    cdno()
        .arg("init")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn init_with_no_path_arg_uses_current_working_directory() {
    let dir = tempdir().unwrap();

    cdno()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success();

    assert!(dir.path().join(".cuaderno/config.toml").is_file());
}

#[test]
fn log_discovers_the_vault_root_when_run_from_a_subdirectory() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    cdno()
        .current_dir(dir.path().join("inbox"))
        .args(["log", "ran from a subdir", "--at", "2026-04-25T09:00:00"])
        .assert()
        .success();

    assert!(
        dir.path()
            .join("journal/2026/daily/2026-04-25.md")
            .is_file()
    );
}

#[test]
fn lint_exits_nonzero_with_summary_on_stderr_when_issues_found() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    std::fs::write(
        dir.path().join("bogus.md"),
        "---\ntype: nonsense\n---\n# Body\n",
    )
    .unwrap();

    cdno()
        .current_dir(dir.path())
        .arg("lint")
        .assert()
        .failure()
        .stderr(predicate::str::contains("1 lint issue"));
}

#[test]
fn log_errors_clearly_when_run_outside_any_vault() {
    let dir = tempdir().unwrap();

    cdno()
        .current_dir(dir.path())
        .args(["log", "anything"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not inside a Cuaderno vault"));
}

#[test]
fn capture_creates_an_inbox_file_when_run_from_inside_a_vault() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    cdno()
        .current_dir(dir.path())
        .args(["capture", "small thought from the cli"])
        .assert()
        .success();

    let inbox = std::fs::read_dir(dir.path().join("inbox")).unwrap();
    let captured: Vec<_> = inbox
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
        .collect();
    assert_eq!(captured.len(), 1, "expected exactly one inbox note");
    let name = captured[0].file_name().into_string().unwrap();
    assert!(
        name.contains("small-thought-from-the-cli"),
        "filename: {name}"
    );
}
