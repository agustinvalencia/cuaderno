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
