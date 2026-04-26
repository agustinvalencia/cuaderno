//! In-process tests for `commands::capture::run`.

use std::fs;

use cdno_cli::commands::{capture, init};
use chrono::{NaiveDate, NaiveTime};
use tempfile::tempdir;

fn moment() -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 4, 26)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap())
}

#[test]
fn capture_writes_an_inbox_note_with_the_expected_filename() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    capture::run(dir.path(), moment(), "Read the new design doc").expect("capture");

    let expected = dir
        .path()
        .join("inbox/2026-04-26-read-the-new-design-doc.md");
    assert!(expected.is_file(), "missing: {}", expected.display());
    let content = fs::read_to_string(&expected).unwrap();
    assert!(content.contains("type: inbox"));
    assert!(content.contains("Read the new design doc"));
}

#[test]
fn capture_errors_when_target_is_not_a_vault() {
    let dir = tempdir().unwrap();

    let err =
        capture::run(dir.path(), moment(), "x").expect_err("capture without a vault must fail");
    let msg = format!("{err}");
    assert!(msg.contains("no Cuaderno vault"), "unexpected error: {msg}");
}
