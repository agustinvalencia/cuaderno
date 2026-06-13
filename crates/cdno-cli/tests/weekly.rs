//! In-process tests for `cdno weekly` — seed a vault on disk, then
//! assert on the text returned by `build_weekly` (rather than capturing
//! stdout from `run`). Mirrors `tests/orient.rs`.

use std::fs;
use std::path::Path;

use cdno_cli::commands::{init, weekly};
use chrono::NaiveDate;
use tempfile::tempdir;

/// A Wednesday in ISO week 2026-W18.
fn day_in_w18() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 4, 29).unwrap()
}

const WEEKLY_NOTE: &str = "---\ntype: weekly\nweek: 2026-W18\ndate_start: 2026-04-27\ndate_end: 2026-05-03\n---\n\n# Week 18, 2026\n\n## Wins\n- Shipped the release.\n\n## Challenges\n- Wednesday was low energy.\n\n## One Improvement\n- Block Tuesday morning.\n\n## Next Week's Focus\nDraft the methods section.\n";

fn seed_weekly_note(root: &Path) {
    init::run(root).expect("init");
    let path = root.join("journal/2026/weekly/2026-W18.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, WEEKLY_NOTE).unwrap();
}

#[test]
fn weekly_shows_a_placeholder_when_the_week_has_no_note() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    let out = weekly::build_weekly(dir.path(), day_in_w18(), None).expect("build");

    assert!(out.contains("No weekly note for 2026-W18"), "out:\n{out}");
}

#[test]
fn weekly_renders_the_note_body_without_frontmatter() {
    let dir = tempdir().unwrap();
    seed_weekly_note(dir.path());

    let out = weekly::build_weekly(dir.path(), day_in_w18(), None).expect("build");

    // YAML frontmatter is stripped; the body starts at the H1.
    assert!(!out.contains("type: weekly"), "frontmatter leaked:\n{out}");
    assert!(
        out.starts_with("# Week 18, 2026"),
        "body starts at the H1:\n{out}"
    );
    // The sections and their content render.
    assert!(
        out.contains("## Next Week's Focus\nDraft the methods section."),
        "focus section:\n{out}"
    );
    assert!(
        out.contains("## Wins\n- Shipped the release."),
        "wins:\n{out}"
    );
}

#[test]
fn weekly_date_flag_selects_the_target_week() {
    let dir = tempdir().unwrap();
    seed_weekly_note(dir.path());

    // `today` is outside W18, but an explicit `--date` in W18 resolves to
    // the seeded note (the note is keyed by ISO week).
    let today_outside = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
    let out = weekly::build_weekly(dir.path(), today_outside, Some(day_in_w18())).expect("build");

    assert!(
        out.contains("# Week 18, 2026"),
        "--date selects W18:\n{out}"
    );
}
