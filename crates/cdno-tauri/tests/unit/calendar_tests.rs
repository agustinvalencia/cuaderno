//! The calendar-view command seams (`read_daily_impl`,
//! `read_weekly_impl`, `read_monthly_impl`, `list_daily_dates_impl`)
//! against the Memory doubles — no Tauri runtime. Exercises the
//! absence-tolerant reads and, crucially, the neighbour identities
//! `read_daily` stamps so the frontend never computes a domain date
//! (#340, plan §3.7).

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_tauri::commands::calendar::{
    list_daily_dates_impl, read_daily_impl, read_monthly_impl, read_weekly_impl,
};
use chrono::NaiveDate;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

fn vault_with(notes: &[(&str, &str)]) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

const DAILY: &str =
    "---\ntype: daily\ndate: 2026-07-15\n---\n\n# Wednesday\n\n## Logs\n- **09:00**: started\n";

#[test]
fn read_daily_stamps_neighbours_week_and_month() {
    // Wednesday 2026-07-15. Its neighbours, the Monday of its ISO week
    // (2026-07-13), and its month (2026-07) are all stamped in Rust so
    // the panel's quick-nav needs no client-side date maths.
    let vault = vault_with(&[("journal/2026/daily/2026-07-15.md", DAILY)]);

    let view = read_daily_impl(&vault, ymd(2026, 7, 15)).expect("read succeeds");

    assert!(view.exists);
    assert!(view.markdown.contains("started"));
    assert_eq!(view.date, ymd(2026, 7, 15));
    assert_eq!(view.prev_date, ymd(2026, 7, 14));
    assert_eq!(view.next_date, ymd(2026, 7, 16));
    assert_eq!(view.week_of, ymd(2026, 7, 13), "Monday of the ISO week");
    assert_eq!(view.month, "2026-07");
    assert_eq!(view.path, "journal/2026/daily/2026-07-15.md");
}

#[test]
fn read_daily_reports_absence_without_erroring() {
    // A day with no note is a calm empty state, not an error — but the
    // neighbours and the resolved path are still stamped so the panel can
    // navigate away and offer "Open in editor".
    let vault = vault_with(&[]);

    let view = read_daily_impl(&vault, ymd(2026, 7, 15)).expect("read succeeds");

    assert!(!view.exists);
    assert!(view.markdown.is_empty());
    assert_eq!(view.prev_date, ymd(2026, 7, 14));
    assert_eq!(view.next_date, ymd(2026, 7, 16));
    assert_eq!(view.path, "journal/2026/daily/2026-07-15.md");
}

#[test]
fn read_daily_month_neighbours_cross_the_boundary() {
    // The last of the month: the next-day neighbour rolls into August,
    // and the week Monday sits back in July — all stamped in Rust.
    let vault = vault_with(&[]);

    let view = read_daily_impl(&vault, ymd(2026, 7, 31)).expect("read succeeds");

    assert_eq!(view.next_date, ymd(2026, 8, 1), "rolls into next month");
    assert_eq!(view.prev_date, ymd(2026, 7, 30));
    assert_eq!(view.week_of, ymd(2026, 7, 27));
    assert_eq!(view.month, "2026-07");
}

#[test]
fn read_weekly_normalises_to_the_monday_and_tolerates_absence() {
    let vault = vault_with(&[]);

    // Any day in the week resolves to the same note; the view echoes the
    // Monday. Absence is non-error.
    let view = read_weekly_impl(&vault, ymd(2026, 7, 15)).expect("read succeeds");
    assert!(!view.exists);
    assert_eq!(view.week_of, ymd(2026, 7, 13));
    assert!(view.path.contains("weekly"));
}

#[test]
fn read_monthly_echoes_the_month_and_tolerates_absence() {
    let vault = vault_with(&[]);

    let view = read_monthly_impl(&vault, ymd(2026, 7, 1)).expect("read succeeds");
    assert!(!view.exists);
    assert_eq!(view.month, "2026-07");
    assert!(view.path.contains("monthly"));
}

#[test]
fn read_weekly_reads_an_existing_note() {
    // A weekly note filed at the ISO-week path (Mon 2026-07-13 is week
    // 29) — the read must surface exists:true and its markdown.
    let weekly = "---\ntype: weekly\n---\n\n# Week 29\n\n## Wins\nShipped.\n";
    let vault = vault_with(&[("journal/2026/weekly/2026-W29.md", weekly)]);

    let view = read_weekly_impl(&vault, ymd(2026, 7, 15)).expect("read succeeds");
    assert!(view.exists);
    assert_eq!(view.week_of, ymd(2026, 7, 13));
    assert!(view.markdown.contains("Shipped."));
}

#[test]
fn read_monthly_reads_an_existing_note() {
    let monthly = "---\ntype: monthly\n---\n\n# July 2026\n\n## Wins\nA good month.\n";
    let vault = vault_with(&[("journal/2026/monthly/2026-07.md", monthly)]);

    let view = read_monthly_impl(&vault, ymd(2026, 7, 1)).expect("read succeeds");
    assert!(view.exists);
    assert_eq!(view.month, "2026-07");
    assert!(view.markdown.contains("A good month."));
}

#[test]
fn list_daily_dates_returns_the_months_note_bearing_days() {
    let vault = vault_with(&[
        ("journal/2026/daily/2026-07-03.md", DAILY),
        ("journal/2026/daily/2026-07-15.md", DAILY),
        // A May note and a prior-year July note must not leak in.
        ("journal/2026/daily/2026-05-01.md", DAILY),
        ("journal/2025/daily/2025-07-20.md", DAILY),
    ]);

    let dates = list_daily_dates_impl(&vault, 2026, 7).expect("scan succeeds");
    assert_eq!(dates, vec![ymd(2026, 7, 3), ymd(2026, 7, 15)]);
}

#[test]
fn list_daily_dates_rejects_an_out_of_range_month() {
    let vault = vault_with(&[]);
    let err = list_daily_dates_impl(&vault, 2026, 13).expect_err("month 13 is invalid");
    assert!(matches!(err, cdno_tauri::error::CmdError::Invalid(_)));
}
