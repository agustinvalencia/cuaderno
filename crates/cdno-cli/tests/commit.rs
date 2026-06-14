//! In-process tests for `cdno commit` / `cdno commitments`. Mirrors
//! `tests/orient.rs` — seed via the command layer, then read files or
//! assert on the rendered text returned by `build_commitments`.

use std::path::Path;

use cdno_cli::commands::commit::CommitCommands;
use cdno_cli::commands::{commit, commitments, init};
use cdno_domain::frontmatter::Context;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    ymd(year, month, day).and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

fn init_vault(root: &Path) {
    init::run(root).expect("init");
}

#[test]
fn commit_create_writes_active_commitment_file() {
    let dir = tempfile::tempdir().unwrap();
    init_vault(dir.path());

    commit::run(
        dir.path(),
        dt(2026, 5, 28, 9, 0),
        CommitCommands::Create {
            title: Some("Pay rent".to_owned()),
            due: Some(ymd(2026, 6, 1)),
            context: Some(Context::Personal),
            project: None,
            stewardship: None,
        },
        true,
    )
    .expect("commit create");

    let raw = std::fs::read_to_string(dir.path().join("commitments/pay-rent.md"))
        .expect("commitment file exists");
    assert!(raw.contains("status: active"), "frontmatter:\n{raw}");
    assert!(raw.contains("due: 2026-06-01"), "frontmatter:\n{raw}");
    assert!(raw.contains("context: personal"), "frontmatter:\n{raw}");
    // No origin links supplied → both null.
    assert!(raw.contains("project: null"), "frontmatter:\n{raw}");
    assert!(raw.contains("stewardship: null"), "frontmatter:\n{raw}");
}

#[test]
fn commit_create_writes_origin_link_slugs_from_flags() {
    let dir = tempfile::tempdir().unwrap();
    init_vault(dir.path());

    commit::run(
        dir.path(),
        dt(2026, 5, 28, 9, 0),
        CommitCommands::Create {
            title: Some("Email ophthalmologist".to_owned()),
            due: Some(ymd(2026, 6, 15)),
            context: Some(Context::Personal),
            project: None,
            stewardship: Some("health".to_owned()),
        },
        true,
    )
    .expect("commit create");

    let raw = std::fs::read_to_string(dir.path().join("commitments/email-ophthalmologist.md"))
        .expect("commitment file exists");
    assert!(raw.contains("stewardship: health"), "frontmatter:\n{raw}");
    assert!(raw.contains("project: null"), "frontmatter:\n{raw}");
}

#[test]
fn commit_done_moves_file_to_year_subfolder_with_completed_stamp() {
    let dir = tempfile::tempdir().unwrap();
    init_vault(dir.path());

    commit::run(
        dir.path(),
        dt(2026, 5, 28, 9, 0),
        CommitCommands::Create {
            title: Some("Pay rent".to_owned()),
            due: Some(ymd(2026, 6, 1)),
            context: Some(Context::Personal),
            project: None,
            stewardship: None,
        },
        true,
    )
    .unwrap();

    commit::run(
        dir.path(),
        dt(2026, 5, 29, 14, 0),
        CommitCommands::Done {
            slug: Some("pay-rent".to_owned()),
        },
        true,
    )
    .expect("commit done");

    assert!(
        !dir.path().join("commitments/pay-rent.md").exists(),
        "active path emptied"
    );
    let done = dir.path().join("commitments/_done/2026/pay-rent.md");
    let raw = std::fs::read_to_string(&done).expect("done file exists");
    assert!(raw.contains("status: completed"), "frontmatter:\n{raw}");
    assert!(raw.contains("completed: 2026-05-29"), "frontmatter:\n{raw}");
}

#[test]
fn commitments_lists_active_in_window() {
    let dir = tempfile::tempdir().unwrap();
    init_vault(dir.path());

    // One inside the 2-week window, one beyond it.
    commit::run(
        dir.path(),
        dt(2026, 5, 28, 9, 0),
        CommitCommands::Create {
            title: Some("Pay rent".to_owned()),
            due: Some(ymd(2026, 6, 1)),
            context: Some(Context::Personal),
            project: None,
            stewardship: None,
        },
        true,
    )
    .unwrap();
    commit::run(
        dir.path(),
        dt(2026, 5, 28, 9, 5),
        CommitCommands::Create {
            title: Some("Book dentist".to_owned()),
            due: Some(ymd(2026, 8, 1)),
            context: Some(Context::Personal),
            project: None,
            stewardship: None,
        },
        true,
    )
    .unwrap();

    let out = commitments::build_commitments(dir.path(), ymd(2026, 5, 28), 2).expect("list");
    assert!(out.contains("Commitments (next 2 weeks"), "header:\n{out}");
    assert!(out.contains("Pay rent"), "near commitment:\n{out}");
    assert!(
        !out.contains("Book dentist"),
        "far commitment excluded:\n{out}"
    );
    // The source is its own table column now, not parenthesised inline.
    assert!(out.contains("commitment"), "source label:\n{out}");
}

#[test]
fn commitments_weeks_flag_widens_the_window() {
    let dir = tempfile::tempdir().unwrap();
    init_vault(dir.path());

    commit::run(
        dir.path(),
        dt(2026, 5, 28, 9, 0),
        CommitCommands::Create {
            title: Some("Book dentist".to_owned()),
            due: Some(ymd(2026, 8, 1)),
            context: Some(Context::Personal),
            project: None,
            stewardship: None,
        },
        true,
    )
    .unwrap();

    // 12 weeks covers 2026-08-01 from 2026-05-28.
    let out = commitments::build_commitments(dir.path(), ymd(2026, 5, 28), 12).expect("list");
    assert!(out.contains("Commitments (next 12 weeks"), "header:\n{out}");
    assert!(
        out.contains("Book dentist"),
        "wider window includes it:\n{out}"
    );
}

#[test]
fn commitments_on_empty_vault_shows_nothing_due() {
    let dir = tempfile::tempdir().unwrap();
    init_vault(dir.path());

    let out = commitments::build_commitments(dir.path(), ymd(2026, 5, 28), 2).expect("list");
    assert!(out.contains("(nothing due)"), "empty:\n{out}");
}

// ---------------------------------------------------------------------
// Non-interactive ergonomics for the retrofitted verbs (#114).
// ---------------------------------------------------------------------

#[test]
fn commit_create_in_non_interactive_errors_when_missing_due() {
    let dir = tempfile::tempdir().unwrap();
    init_vault(dir.path());
    let err = commit::run(
        dir.path(),
        dt(2026, 5, 28, 9, 0),
        CommitCommands::Create {
            title: Some("Pay rent".to_owned()),
            due: None,
            context: Some(Context::Personal),
            project: None,
            stewardship: None,
        },
        true,
    )
    .expect_err("missing --due should error in non-interactive mode");
    let msg = format!("{err:#}");
    assert!(msg.contains("--due"), "error message: {msg}");
}

#[test]
fn commit_done_in_non_interactive_errors_when_missing_slug() {
    let dir = tempfile::tempdir().unwrap();
    init_vault(dir.path());
    let err = commit::run(
        dir.path(),
        dt(2026, 5, 29, 14, 0),
        CommitCommands::Done { slug: None },
        true,
    )
    .expect_err("missing --slug should error in non-interactive mode");
    let msg = format!("{err:#}");
    assert!(msg.contains("--slug"), "error message: {msg}");
}
