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
    open::resolve(&vault, reference, date(2026, 8, 21), false)
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

// ---------------------------------------------------------------------
// Regressions from review
// ---------------------------------------------------------------------

/// The commonest reference there is, on a day not yet logged. Daily notes are
/// scaffolded lazily, so this happens constantly — and answering it with
/// "did you mean project:something-unrelated" is worse than useless.
#[test]
fn a_missing_daily_note_does_not_suggest_unrelated_notes() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    fs::write(dir.path().join("projects/surrogate-model.md"), PROJECT).unwrap();

    let err = resolve_in(dir.path(), "today").unwrap_err().to_string();

    assert!(
        !err.contains("did you mean"),
        "a named file that is absent must not offer near matches: {err}"
    );
    assert!(
        err.contains("cdno log"),
        "should say what creates one: {err}"
    );
}

/// ...and a missing *path* gets a different message again: nothing about the
/// journal, because a path is not a journal note.
#[test]
fn a_missing_path_does_not_mention_the_journal() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let err = resolve_in(dir.path(), "projects/nope.md")
        .unwrap_err()
        .to_string();

    assert!(!err.contains("cdno log"), "got: {err}");
    assert!(!err.contains("did you mean"), "got: {err}");
    assert!(err.contains("--list"), "got: {err}");
}

/// `exists()` is true for a directory, so without an explicit file check this
/// handed back a path no editor can open and no `$(…)` can use.
#[test]
fn a_directory_is_not_a_note() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    for reference in ["projects/", "journal/2026", "actions"] {
        let result = resolve_in(dir.path(), reference);
        assert!(
            result.is_err(),
            "`{reference}` is a directory and must not resolve, got {result:?}"
        );
    }
}

/// The typed form is exactly what the ambiguity error tells people to type,
/// so an incomplete one must still get a hint. Ranking the `project:` prefix
/// along with the slug used to sink the score below the match threshold, so
/// the form we recommend got the worst message.
///
/// Note the limit this does not claim to fix: matching is subsequence-based,
/// so a *transposition* (`surrogate-modle`) cannot match `surrogate-model` by
/// either name and gets the generic message. Truncations and omissions — far
/// and away the common case when someone half-remembers a slug — do match.
#[test]
fn an_incomplete_typed_reference_still_gets_a_hint() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let err = resolve_in(dir.path(), "project:surrogate-mod")
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("did you mean"),
        "typed refs deserve hints too: {err}"
    );
    assert!(err.contains("project:surrogate-model"), "got: {err}");
}

/// The slug is scored as well as the title, because the thing people mistype
/// is the slug — and a slug is not a subsequence of its own title once a
/// hyphen stands where a space does.
#[test]
fn a_partial_slug_matches_even_though_it_is_not_a_subsequence_of_the_title() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    let err = resolve_in(dir.path(), "surrogate-mod")
        .unwrap_err()
        .to_string();

    assert!(err.contains("project:surrogate-model"), "got: {err}");
}

/// A path outside the vault is a user error, not a layer error: the raw
/// `VaultPath` message ("path must be relative") is written for a programmer.
#[test]
fn a_path_outside_the_vault_is_a_readable_error() {
    let dir = tempdir().unwrap();
    seed(dir.path());

    for reference in ["/elsewhere/notes/foo.md", "../foo.md"] {
        let err = resolve_in(dir.path(), reference).unwrap_err().to_string();
        assert!(
            err.contains("outside this vault"),
            "unreadable error for {reference}: {err}"
        );
        assert!(
            !err.contains("must be relative"),
            "leaked a layer error for {reference}: {err}"
        );
    }
}

// ---------------------------------------------------------------------
// Second review round
// ---------------------------------------------------------------------

/// A flat and an expanded stewardship share both slug *and* type, so the
/// typed form cannot tell them apart. Offering it twice would hand the user
/// a suggestion that fails exactly as their input did.
#[test]
fn an_ambiguity_the_typed_form_cannot_resolve_names_paths_instead() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    fs::write(
        dir.path().join("stewardships/gym.md"),
        "---\ntype: stewardship\ncontext: personal\ncreated: 2026-04-01\n---\n\n# Gym\n",
    )
    .unwrap();
    let expanded = dir.path().join("stewardships/gym");
    fs::create_dir_all(&expanded).unwrap();
    fs::write(
        expanded.join("_index.md"),
        "---\ntype: stewardship\ncontext: personal\ncreated: 2026-04-01\n---\n\n# Gym\n",
    )
    .unwrap();

    let err = resolve_in(dir.path(), "gym").unwrap_err().to_string();

    assert!(
        !err.contains("stewardship:gym or stewardship:gym"),
        "every suggestion must be distinct: {err}"
    );
    assert!(err.contains("stewardships/gym.md"), "got: {err}");
    assert!(err.contains("stewardships/gym/_index.md"), "got: {err}");
    // And each suggestion must actually work.
    assert_eq!(
        resolve_in(dir.path(), "stewardships/gym.md").unwrap(),
        "stewardships/gym.md"
    );
}

/// The near-miss hint must not repeat a name either.
#[test]
fn the_near_miss_hint_does_not_repeat_a_suggestion() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    fs::write(
        dir.path().join("stewardships/gym.md"),
        "---\ntype: stewardship\ncontext: personal\ncreated: 2026-04-01\n---\n\n# Gym\n",
    )
    .unwrap();
    let expanded = dir.path().join("stewardships/gym");
    fs::create_dir_all(&expanded).unwrap();
    fs::write(
        expanded.join("_index.md"),
        "---\ntype: stewardship\ncontext: personal\ncreated: 2026-04-01\n---\n\n# Gym\n",
    )
    .unwrap();

    let err = resolve_in(dir.path(), "gy").unwrap_err().to_string();

    assert!(
        !err.contains("stewardship:gym, stewardship:gym"),
        "suggestions must be deduped: {err}"
    );
}

/// The picker filters on the rendered label and is seeded with what the user
/// typed — a slug. Without the slug in the label, the interactive fallback
/// would filter to nothing, making it worse than the piped path.
#[test]
fn a_picker_row_carries_the_slug_so_the_seeded_filter_can_match_it() {
    let dir = tempdir().unwrap();
    seed(dir.path());
    let (vault, _report) = bootstrap::open_vault(dir.path()).unwrap();
    let candidates = vault.list_note_candidates().unwrap();

    let row = candidates
        .iter()
        .map(open::picker_label)
        .find(|l| l.contains("Surrogate model"))
        .expect("project row");

    assert!(
        row.contains("surrogate-model"),
        "label must carry the slug: {row}"
    );
}

/// `current_dir` resolves symlinks but `--vault` does not, so the same vault
/// reached two ways spells its root differently — and the documented
/// `$(cdno open --path …)` round-trip would fail with "outside this vault".
#[test]
fn an_absolute_path_under_a_symlinked_root_still_becomes_relative() {
    let dir = tempdir().unwrap();
    let real = dir.path().join("real");
    fs::create_dir_all(real.join("projects")).unwrap();
    let link = dir.path().join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real, &link).unwrap();

    // The path is spelled through the real directory; the root through the
    // symlink. The textual comparison cannot match, so canonicalisation must.
    let spelled = real.join("projects/surrogate-model.md");
    fs::write(&spelled, "x").unwrap();

    assert_eq!(
        open::strip_vault_root(&spelled.to_string_lossy(), &link),
        "projects/surrogate-model.md"
    );
}
