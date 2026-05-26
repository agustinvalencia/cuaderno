use chrono::NaiveDate;

use cdno_core::paths;

#[test]
fn journal_daily_dir_uses_calendar_year() {
    assert_eq!(paths::journal_daily_dir(2026), "journal/2026/daily");
}

#[test]
fn daily_note_relpath_combines_year_folder_and_iso_date() {
    let date = NaiveDate::from_ymd_opt(2026, 4, 25).unwrap();
    assert_eq!(
        paths::daily_note_relpath(date),
        "journal/2026/daily/2026-04-25.md",
    );
}

#[test]
fn weekly_note_relpath_uses_iso_year_for_iso_week_one() {
    // Mon 29 Dec 2025 is the start of ISO week 1 of 2026. The folder
    // year and the filename year must agree on the ISO week year, not
    // the calendar year, otherwise the W01 file would land in the
    // 2025 folder and confuse readers.
    let date = NaiveDate::from_ymd_opt(2025, 12, 29).unwrap();
    assert_eq!(
        paths::weekly_note_relpath(date),
        "journal/2026/weekly/2026-W01.md",
    );
}

#[test]
fn weekly_note_relpath_pads_single_digit_weeks() {
    let date = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
    assert_eq!(
        paths::weekly_note_relpath(date),
        "journal/2026/weekly/2026-W02.md",
    );
}

#[test]
fn commitments_done_dir_partitions_by_year() {
    assert_eq!(paths::commitments_done_dir(2026), "commitments/_done/2026");
}

#[test]
fn actions_done_dir_partitions_by_year() {
    assert_eq!(paths::actions_done_dir(2026), "actions/_done/2026");
}

#[test]
fn init_dirs_includes_year_partitions_and_static_folders() {
    let today = NaiveDate::from_ymd_opt(2026, 4, 25).unwrap();
    let dirs = paths::init_dirs(today);

    assert!(dirs.contains(&"journal/2026/daily".to_string()));
    assert!(dirs.contains(&"journal/2026/weekly".to_string()));
    assert!(dirs.contains(&"commitments/_done/2026".to_string()));
    assert!(dirs.contains(&paths::ACTIONS.to_string()));
    assert!(dirs.contains(&"actions/_done/2026".to_string()));
    assert!(dirs.contains(&paths::PROJECTS.to_string()));
    assert!(dirs.contains(&paths::INBOX.to_string()));
    assert!(dirs.contains(&paths::CUADERNO_DIR.to_string()));
}
