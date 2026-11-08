use chrono::{Datelike, NaiveDate};

use cdno_core::path::VaultPath;
use cdno_core::paths::{self, NoteFormat, note_address, note_candidates, note_format};
use proptest::prelude::*;

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
fn journal_monthly_dir_uses_calendar_year() {
    assert_eq!(paths::journal_monthly_dir(2026), "journal/2026/monthly");
}

#[test]
fn monthly_note_relpath_uses_calendar_year_and_year_month_stem() {
    let date = NaiveDate::from_ymd_opt(2026, 7, 15).unwrap();
    assert_eq!(
        paths::monthly_note_relpath(date),
        "journal/2026/monthly/2026-07.md",
    );
}

#[test]
fn monthly_note_relpath_is_keyed_by_month_not_day() {
    // Any day in July 2026 resolves to the same monthly note.
    let first = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
    let last = NaiveDate::from_ymd_opt(2026, 7, 31).unwrap();
    assert_eq!(
        paths::monthly_note_relpath(first),
        paths::monthly_note_relpath(last),
    );
}

#[test]
fn monthly_note_relpath_pads_single_digit_months() {
    let date = NaiveDate::from_ymd_opt(2026, 3, 9).unwrap();
    assert_eq!(
        paths::monthly_note_relpath(date),
        "journal/2026/monthly/2026-03.md",
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
fn projects_done_dir_is_year_partitioned() {
    assert_eq!(paths::projects_done_dir(2026), "projects/_done/2026");
}

#[test]
fn init_dirs_includes_year_partitions_and_static_folders() {
    let today = NaiveDate::from_ymd_opt(2026, 4, 25).unwrap();
    let dirs = paths::init_dirs(today);

    assert!(dirs.contains(&"journal/2026/daily".to_string()));
    assert!(dirs.contains(&"journal/2026/weekly".to_string()));
    assert!(dirs.contains(&"journal/2026/monthly".to_string()));
    assert!(dirs.contains(&"commitments/_done/2026".to_string()));
    assert!(dirs.contains(&paths::ACTIONS.to_string()));
    assert!(dirs.contains(&"actions/_done/2026".to_string()));
    assert!(dirs.contains(&paths::PROJECTS.to_string()));
    assert!(dirs.contains(&paths::INBOX.to_string()));
    assert!(dirs.contains(&paths::CUADERNO_DIR.to_string()));
}

#[test]
fn init_dirs_includes_projects_done_for_the_current_year() {
    let today = NaiveDate::from_ymd_opt(2026, 4, 25).unwrap();
    let dirs = paths::init_dirs(today);

    let projects_done = paths::projects_done_dir(today.year());
    assert_eq!(
        dirs.iter().filter(|d| **d == projects_done).count(),
        1,
        "init_dirs must list {projects_done} exactly once: {dirs:?}"
    );
}

fn vp(s: &str) -> VaultPath {
    VaultPath::new(s).unwrap()
}

#[test]
fn note_format_reads_the_extension_case_insensitively() {
    assert_eq!(note_format(&vp("a.md")), Some(NoteFormat::Markdown));
    assert_eq!(note_format(&vp("a.MD")), Some(NoteFormat::Markdown));
    assert_eq!(note_format(&vp("a.typ")), Some(NoteFormat::Typst));
    assert_eq!(note_format(&vp("a.TYP")), Some(NoteFormat::Typst));
    assert_eq!(note_format(&vp("a.pdf")), None);
    assert_eq!(note_format(&vp("a")), None);
    // `VaultPath` normalises a trailing slash away, so `a.md/` reaches
    // `note_format` as `a.md`; the directory case it stands for is a path
    // whose last component is not a file name ending in the extension.
    assert_eq!(note_format(&vp("a.md/b")), None);
    assert_eq!(note_format(&vp(".md")), None);
}

#[test]
fn note_address_strips_only_the_note_extension() {
    assert_eq!(note_address(&vp("run-v1.2.md")), "run-v1.2");
    assert_eq!(note_address(&vp("x.typ")), "x");
    assert_eq!(note_address(&vp("x.TYP")), "x");
    assert_eq!(note_address(&vp("x.pdf")), "x.pdf");
    assert_eq!(note_address(&vp("p/_index.md")), "p/_index");
    assert_eq!(note_address(&vp("p/noext")), "p/noext");
}

#[test]
fn note_candidates_lists_both_formats_then_the_folder_index() {
    let got: Vec<String> = note_candidates("portfolios/surrogate-model")
        .iter()
        .map(|p| p.to_string())
        .collect();
    assert_eq!(
        got,
        [
            "portfolios/surrogate-model.md",
            "portfolios/surrogate-model.typ",
            "portfolios/surrogate-model/_index.md",
        ]
    );
}

proptest! {
    #[test]
    fn note_address_then_candidates_round_trips(
        stem in "[a-z][a-z0-9-]{0,12}(\\.[a-z0-9]{1,3})?",
        dir in "([a-z]{1,8}/){0,3}",
        ext in prop::sample::select(vec!["md", "typ", "MD"]),
    ) {
        let path = vp(&format!("{dir}{stem}.{ext}"));
        let candidates = note_candidates(&note_address(&path));
        if ext.eq_ignore_ascii_case("md") && ext != "md" {
            // Candidates are lower-case; a shouting extension is the same note
            // format, not the same path.
            let lowered = vp(&format!("{}{}.md", dir, stem));
            prop_assert!(candidates.contains(&lowered));
        } else {
            prop_assert!(candidates.contains(&path));
        }
    }
}
