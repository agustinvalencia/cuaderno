//! Tests for the weekly-note read + section writes
//! (`Vault::read_weekly_note`, `Vault::upsert_weekly_section`) and the
//! [`WeeklySection`] allowlist (design §5.2).

use std::str::FromStr;
use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::{Vault, WeeklySection};
use chrono::NaiveDate;

/// A Wednesday in ISO week 2026-W18 (Mon 2026-04-27 .. Sun 2026-05-03).
fn midweek() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 4, 29).unwrap()
}

fn make_vault() -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) = Vault::new(Arc::clone(&store), index, VaultConfig::default())
        .expect("Vault::new on empty store");
    (vault, store)
}

// --- WeeklySection parsing -------------------------------------------

#[test]
fn weekly_section_parses_case_and_punctuation_insensitively() {
    assert_eq!(
        WeeklySection::from_str("Wins").unwrap(),
        WeeklySection::Wins
    );
    assert_eq!(
        WeeklySection::from_str("one improvement").unwrap(),
        WeeklySection::OneImprovement
    );
    assert_eq!(
        WeeklySection::from_str("One-Improvement").unwrap(),
        WeeklySection::OneImprovement
    );
    assert_eq!(
        WeeklySection::from_str("This Week's Goal").unwrap(),
        WeeklySection::ThisWeeksGoal
    );
    assert_eq!(
        WeeklySection::from_str("this weeks goal").unwrap(),
        WeeklySection::ThisWeeksGoal
    );
    // The former name is kept as a deprecated alias; it resolves to the
    // renamed section so pre-rename callers don't hard-fail.
    assert_eq!(
        WeeklySection::from_str("Next Week's Focus").unwrap(),
        WeeklySection::ThisWeeksGoal
    );
}

#[test]
fn weekly_section_rejects_unknown_with_an_allowlist_message() {
    let err = WeeklySection::from_str("retrospective").unwrap_err();
    assert!(err.contains("unknown weekly section"), "msg: {err}");
    assert!(
        err.contains("this week's goal"),
        "msg names allowlist: {err}"
    );
}

// --- read_weekly_note -------------------------------------------------

#[test]
fn read_weekly_note_reports_absence_without_erroring() {
    let (vault, _store) = make_vault();

    let view = vault.read_weekly_note(midweek()).expect("read succeeds");

    assert!(!view.exists, "no note created yet");
    assert!(view.markdown.is_empty());
    // ISO week 18 of 2026, keyed by week regardless of the day passed.
    assert!(
        view.path.to_string().ends_with("2026-W18.md"),
        "path: {}",
        view.path
    );
}

// --- upsert_weekly_section -------------------------------------------

#[test]
fn upsert_scaffolds_the_note_with_iso_week_frontmatter_and_four_sections() {
    let (vault, store) = make_vault();

    let path = vault
        .upsert_weekly_section(
            midweek(),
            WeeklySection::Wins,
            "- Shipped the release.",
            false,
        )
        .expect("upsert wins");
    let raw = store.read_file(&path).unwrap();

    // Frontmatter keyed off the ISO week: Monday start, Sunday end.
    assert!(raw.contains("type: weekly"), "{raw}");
    assert!(raw.contains("week: 2026-W18"), "{raw}");
    assert!(raw.contains("date_start: 2026-04-27"), "{raw}");
    assert!(raw.contains("date_end: 2026-05-03"), "{raw}");
    assert!(raw.contains("# Week 18, 2026"), "{raw}");
    // All four sections scaffolded, with the written one filled.
    assert!(raw.contains("## Wins\n- Shipped the release."), "{raw}");
    assert!(raw.contains("## Challenges"), "{raw}");
    assert!(raw.contains("## One Improvement"), "{raw}");
    assert!(raw.contains("## This Week's Goal"), "{raw}");
}

#[test]
fn upsert_is_keyed_by_iso_week_so_any_day_writes_the_same_note() {
    let (vault, store) = make_vault();
    let monday = NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
    let sunday = NaiveDate::from_ymd_opt(2026, 5, 3).unwrap();

    let p1 = vault
        .upsert_weekly_section(monday, WeeklySection::Wins, "- a", false)
        .unwrap();
    let p2 = vault
        .upsert_weekly_section(sunday, WeeklySection::Challenges, "- b", false)
        .unwrap();

    assert_eq!(
        p1, p2,
        "Monday and Sunday of the same ISO week share a note"
    );
    let raw = store.read_file(&p1).unwrap();
    assert!(
        raw.contains("- a") && raw.contains("- b"),
        "both writes landed:\n{raw}"
    );
}

#[test]
fn upsert_replace_overwrites_the_section_append_accrues() {
    let (vault, store) = make_vault();

    vault
        .upsert_weekly_section(
            midweek(),
            WeeklySection::ThisWeeksGoal,
            "First draft.",
            false,
        )
        .unwrap();
    // Replace (default) swaps the content.
    let path = vault
        .upsert_weekly_section(
            midweek(),
            WeeklySection::ThisWeeksGoal,
            "Final focus.",
            false,
        )
        .unwrap();
    let raw = store.read_file(&path).unwrap();
    assert!(raw.contains("Final focus."), "{raw}");
    assert!(
        !raw.contains("First draft."),
        "replace dropped the old content:\n{raw}"
    );

    // Append grows the section.
    vault
        .upsert_weekly_section(midweek(), WeeklySection::Wins, "- one", false)
        .unwrap();
    let path = vault
        .upsert_weekly_section(midweek(), WeeklySection::Wins, "- two", true)
        .unwrap();
    let raw = store.read_file(&path).unwrap();
    assert!(
        raw.contains("- one") && raw.contains("- two"),
        "append accrued:\n{raw}"
    );
}

#[test]
fn read_weekly_note_returns_markdown_after_a_write() {
    let (vault, _store) = make_vault();

    vault
        .upsert_weekly_section(midweek(), WeeklySection::Wins, "- Did the thing.", false)
        .unwrap();
    let view = vault.read_weekly_note(midweek()).expect("read");

    assert!(view.exists);
    assert!(
        view.markdown.contains("- Did the thing."),
        "{}",
        view.markdown
    );
}
