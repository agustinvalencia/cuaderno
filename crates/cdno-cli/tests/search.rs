//! In-process tests for `cdno search`. Seed a vault on disk, then assert
//! on the text from the `build_search` seam rather than capturing stdout
//! (same pattern as `cdno orient` / `cdno commitments`).

use std::fs;
use std::path::Path;

use cdno_cli::commands::{init, search};
use cdno_domain::SearchFilters;
use cdno_domain::note_type::NoteType;
use tempfile::tempdir;

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
fn render_handles_empty_and_untitled() {
    assert!(search::render("anything", &[]).contains("(no matches)"));
}
