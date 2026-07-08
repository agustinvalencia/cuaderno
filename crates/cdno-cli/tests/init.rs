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

/// The `cdno-domain` built-in daily template — the fallback used when no
/// custom one exists.
const DOMAIN_DAILY: &str = include_str!("../../cdno-domain/templates/daily.md");

#[test]
fn init_daily_seed_matches_the_domain_builtin_template() {
    // `cdno init` seeds `daily.md` into `.cuaderno/templates/`, where it
    // becomes an active custom override (daily is template-driven, #212).
    // If it drifted from the built-in default, an init'd vault would
    // render dailies differently from a non-init'd one. Pin them equal.
    assert_eq!(
        EMBEDDED_DAILY, DOMAIN_DAILY,
        "the init daily seed must stay byte-identical to the cdno-domain built-in daily template"
    );
}

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
    assert!(exists(&format!("journal/{year}/monthly")));
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
