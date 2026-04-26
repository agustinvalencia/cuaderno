//! In-process tests for `commands::log::run`.

use std::fs;

use cdno_cli::commands::{init, log};
use chrono::{NaiveDate, NaiveTime};
use tempfile::tempdir;

fn moment() -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 4, 25)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(14, 30, 0).unwrap())
}

#[test]
fn log_appends_a_line_to_the_daily_note_for_the_given_moment() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    log::run(dir.path(), moment(), "first entry").expect("log");

    let daily = dir.path().join("journal/2026/daily/2026-04-25.md");
    let content = fs::read_to_string(&daily).expect("daily note exists");
    assert!(content.contains("type: daily"));
    assert!(content.contains("- **14:30**: first entry"));
}

#[test]
fn log_stacks_multiple_entries_under_the_logs_section() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    log::run(dir.path(), moment(), "first entry").expect("log");
    let later = moment() + chrono::Duration::minutes(15);
    log::run(dir.path(), later, "second entry").expect("log");

    let daily = dir.path().join("journal/2026/daily/2026-04-25.md");
    let content = fs::read_to_string(&daily).expect("daily note exists");
    assert!(content.contains("- **14:30**: first entry"));
    assert!(content.contains("- **14:45**: second entry"));
    assert!(
        content.find("first entry").unwrap() < content.find("second entry").unwrap(),
        "entries must appear in chronological order"
    );
}

#[test]
fn log_errors_when_target_is_not_a_vault() {
    let dir = tempdir().unwrap();
    // No init: `.cuaderno/` is missing.

    let err =
        log::run(dir.path(), moment(), "x").expect_err("log without an inited vault should fail");
    let msg = format!("{err}");
    assert!(msg.contains("no Cuaderno vault"), "unexpected error: {msg}");
}
