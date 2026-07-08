//! Tests for the monthly-note read + section writes
//! (`Vault::read_monthly_note`, `Vault::upsert_monthly_section`), the
//! [`MonthlySection`] allowlist, and the month's weeks-link block
//! (design §5.2).

use std::str::FromStr;
use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::{MonthlySection, SearchFilters, Vault};
use chrono::NaiveDate;

/// A mid-month day in July 2026 (Wed 2026-07-01 .. Fri 2026-07-31).
fn midmonth() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()
}

fn make_vault() -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) = Vault::new(Arc::clone(&store), index, VaultConfig::default())
        .expect("Vault::new on empty store");
    (vault, store)
}

// --- MonthlySection parsing ------------------------------------------

#[test]
fn monthly_section_parses_case_and_punctuation_insensitively() {
    assert_eq!(
        MonthlySection::from_str("Wins").unwrap(),
        MonthlySection::Wins
    );
    assert_eq!(
        MonthlySection::from_str("themes").unwrap(),
        MonthlySection::Themes
    );
    assert_eq!(
        MonthlySection::from_str("Next Month's Focus").unwrap(),
        MonthlySection::NextMonthsFocus
    );
    assert_eq!(
        MonthlySection::from_str("next months focus").unwrap(),
        MonthlySection::NextMonthsFocus
    );
    assert_eq!(
        MonthlySection::from_str("Next-Months-Focus").unwrap(),
        MonthlySection::NextMonthsFocus
    );
    assert_eq!(
        MonthlySection::from_str("next_months_focus").unwrap(),
        MonthlySection::NextMonthsFocus
    );
}

#[test]
fn monthly_section_rejects_unknown_with_an_allowlist_message() {
    let err = MonthlySection::from_str("metrics").unwrap_err();
    assert!(err.contains("unknown monthly section"), "msg: {err}");
    assert!(
        err.contains("next month's focus"),
        "msg names allowlist: {err}"
    );
}

// --- read_monthly_note ------------------------------------------------

#[test]
fn read_monthly_note_reports_absence_without_erroring() {
    let (vault, _store) = make_vault();

    let view = vault.read_monthly_note(midmonth()).expect("read succeeds");

    assert!(!view.exists, "no note created yet");
    assert!(view.markdown.is_empty());
    // Keyed by the calendar month, regardless of the day passed.
    assert!(
        view.path.to_string().ends_with("2026-07.md"),
        "path: {}",
        view.path
    );
    assert!(
        view.path.to_string() == "journal/2026/monthly/2026-07.md",
        "path scheme: {}",
        view.path
    );
}

// --- upsert_monthly_section ------------------------------------------

#[test]
fn upsert_scaffolds_the_note_with_month_frontmatter_and_three_sections() {
    let (vault, store) = make_vault();

    let path = vault
        .upsert_monthly_section(
            midmonth(),
            MonthlySection::Wins,
            "- Shipped the release.",
            false,
        )
        .expect("upsert wins");
    let raw = store.read_file(&path).unwrap();

    // Frontmatter keyed off the calendar month: first-to-last of month.
    assert!(raw.contains("type: monthly"), "{raw}");
    assert!(raw.contains("month: 2026-07"), "{raw}");
    assert!(raw.contains("date_start: 2026-07-01"), "{raw}");
    assert!(raw.contains("date_end: 2026-07-31"), "{raw}");
    assert!(raw.contains("# July 2026"), "{raw}");
    // The three review sections scaffolded, with the written one filled.
    assert!(raw.contains("## Wins\n- Shipped the release."), "{raw}");
    assert!(raw.contains("## Themes"), "{raw}");
    assert!(raw.contains("## Next Month's Focus"), "{raw}");
    // No Metrics section — quantitative metrics stay behind the desktop
    // 'show metrics' toggle, not a note section (design law).
    assert!(!raw.contains("## Metrics"), "no Metrics section:\n{raw}");
    // ...in ritual order.
    let wins = raw.find("## Wins").unwrap();
    let themes = raw.find("## Themes").unwrap();
    let focus = raw.find("## Next Month's Focus").unwrap();
    assert!(
        wins < themes && themes < focus,
        "sections out of order:\n{raw}"
    );
}

#[test]
fn scaffold_links_the_months_weeks_as_wikilinks_one_per_monday() {
    // July 2026's Mondays are the 6th, 13th, 20th, 27th (ISO weeks
    // W28..W31). The scaffold links (never copies) each week's note.
    let (vault, store) = make_vault();

    let path = vault
        .upsert_monthly_section(midmonth(), MonthlySection::Wins, "x", false)
        .expect("upsert");
    let raw = store.read_file(&path).unwrap();

    assert!(raw.contains("## Weeks"), "weeks block present:\n{raw}");
    assert!(
        raw.contains("- [[journal/2026/weekly/2026-W28]]"),
        "W28 link:\n{raw}"
    );
    assert!(
        raw.contains("- [[journal/2026/weekly/2026-W29]]"),
        "W29 link:\n{raw}"
    );
    assert!(
        raw.contains("- [[journal/2026/weekly/2026-W30]]"),
        "W30 link:\n{raw}"
    );
    assert!(
        raw.contains("- [[journal/2026/weekly/2026-W31]]"),
        "W31 link:\n{raw}"
    );
    // Exactly four Mondays in July 2026 -> four week bullets.
    assert_eq!(
        raw.matches("- [[journal/").count(),
        4,
        "one bullet per Monday in the month:\n{raw}"
    );
}

#[test]
fn scaffold_weeks_handle_month_boundaries() {
    let (vault, store) = make_vault();

    // A month starting ON a Monday (June 2026, 1 June is a Monday): the
    // 1st must be included and it has five Mondays.
    let june = vault
        .upsert_monthly_section(
            NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
            MonthlySection::Wins,
            "x",
            false,
        )
        .unwrap();
    let june_raw = store.read_file(&june).unwrap();
    assert!(
        june_raw.contains("- [[journal/2026/weekly/2026-W23]]"),
        "1 June (a Monday) is included:\n{june_raw}"
    );
    assert_eq!(
        june_raw.matches("- [[journal/").count(),
        5,
        "June 2026 has five Mondays:\n{june_raw}"
    );

    // A Monday on the last day of the month whose ISO week straddles into
    // the next month (31 Aug 2026 -> W36): it lists under August, keyed
    // by the Monday's own date, not by the week.
    let aug = vault
        .upsert_monthly_section(
            NaiveDate::from_ymd_opt(2026, 8, 15).unwrap(),
            MonthlySection::Wins,
            "x",
            false,
        )
        .unwrap();
    let aug_raw = store.read_file(&aug).unwrap();
    assert!(
        aug_raw.contains("- [[journal/2026/weekly/2026-W36]]"),
        "31 Aug (a Monday) is listed under August:\n{aug_raw}"
    );
    assert_eq!(
        aug_raw.matches("- [[journal/").count(),
        5,
        "August 2026 has five Mondays:\n{aug_raw}"
    );

    // December exercises the year-rollover in the last-of-month
    // computation (Jan of the next year) and ISO week 53.
    let dec = vault
        .upsert_monthly_section(
            NaiveDate::from_ymd_opt(2026, 12, 15).unwrap(),
            MonthlySection::Wins,
            "x",
            false,
        )
        .unwrap();
    let dec_raw = store.read_file(&dec).unwrap();
    assert!(dec_raw.contains("date_end: 2026-12-31"), "{dec_raw}");
    assert!(
        dec_raw.contains("- [[journal/2026/weekly/2026-W53]]"),
        "28 Dec (a Monday) links ISO week 53:\n{dec_raw}"
    );
}

#[test]
fn upsert_is_keyed_by_month_so_any_day_writes_the_same_note() {
    let (vault, store) = make_vault();
    let first = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
    let last = NaiveDate::from_ymd_opt(2026, 7, 31).unwrap();

    let p1 = vault
        .upsert_monthly_section(first, MonthlySection::Wins, "- a", false)
        .unwrap();
    let p2 = vault
        .upsert_monthly_section(last, MonthlySection::Themes, "- b", false)
        .unwrap();

    assert_eq!(p1, p2, "the 1st and the 31st share the month's note");
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
        .upsert_monthly_section(
            midmonth(),
            MonthlySection::NextMonthsFocus,
            "First draft.",
            false,
        )
        .unwrap();
    // Replace (default) swaps the content.
    let path = vault
        .upsert_monthly_section(
            midmonth(),
            MonthlySection::NextMonthsFocus,
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
        .upsert_monthly_section(midmonth(), MonthlySection::Wins, "- one", false)
        .unwrap();
    let path = vault
        .upsert_monthly_section(midmonth(), MonthlySection::Wins, "- two", true)
        .unwrap();
    let raw = store.read_file(&path).unwrap();
    assert!(
        raw.contains("- one") && raw.contains("- two"),
        "append accrued:\n{raw}"
    );
}

#[test]
fn read_monthly_note_returns_markdown_after_a_write() {
    let (vault, _store) = make_vault();

    vault
        .upsert_monthly_section(midmonth(), MonthlySection::Wins, "- Did the thing.", false)
        .unwrap();
    let view = vault.read_monthly_note(midmonth()).expect("read");

    assert!(view.exists);
    assert!(
        view.markdown.contains("- Did the thing."),
        "{}",
        view.markdown
    );
}

#[test]
fn reconcile_recognises_a_monthly_note_by_its_type_frontmatter() {
    // Write a monthly note straight to the store, then let `Vault::new`'s
    // startup reconciliation index it. Reconcile derives the note type
    // from the `type:` frontmatter string, so a `monthly`-typed note must
    // be indexed as `monthly` and reachable by a `note_type=monthly`
    // search filter — the same recognition weekly notes get.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let path = VaultPath::new("journal/2026/monthly/2026-07.md").unwrap();
    store
        .write_file(
            &path,
            "---\ntype: monthly\nmonth: 2026-07\n---\n# July 2026\n\n## Wins\nzentangle marginalia\n",
        )
        .unwrap();
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");

    let filters = SearchFilters {
        note_type_names: vec!["monthly".to_owned()],
        ..Default::default()
    };
    let hits = vault.search("zentangle", &filters, 10).expect("search");
    assert!(
        hits.iter()
            .any(|h| h.path == path && h.note_type == "monthly"),
        "reconcile indexed the note as type=monthly: {hits:?}"
    );
}
