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

// ---------------------------------------------------------------------
// Attachment-artefact ownership (#451). A markdown file inside a folder
// owned by an evidence stub is a filed document, not a note, so
// reconciliation must not try to index it and lint must not call its
// folder an orphan. Both sides resolve ownership through this helper.
// ---------------------------------------------------------------------

use cdno_core::path::VaultPath;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// Resolve against a fixed set of stub paths, as reconciliation does
/// against the filesystem's markdown set.
fn owner(path: &str, stubs: &[&str]) -> Option<String> {
    paths::owning_artefact_stub(&vp(path), |stub| stubs.iter().any(|s| vp(s) == *stub))
        .map(|s| s.to_string())
}

#[test]
fn artefact_beside_its_stub_is_owned() {
    assert_eq!(
        owner(
            "portfolios/demo/2026-06-13-paper/paper.pdf",
            &["portfolios/demo/2026-06-13-paper.md"],
        ),
        Some("portfolios/demo/2026-06-13-paper.md".to_string()),
    );
}

#[test]
fn markdown_artefact_is_owned_just_like_any_other_file() {
    // The whole point of #451: filing a `.md` document produces the same
    // stub-plus-folder pair as filing a PDF, and the artefact has no
    // frontmatter, so indexing it can only ever fail.
    assert_eq!(
        owner(
            "portfolios/demo/2026-07-03-review-panel/02-reviewer-b.md",
            &["portfolios/demo/2026-07-03-review-panel.md"],
        ),
        Some("portfolios/demo/2026-07-03-review-panel.md".to_string()),
    );
}

#[test]
fn evidence_note_at_the_portfolio_root_is_not_an_artefact() {
    assert_eq!(
        owner(
            "portfolios/demo/2026-06-13-paper.md",
            &["portfolios/demo/2026-06-13-paper.md", "portfolios.md"],
        ),
        None,
    );
}

#[test]
fn portfolio_index_is_not_an_artefact() {
    assert_eq!(
        owner("portfolios/demo/_index.md", &["portfolios/demo.md"]),
        None
    );
}

#[test]
fn folder_without_a_stub_owns_nothing() {
    assert_eq!(owner("portfolios/demo/assets/pasted.png", &[]), None);
}

#[test]
fn ownership_survives_an_intervening_grouping_folder() {
    // Depth-independence, the constraint from #454: a portfolio that
    // grows grouping subfolders must not need this rule rewritten.
    assert_eq!(
        owner(
            "portfolios/demo/sweep/2026-06-13-run-07/metrics.json",
            &["portfolios/demo/sweep/2026-06-13-run-07.md"],
        ),
        Some("portfolios/demo/sweep/2026-06-13-run-07.md".to_string()),
    );
}

#[test]
fn ownership_reaches_through_nesting_inside_the_artefact_folder() {
    // A filed directory tree keeps its internal structure; every file in
    // it resolves to the same owning stub as its siblings.
    assert_eq!(
        owner(
            "portfolios/demo/2026-06-13-bundle/src/deep/main.rs",
            &["portfolios/demo/2026-06-13-bundle.md"],
        ),
        Some("portfolios/demo/2026-06-13-bundle.md".to_string()),
    );
}

#[test]
fn nearest_owning_ancestor_wins() {
    // Both an inner and an outer candidate stub exist; the inner one is
    // the owner, so the artefact is attributed to the closest stub.
    assert_eq!(
        owner(
            "portfolios/demo/outer/inner/file.txt",
            &["portfolios/demo/outer.md", "portfolios/demo/outer/inner.md"],
        ),
        Some("portfolios/demo/outer/inner.md".to_string()),
    );
}

#[test]
fn a_dotted_folder_name_pairs_with_the_right_stub() {
    // `with_extension` would rewrite `run-v1.2` to the stub `run-v1.md`
    // and pair the artefact with an unrelated note; the stub name is
    // built by appending, not replacing.
    assert_eq!(
        owner(
            "portfolios/demo/run-v1.2/out.log",
            &["portfolios/demo/run-v1.md"],
        ),
        None,
    );
    assert_eq!(
        owner(
            "portfolios/demo/run-v1.2/out.log",
            &["portfolios/demo/run-v1.2.md"],
        ),
        Some("portfolios/demo/run-v1.2.md".to_string()),
    );
}

#[test]
fn the_rule_is_confined_to_portfolios() {
    // An expanded stewardship is `stewardships/<slug>/` and a flat one is
    // `stewardships/<slug>.md`. If the pairing applied vault-wide, a
    // vault holding both spellings would see the expanded folder's notes
    // silently vanish from the index.
    assert_eq!(
        owner("stewardships/health/_index.md", &["stewardships/health.md"],),
        None,
    );
    assert_eq!(
        owner("projects/alpha/notes.md", &["projects/alpha.md"]),
        None,
    );
}
