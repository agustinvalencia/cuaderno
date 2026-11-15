//! `Journal` — the focus readers over a store alone (#1003).
//!
//! `Journal` takes no index, so "it never opens one" holds by
//! construction; what these tests pin is that it answers exactly as
//! `Vault` answers over the same store and config, that it honours the
//! `[focus]` windows, and that it refuses a config `Vault::new` refuses.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::error::DomainError;
use cdno_domain::{Journal, Vault};
use chrono::NaiveDate;

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 13).unwrap()
}

fn yesterday() -> NaiveDate {
    today().pred_opt().unwrap()
}

/// Write `date`'s daily note with `body` as its `## Logs`, verbatim.
fn write_daily(store: &dyn VaultStore, date: NaiveDate, body: &str) {
    let note = format!("---\ndate: {date}\ntype: daily\n---\n\n# {date}\n\n## Logs\n{body}\n");
    let path = VaultPath::new(cdno_core::paths::daily_note_relpath(date)).unwrap();
    store.write_file(&path, &note).unwrap();
}

/// Two days of log: a start carried over from yesterday, a pause with a
/// `next:` hint on another project, a promotion renaming the open start.
fn two_day_store() -> Arc<dyn VaultStore> {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    write_daily(
        &*store,
        yesterday(),
        "- **09:00**: started [[surrogate-model]] \u{2014} Fit the baseline (deep)\n\
         - **10:00**: action paused on [[surrogate-model]] \u{2014} Fit the baseline (deep)\n  \
           next: rerun with the new seed\n\
         - **16:30**: started [[surrogate-model]] \u{2014} Write the methods section (medium)",
    );
    write_daily(
        &*store,
        today(),
        "- **08:15**: action promoted on [[surrogate-model]] \u{2014} \"Write the methods section\" -> [[actions/methods-section]]",
    );
    store
}

/// Now that `Vault`'s focus readers delegate to `Journal`, the equality
/// with `Vault` holds by construction; it guards against the delegation
/// being undone. The fixed assertions after each comparison are what pin
/// the fold itself: the carried start, the promotion's rename, the pause.
#[test]
fn a_journal_answers_as_the_vault_does_over_the_same_store() {
    let store = two_day_store();
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    let journal = Journal::new(Arc::clone(&store), VaultConfig::default()).expect("Journal::new");

    let focus = journal.current_focus(today()).unwrap();
    assert_eq!(focus, vault.current_focus(today()).unwrap());
    let focus = focus.expect("the carried, promoted start is open");
    assert_eq!(focus.project, "surrogate-model");
    assert_eq!(focus.action, "[[actions/methods-section]] (medium)");
    assert_eq!(focus.date, yesterday());

    let pauses = journal.open_pauses(today()).unwrap();
    assert_eq!(pauses, vault.open_pauses(today()).unwrap());
    let latest = pauses.latest.expect("the pause is open");
    assert_eq!(latest.next.as_deref(), Some("rerun with the new seed"));
    assert_eq!(
        journal.last_paused(today()).unwrap(),
        vault.last_paused(today()).unwrap()
    );
    assert_eq!(
        journal.method_status().unwrap(),
        vault.method_status().unwrap()
    );
}

#[test]
fn a_journal_honours_the_carry_over_window() {
    let store = two_day_store();
    let mut config = VaultConfig::default();
    config.focus.carry_over_days = 0;
    let journal = Journal::new(store, config).expect("Journal::new");
    // Today's note alone holds only a promotion, which renames nothing
    // when no start is open in the window.
    assert_eq!(journal.current_focus(today()).unwrap(), None);
}

#[test]
fn a_journal_refuses_a_config_the_vault_refuses() {
    let mut config = VaultConfig::default();
    config.focus.carry_over_days = config.focus.max_window_days + 1;
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    assert!(Vault::new(Arc::clone(&store), index, config.clone()).is_err());
    assert!(matches!(
        Journal::new(store, config),
        Err(DomainError::Config(_))
    ));
}
