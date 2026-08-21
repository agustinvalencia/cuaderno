//! In-process tests for `cdno open`. Seed a vault on disk, then assert on the
//! `resolve` / `render_list` seams rather than capturing stdout (the pattern
//! `cdno search` uses).
//!
//! Wiring only — the reference grammar itself is covered in
//! `cdno-domain`'s `note_ref_tests`.

use std::fs;
use std::path::Path;

use cdno_cli::bootstrap;
use cdno_cli::commands::{init, open};
use chrono::NaiveDate;
use tempfile::tempdir;

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

const PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nFitting the emulator.\n";
const DAILY: &str =
    "---\ntype: daily\ncreated: 2026-08-21\n---\n\n# Friday, 21 August 2026\n\nRan the sweep.\n";

fn seed(root: &Path) {
    init::run(root).expect("init");
    fs::write(root.join("projects/surrogate-model.md"), PROJECT).unwrap();
    let daily_dir = root.join("journal/2026/daily");
    fs::create_dir_all(&daily_dir).unwrap();
    fs::write(daily_dir.join("2026-08-21.md"), DAILY).unwrap();
}

/// A stewardship and a portfolio that share one slug. The vault's
/// globally-unique-stem rule does not span these two, because the stems are
/// `gym` and `_index` — so this ambiguity is reachable through ordinary use,
/// not just hand-editing.
fn seed_colliding_slugs(root: &Path) {
    fs::write(
        root.join("stewardships/gym.md"),
        "---\ntype: stewardship\ncontext: personal\ncreated: 2026-04-01\n---\n\n# Gym\n",
    )
    .unwrap();
    let dir = root.join("portfolios/gym");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("_index.md"),
        "---\ntype: portfolio\ncreated: 2026-04-01\n---\n\n# Gym\n",
    )
    .unwrap();
}

fn resolve_in(root: &Path, reference: &str) -> anyhow::Result<String> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    open::resolve(&vault, reference, date(2026, 8, 21))
        .map(|p| p.as_path().to_string_lossy().into_owned())
}

#[test]
fn resolves_a_bare_slug_a_typed_ref_and_a_calendar_word() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    assert_eq!(
        resolve_in(dir.path(), "surrogate-model").unwrap(),
        "projects/surrogate-model.md"
    );
    assert_eq!(
        resolve_in(dir.path(), "project:surrogate-model").unwrap(),
        "projects/surrogate-model.md"
    );
    assert_eq!(
        resolve_in(dir.path(), "today").unwrap(),
        "journal/2026/daily/2026-08-21.md"
    );
}

/// The near-miss hint is what replaces `available_slugs_hint` here: `open`
/// does not know the type, so listing every slug of one type is not an
/// option, and listing the whole vault would be noise.
#[test]
fn a_near_miss_names_what_you_probably_meant() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let err = resolve_in(dir.path(), "surro").unwrap_err().to_string();

    assert!(
        err.contains("project:surrogate-model"),
        "hint should name the typed ref, got: {err}"
    );
}

#[test]
fn a_reference_matching_nothing_points_at_the_listing() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let err = resolve_in(dir.path(), "zzzzqqqq").unwrap_err().to_string();

    assert!(err.contains("--list"), "got: {err}");
}

#[test]
fn an_ambiguous_slug_names_both_typed_forms() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    seed_colliding_slugs(dir.path());

    let err = resolve_in(dir.path(), "gym").unwrap_err().to_string();

    assert!(err.contains("ambiguous"), "got: {err}");
    assert!(err.contains("portfolio:gym"), "got: {err}");
    assert!(err.contains("stewardship:gym"), "got: {err}");
}

#[test]
fn a_typed_reference_resolves_an_ambiguous_slug() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    seed_colliding_slugs(dir.path());

    assert_eq!(
        resolve_in(dir.path(), "portfolio:gym").unwrap(),
        "portfolios/gym/_index.md"
    );
    assert_eq!(
        resolve_in(dir.path(), "stewardship:gym").unwrap(),
        "stewardships/gym.md"
    );
}

/// A path names a file outright, so a typo must be a miss rather than a
/// fuzzy hit on something adjacent.
#[test]
fn a_typod_path_does_not_fall_through_to_the_fuzzy_tier() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let err = resolve_in(dir.path(), "projects/surrogate-modle.md")
        .unwrap_err()
        .to_string();

    assert!(!err.contains("did you mean"), "got: {err}");
}

#[test]
fn render_list_is_tab_separated_with_the_h1_as_the_title() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    let (vault, _report) = bootstrap::open_vault(dir.path()).unwrap();

    let listing = open::render_list(&vault.list_note_candidates().unwrap());

    let project_row = listing
        .lines()
        .find(|l| l.starts_with("projects/surrogate-model.md"))
        .expect("project row");
    assert_eq!(
        project_row,
        "projects/surrogate-model.md\tSurrogate model\tproject"
    );
}

/// The label falls back to the slug rather than rendering an empty column:
/// a note with no H1 is still openable and must stay pickable.
#[test]
fn a_note_without_an_h1_is_labelled_by_its_slug() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    fs::write(
        dir.path().join("projects/no-heading.md"),
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\nJust a body.\n",
    )
    .unwrap();
    let (vault, _report) = bootstrap::open_vault(dir.path()).unwrap();

    let listing = open::render_list(&vault.list_note_candidates().unwrap());

    let row = listing
        .lines()
        .find(|l| l.starts_with("projects/no-heading.md"))
        .expect("row");
    assert_eq!(row, "projects/no-heading.md\tno-heading\tproject");
}

// ---------------------------------------------------------------------
// strip_vault_root — what makes the fzf round-trip work
// ---------------------------------------------------------------------

#[test]
fn an_absolute_path_under_the_vault_becomes_vault_relative() {
    let dir = tempdir().unwrap();
    let absolute = dir.path().join("projects/surrogate-model.md");

    assert_eq!(
        open::strip_vault_root(&absolute.to_string_lossy(), dir.path()),
        "projects/surrogate-model.md"
    );
}

#[test]
fn strip_vault_root_leaves_a_relative_reference_alone() {
    let dir = tempdir().unwrap();

    assert_eq!(open::strip_vault_root("today", dir.path()), "today");
    assert_eq!(
        open::strip_vault_root("project:surrogate-model", dir.path()),
        "project:surrogate-model"
    );
}

/// An absolute path outside the vault is passed through untouched, so it
/// fails as a plain not-found rather than being silently reinterpreted as a
/// slug relative to the vault.
#[test]
fn strip_vault_root_leaves_a_path_outside_the_vault_alone() {
    let dir = tempdir().unwrap();

    assert_eq!(
        open::strip_vault_root("/elsewhere/notes/foo.md", dir.path()),
        "/elsewhere/notes/foo.md"
    );
}
