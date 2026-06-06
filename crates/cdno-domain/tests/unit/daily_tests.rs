//! Tests for the daily-note read + planning-section writes
//! (`Vault::read_daily_note`, `Vault::upsert_daily_section`) added in
//! GH #158, and the [`DailySection`] allowlist.

use std::str::FromStr;
use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::{DailySection, Vault};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 4, 26).unwrap()
}

fn moment() -> NaiveDateTime {
    date().and_time(NaiveTime::from_hms_opt(9, 15, 0).unwrap())
}

fn make_vault() -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) = Vault::new(Arc::clone(&store), index, VaultConfig::default())
        .expect("Vault::new on empty store");
    (vault, store)
}

// --- read_daily_note --------------------------------------------------

#[test]
fn read_daily_note_reports_absence_without_erroring() {
    let (vault, _store) = make_vault();

    let view = vault.read_daily_note(date()).expect("read succeeds");

    assert!(!view.exists, "no note created yet");
    assert!(view.markdown.is_empty());
    // The path is still resolved so callers can reference it.
    assert!(view.path.to_string().ends_with("2026-04-26.md"));
}

#[test]
fn read_daily_note_returns_markdown_when_present() {
    let (vault, _store) = make_vault();
    vault
        .log_to_daily_note(moment(), "did a thing")
        .expect("log creates the note");

    let view = vault.read_daily_note(date()).expect("read succeeds");

    assert!(view.exists);
    assert!(view.markdown.contains("type: daily"));
    assert!(view.markdown.contains("did a thing"));
}

// --- upsert_daily_section --------------------------------------------

#[test]
fn upsert_creates_the_note_and_section_when_absent() {
    let (vault, store) = make_vault();

    let path = vault
        .upsert_daily_section(date(), DailySection::Intention, "Ship the daily-note tools")
        .expect("upsert creates the note");

    let content = store.read_file(&path).unwrap();
    assert!(content.contains("type: daily"));
    // Logs section is scaffolded even though we wrote a planning section.
    assert!(content.contains("## Logs"));
    assert!(content.contains("## Intention"));
    assert!(content.contains("Ship the daily-note tools"));
}

#[test]
fn upsert_leaves_the_logs_history_untouched() {
    let (vault, store) = make_vault();
    vault
        .log_to_daily_note(moment(), "morning log entry")
        .expect("log creates the note");

    let path = vault
        .upsert_daily_section(date(), DailySection::Standup, "Yesterday: shipped X")
        .expect("upsert succeeds");

    let content = store.read_file(&path).unwrap();
    assert!(
        content.contains("morning log entry"),
        "append-only log must survive a section upsert"
    );
    assert!(content.contains("## Standup"));
    assert!(content.contains("Yesterday: shipped X"));
}

#[test]
fn upsert_overwrites_an_existing_section() {
    let (vault, store) = make_vault();

    vault
        .upsert_daily_section(date(), DailySection::Intention, "first take")
        .expect("first upsert");
    let path = vault
        .upsert_daily_section(date(), DailySection::Intention, "second take")
        .expect("second upsert overwrites");

    let content = store.read_file(&path).unwrap();
    assert!(content.contains("second take"));
    assert!(
        !content.contains("first take"),
        "planning section is replaced, not appended"
    );
}

#[test]
fn upsert_with_empty_content_clears_to_just_the_heading() {
    let (vault, store) = make_vault();
    vault
        .upsert_daily_section(date(), DailySection::Agenda, "10:00 standup")
        .expect("seed agenda");

    let path = vault
        .upsert_daily_section(date(), DailySection::Agenda, "")
        .expect("clear agenda");

    let content = store.read_file(&path).unwrap();
    assert!(content.contains("## Agenda"));
    assert!(!content.contains("10:00 standup"));
}

// --- DailySection allowlist ------------------------------------------

#[test]
fn daily_section_parses_case_insensitively() {
    assert_eq!(
        DailySection::from_str("Standup").unwrap(),
        DailySection::Standup
    );
    assert_eq!(
        DailySection::from_str("  intention ").unwrap(),
        DailySection::Intention
    );
    assert_eq!(
        DailySection::from_str("AGENDA").unwrap(),
        DailySection::Agenda
    );
}

#[test]
fn daily_section_rejects_history_and_unknown_sections() {
    // History sections are append-only and deliberately not on the
    // allowlist.
    assert!(DailySection::from_str("Logs").is_err());
    assert!(DailySection::from_str("Notes").is_err());
    let err = DailySection::from_str("whatever").unwrap_err();
    assert!(err.contains("standup"), "error names the allowlist: {err}");
}
