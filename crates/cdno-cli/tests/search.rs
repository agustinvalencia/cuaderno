//! In-process tests for `cdno search`. Seed a vault on disk, then assert
//! on the text from the `build_search` seam rather than capturing stdout
//! (same pattern as `cdno orient` / `cdno commitments`).

use std::fs;
use std::path::Path;

use cdno_cli::commands::{init, search};
use cdno_core::path::VaultPath;
use cdno_domain::note_type::NoteType;
use cdno_domain::{SearchFilters, SearchResultEntry};
use chrono::NaiveDate;
use tempfile::tempdir;

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

const PROJECT_ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nWorking on the sparse attention kernel.\n";

const DAILY: &str = "---\ntype: daily\ncreated: 2026-05-15\n---\n\n# 2026-05-15\n\nDebugged the sparse attention kernel today.\n";

fn seed(root: &Path) {
    init::run(root).expect("init");
    fs::write(root.join("projects/alpha.md"), PROJECT_ALPHA).unwrap();
    let daily_dir = root.join("journal/2026/daily");
    fs::create_dir_all(&daily_dir).unwrap();
    fs::write(daily_dir.join("2026-05-15.md"), DAILY).unwrap();
}

#[test]
fn search_renders_ranked_matches() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let out = search::build_search(
        dir.path(),
        "sparse attention",
        &SearchFilters::default(),
        20,
    )
    .expect("search builds");

    // Both notes mention the phrase, so both surface.
    assert!(out.contains("projects/alpha.md"), "output:\n{out}");
    assert!(
        out.contains("journal/2026/daily/2026-05-15.md"),
        "output:\n{out}"
    );
    // The matched term is bracketed in the snippet.
    assert!(
        out.contains("[sparse]") || out.contains("[attention]"),
        "output:\n{out}"
    );
}

#[test]
fn search_respects_the_note_type_filter() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let filters = SearchFilters {
        note_types: vec![NoteType::Project],
        ..Default::default()
    };
    let out =
        search::build_search(dir.path(), "sparse attention", &filters, 20).expect("search builds");

    assert!(out.contains("projects/alpha.md"), "output:\n{out}");
    assert!(
        !out.contains("journal/2026/daily/2026-05-15.md"),
        "daily note should be filtered out:\n{out}"
    );
}

#[test]
fn search_reports_no_matches() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let out = search::build_search(
        dir.path(),
        "nonexistent term",
        &SearchFilters::default(),
        20,
    )
    .expect("search builds");
    assert!(out.contains("(no matches)"), "output:\n{out}");
}

#[test]
fn search_date_window_excludes_out_of_range_notes() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    // The daily note is dated 2026-05-15; the project map is undated, so a
    // date bound drops both the out-of-window daily and the undated project.
    let filters = SearchFilters {
        date_from: Some(date(2026, 6, 1)),
        ..Default::default()
    };
    let out =
        search::build_search(dir.path(), "sparse attention", &filters, 20).expect("search builds");
    assert!(out.contains("(no matches)"), "output:\n{out}");

    // Widen the window to include the daily note.
    let filters = SearchFilters {
        date_from: Some(date(2026, 5, 1)),
        date_to: Some(date(2026, 5, 31)),
        ..Default::default()
    };
    let out =
        search::build_search(dir.path(), "sparse attention", &filters, 20).expect("search builds");
    assert!(
        out.contains("journal/2026/daily/2026-05-15.md"),
        "output:\n{out}"
    );
    assert!(
        !out.contains("projects/alpha.md"),
        "undated note excluded:\n{out}"
    );
}

#[test]
fn search_limit_caps_the_number_of_hits() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    // Both notes match "sparse attention"; limit 1 returns only the top one.
    let out = search::build_search(dir.path(), "sparse attention", &SearchFilters::default(), 1)
        .expect("search builds");
    // Exactly one hit: precisely one of the two matching notes appears.
    let alpha = out.contains("projects/alpha.md");
    let daily = out.contains("journal/2026/daily/2026-05-15.md");
    assert!(
        alpha ^ daily,
        "limit 1 should return exactly one hit:\n{out}"
    );
}

#[test]
fn render_uses_a_placeholder_for_an_untitled_hit() {
    let hit = SearchResultEntry {
        path: VaultPath::new("inbox/x.md").unwrap(),
        note_type: "inbox".to_owned(),
        title: None,
        snippet: "some [body] text".to_owned(),
        score: -1.0,
    };
    let out = search::render("body", std::slice::from_ref(&hit));
    assert!(out.contains("(untitled)"), "output:\n{out}");
    assert!(out.contains("inbox/x.md"), "output:\n{out}");
}

#[test]
fn render_reports_no_matches_for_empty_results() {
    assert!(search::render("anything", &[]).contains("(no matches)"));
}
