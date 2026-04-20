use std::sync::Arc;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;

/// A fixed moment used across tests so formatted log lines stay
/// deterministic and assertions don't depend on the wall clock.
fn sample_moment() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 4, 20)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(14, 30, 0).unwrap())
}

fn daily_path() -> VaultPath {
    VaultPath::new("journal/daily/2026-04-20.md").unwrap()
}

fn make_vault() -> (Vault, Arc<dyn VaultStore>, Arc<dyn VaultIndex>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) = Vault::new(
        Arc::clone(&store),
        Arc::clone(&index),
        VaultConfig::default(),
    )
    .expect("Vault::new on empty store");
    (vault, store, index)
}

#[test]
fn new_vault_on_empty_store_produces_empty_report() {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (_vault, report) =
        Vault::new(store, index, VaultConfig::default()).expect("Vault::new on empty store");

    assert_eq!(report.scanned, 0);
    assert_eq!(report.added, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(report.removed, 0);
    assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
}

#[test]
fn log_to_daily_note_creates_missing_file_with_logs_section() {
    let (vault, store, _index) = make_vault();

    let path = vault
        .log_to_daily_note(sample_moment(), "first entry of the day")
        .expect("log_to_daily_note on missing file");

    assert_eq!(path, daily_path());

    let content = store.read_file(&path).expect("daily note written");
    assert!(
        content.contains("type: daily"),
        "frontmatter missing type: daily\ncontent:\n{content}",
    );
    assert!(
        content.contains("## Logs\n"),
        "Logs section missing\ncontent:\n{content}",
    );
    assert!(
        content.contains("- **14:30**: first entry of the day\n"),
        "log line missing or wrong format\ncontent:\n{content}",
    );
}

#[test]
fn log_to_daily_note_appends_into_logs_section_of_existing_file() {
    let (vault, store, _index) = make_vault();

    // Two consecutive logs on the same day.
    vault
        .log_to_daily_note(sample_moment(), "first entry")
        .expect("first log succeeds");

    let later = NaiveDate::from_ymd_opt(2026, 4, 20)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(16, 5, 0).unwrap());
    vault
        .log_to_daily_note(later, "second entry")
        .expect("second log succeeds");

    let content = store.read_file(&daily_path()).unwrap();

    // Both lines present, first entry appears before the second, and
    // both live under the Logs heading (not tacked onto the end of
    // the file).
    let first_pos = content
        .find("- **14:30**: first entry")
        .expect("first log line");
    let second_pos = content
        .find("- **16:05**: second entry")
        .expect("second log line");
    let logs_pos = content.find("## Logs\n").expect("Logs heading");

    assert!(logs_pos < first_pos, "first log should follow Logs heading");
    assert!(first_pos < second_pos, "second log should come after first");
}

#[test]
fn log_to_daily_note_errors_when_logs_section_missing() {
    let (vault, store, _index) = make_vault();

    // Pre-seed a daily note without a Logs section so the append path
    // has nowhere to insert into. This is the unhappy case the
    // `MarkdownDocument` contract surfaces explicitly.
    let path = daily_path();
    store
        .write_file(
            &path,
            "---\ndate: 2026-04-20\ntype: daily\n---\n\n# Monday\n\n## Captured\n",
        )
        .unwrap();

    let err = vault
        .log_to_daily_note(sample_moment(), "entry with nowhere to go")
        .expect_err("should fail without a Logs section");

    // The raw error type isn't the focus; what matters is that the
    // failure surfaces rather than corrupting the file.
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("logs") || msg.to_lowercase().contains("section"),
        "error should mention the missing section; got: {msg}",
    );
}

#[test]
fn log_to_daily_note_upserts_index_row_for_the_daily_note() {
    let (vault, _store, index) = make_vault();

    let path = vault
        .log_to_daily_note(sample_moment(), "logged")
        .expect("log succeeds");

    let row = index
        .find_by_path(&path)
        .expect("index lookup")
        .expect("row present");

    assert_eq!(row.path, path);
    assert_eq!(row.note_type, "daily");
    assert!(
        row.size > 0,
        "indexed size should reflect the written content",
    );
    assert!(
        !row.content_hash.is_empty(),
        "content_hash should be populated",
    );
}
