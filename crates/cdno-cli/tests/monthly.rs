//! In-process tests for `cdno monthly` — seed a vault on disk, then
//! assert on the text returned by `build_monthly` (rather than capturing
//! stdout from `run`). Mirrors `tests/weekly.rs`.

use std::fs;
use std::path::Path;

use cdno_cli::commands::{init, monthly};
use chrono::NaiveDate;
use tempfile::tempdir;

/// A day in July 2026.
fn day_in_july() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()
}

const MONTHLY_NOTE: &str = "---\ntype: monthly\nmonth: 2026-07\ndate_start: 2026-07-01\ndate_end: 2026-07-31\n---\n\n# July 2026\n\n## Wins\n- Shipped the release.\n\n## Themes\n- Momentum on the paper.\n\n## Next Month's Focus\nDraft the discussion section.\n\n## Weeks\n- [[journal/2026/weekly/2026-W28]]\n- [[journal/2026/weekly/2026-W29]]\n- [[journal/2026/weekly/2026-W30]]\n- [[journal/2026/weekly/2026-W31]]\n";

fn seed_monthly_note(root: &Path) {
    init::run(root).expect("init");
    let path = root.join("journal/2026/monthly/2026-07.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, MONTHLY_NOTE).unwrap();
}

#[test]
fn monthly_shows_a_placeholder_when_the_month_has_no_note() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    let out = monthly::build_monthly(dir.path(), day_in_july(), None).expect("build");

    assert!(out.contains("No monthly note for 2026-07"), "out:\n{out}");
}

#[test]
fn monthly_renders_the_note_body_without_frontmatter() {
    let dir = tempdir().unwrap();
    seed_monthly_note(dir.path());

    let out = monthly::build_monthly(dir.path(), day_in_july(), None).expect("build");

    // YAML frontmatter is stripped; the body starts at the H1.
    assert!(!out.contains("type: monthly"), "frontmatter leaked:\n{out}");
    assert!(
        out.starts_with("# July 2026"),
        "body starts at the H1:\n{out}"
    );
    // The review sections and their content render.
    assert!(
        out.contains("## Next Month's Focus\nDraft the discussion section."),
        "focus section:\n{out}"
    );
    assert!(
        out.contains("## Wins\n- Shipped the release."),
        "wins:\n{out}"
    );
    // The linked weeks render (links, not copies).
    assert!(
        out.contains("- [[journal/2026/weekly/2026-W28]]"),
        "weeks block:\n{out}"
    );
}
