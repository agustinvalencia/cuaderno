//! In-process tests for `commands::init::run`.
//!
//! Calls the library function directly against a tempdir so coverage
//! is tracked. Binary-level concerns (exit codes, CWD defaulting,
//! argument parsing) live in `tests/cli.rs` instead.

use std::fs;

use cdno_cli::commands::init;
use chrono::{Datelike, Local};
use tempfile::tempdir;

const EMBEDDED_DAILY: &str = include_str!("../templates/daily.md");

#[test]
fn run_creates_full_directory_tree_with_current_year_partitions() {
    let dir = tempdir().unwrap();
    let target = dir.path();

    let now = Local::now().date_naive();
    let year = now.year();
    let iso_year = now.iso_week().year();

    init::run(target).expect("init succeeds on fresh dir");

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
fn run_writes_default_config_with_five_project_cap() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();

    let config =
        fs::read_to_string(dir.path().join(".cuaderno/config.toml")).expect("config.toml present");
    assert!(config.contains("[vault]"));
    assert!(config.contains("max_active_projects = 5"));
}

#[test]
fn run_dumps_daily_template_byte_identical_to_embedded() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();

    let dumped = fs::read_to_string(dir.path().join(".cuaderno/templates/daily.md"))
        .expect("daily.md dumped");
    assert_eq!(dumped, EMBEDDED_DAILY);
}

#[test]
fn run_refuses_when_cuaderno_dir_already_exists() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).unwrap();

    let err = init::run(dir.path()).expect_err("re-init must fail");
    let msg = format!("{err}");
    assert!(msg.contains("already exists"), "unexpected error: {msg}");
}

#[test]
fn run_creates_target_directory_when_missing() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("nested-vault");
    assert!(!target.exists(), "precondition: target absent");

    init::run(&target).expect("init creates missing parent");

    assert!(target.join(".cuaderno").is_dir());
    assert!(target.join("inbox").is_dir());
}
