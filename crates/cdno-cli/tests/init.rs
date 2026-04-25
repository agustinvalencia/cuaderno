//! Integration tests for `cdno init`.
//!
//! Each test runs the freshly built `cdno` binary against a temp dir
//! via `assert_cmd`, then inspects the resulting tree. Year-partitioned
//! directories are computed from the wall clock so the assertions
//! match the same logic the binary uses.

use std::fs;

use assert_cmd::Command;
use chrono::{Datelike, Local};
use predicates::prelude::*;
use tempfile::tempdir;

/// Source-of-truth copy of the embedded `daily.md`. Used to verify
/// the binary writes a byte-identical file at init time.
const EMBEDDED_DAILY: &str = include_str!("../templates/daily.md");

fn cdno() -> Command {
    Command::cargo_bin("cdno").expect("cdno binary built")
}

#[test]
fn init_creates_full_directory_tree_with_current_year_partitions() {
    let dir = tempdir().unwrap();
    let target = dir.path();

    let now = Local::now().date_naive();
    let year = now.year();
    let iso_year = now.iso_week().year();

    cdno().arg("init").arg(target).assert().success();

    let exists = |rel: &str| target.join(rel).is_dir();
    assert!(exists(&format!("journal/{year}/daily")));
    assert!(exists(&format!("journal/{iso_year}/weekly")));
    assert!(exists("projects"));
    assert!(exists("projects/_parked"));
    assert!(exists("portfolios"));
    assert!(exists("stewardships"));
    assert!(exists("commitments"));
    assert!(exists(&format!("commitments/_done/{year}")));
    assert!(exists("questions/research"));
    assert!(exists("questions/life"));
    assert!(exists("inbox"));
    assert!(exists(".cuaderno"));
    assert!(exists(".cuaderno/templates"));
}

#[test]
fn init_writes_default_config_with_five_project_cap() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    let config =
        fs::read_to_string(dir.path().join(".cuaderno/config.toml")).expect("config.toml present");
    assert!(config.contains("[vault]"));
    assert!(config.contains("max_active_projects = 5"));
}

#[test]
fn init_dumps_daily_template_byte_identical_to_embedded() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    let dumped = fs::read_to_string(dir.path().join(".cuaderno/templates/daily.md"))
        .expect("daily.md dumped");
    assert_eq!(dumped, EMBEDDED_DAILY);
}

#[test]
fn init_refuses_when_cuaderno_dir_already_exists() {
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
