//! Tests for `Vault::note_to_daily` (RFC 0002 §5.4, T2): a `### <heading>`
//! entry under the daily note's `## Notes` and a `noted [[…#<heading>]]`
//! pointer line under `## Logs`, in one transaction.

use std::sync::{Arc, Mutex};

use cdno_core::config::VaultConfig;
use cdno_core::error::StoreError;
use cdno_core::file_meta::FileMeta;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

const DAILY: &str = "journal/2026/daily/2026-09-27.md";

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn at(hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 9, 27)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
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

fn read(store: &Arc<dyn VaultStore>) -> String {
    store.read_file(&vp(DAILY)).expect("daily note exists")
}

/// The `## Logs` section's content, via the same lookup the writers use.
fn logs_of(content: &str) -> String {
    MarkdownDocument::parse(content.to_owned())
        .unwrap()
        .section("Logs")
        .unwrap()
        .to_owned()
}

fn assert_heading_refused(err: DomainError, expected_heading: &str) {
    match err {
        DomainError::HistoryEntryHeadingInvalid {
            ref section,
            ref heading,
            ..
        } => {
            assert_eq!(section, "Notes");
            assert_eq!(heading, expected_heading);
        }
        other => panic!("expected HistoryEntryHeadingInvalid, got {other:?}"),
    }
}

#[test]
fn note_to_daily_writes_entry_and_pointer_in_one_commit() {
    let (vault, store, _index) = make_vault();
    vault.log_to_daily_note(at(9, 0), "started").unwrap();

    let outcome = vault
        .note_to_daily(
            at(14, 32),
            "Woodbury identity",
            "Cheap low-rank update; used on [[projects/surrogate-model]].",
        )
        .expect("note_to_daily succeeds");

    assert_eq!(outcome.path, vp(DAILY));
    assert_eq!(
        outcome.target,
        "journal/2026/daily/2026-09-27#Woodbury identity"
    );
    assert_eq!(
        outcome.log_line,
        "noted [[journal/2026/daily/2026-09-27#Woodbury identity]] ([[projects/surrogate-model]])"
    );

    let content = read(&store);
    let doc = MarkdownDocument::parse(content.clone()).unwrap();
    // The entry is addressable by its heading: the anchor round-trips.
    let entry = doc.section("Woodbury identity").expect("entry present");
    assert!(entry.contains("Cheap low-rank update"), "{content}");
    assert!(
        doc.section("Notes")
            .unwrap()
            .contains("### Woodbury identity\n"),
        "entry sits under ## Notes:\n{content}"
    );
    let logs = logs_of(&content);
    assert!(
        logs.contains(&format!("- **14:32**: {}\n", outcome.log_line)),
        "pointer under ## Logs:\n{content}"
    );
    assert!(logs.contains("- **09:00**: started"), "{content}");

    let notes = content.find("## Notes").unwrap();
    let logs_at = content.find("## Logs").unwrap();
    assert!(notes < logs_at, "## Notes precedes ## Logs:\n{content}");
}

#[test]
fn pointer_line_carries_the_bodys_wikilinks_in_order_deduplicated() {
    let (vault, _store, _index) = make_vault();

    let outcome = vault
        .note_to_daily(
            at(10, 0),
            "Linked entry",
            "First [[projects/a]], then [[questions/research/b|B]],\nand again [[projects/a]].",
        )
        .unwrap();

    assert_eq!(
        outcome.log_line,
        "noted [[journal/2026/daily/2026-09-27#Linked entry]] ([[projects/a]] [[questions/research/b|B]])"
    );
    assert_eq!(outcome.log_line.matches("[[projects/a]]").count(), 1);
    assert_eq!(
        outcome
            .log_line
            .matches("[[questions/research/b|B]]")
            .count(),
        1
    );
}

#[test]
fn pointer_line_has_no_parentheses_when_body_has_no_links() {
    let (vault, store, _index) = make_vault();

    let outcome = vault
        .note_to_daily(at(10, 0), "Plain entry", "No links here.")
        .unwrap();

    assert_eq!(
        outcome.log_line,
        "noted [[journal/2026/daily/2026-09-27#Plain entry]]"
    );
    assert!(!outcome.log_line.contains('('));
    assert!(
        logs_of(&read(&store))
            .contains("- **10:00**: noted [[journal/2026/daily/2026-09-27#Plain entry]]\n")
    );
}

#[test]
fn duplicate_heading_is_refused_and_nothing_is_written() {
    let (vault, store, _index) = make_vault();
    vault
        .note_to_daily(at(10, 0), "Woodbury identity", "first")
        .unwrap();
    let before = read(&store);

    let err = vault
        .note_to_daily(at(11, 0), "WOODBURY identity", "second")
        .unwrap_err();

    assert_heading_refused(err, "WOODBURY identity");
    assert_eq!(read(&store), before, "file must be byte-identical");
}

#[test]
fn section_name_heading_is_refused() {
    let (vault, store, _index) = make_vault();
    vault.log_to_daily_note(at(9, 0), "started").unwrap();
    let before = read(&store);

    for name in ["Standup", "logs"] {
        let err = vault.note_to_daily(at(10, 0), name, "body").unwrap_err();
        assert_heading_refused(err, name);
        assert_eq!(read(&store), before, "nothing written for `{name}`");
    }
}

#[test]
fn empty_heading_is_refused() {
    let (vault, store, _index) = make_vault();

    let err = vault.note_to_daily(at(10, 0), "   ", "body").unwrap_err();

    match err {
        DomainError::HistoryEntryHeadingInvalid { ref reason, .. } => {
            assert_eq!(reason, "empty");
        }
        other => panic!("expected HistoryEntryHeadingInvalid, got {other:?}"),
    }
    assert!(!store.exists(&vp(DAILY)).unwrap(), "nothing written");
}

#[test]
fn heading_with_newline_is_refused() {
    let (vault, store, _index) = make_vault();

    let err = vault
        .note_to_daily(at(10, 0), "First line\nsecond line", "body")
        .unwrap_err();

    assert_heading_refused(err, "First line\nsecond line");
    assert!(!store.exists(&vp(DAILY)).unwrap(), "nothing written");
}

#[test]
fn heading_that_would_break_the_pointer_is_refused() {
    let (vault, store, _index) = make_vault();

    for heading in ["See [[x]]", "A | B", "C# tips"] {
        let err = vault.note_to_daily(at(10, 0), heading, "body").unwrap_err();
        assert_heading_refused(err, heading);
    }
    assert!(!store.exists(&vp(DAILY)).unwrap(), "nothing written");
}

#[test]
fn creates_the_daily_note_when_absent() {
    let (vault, store, _index) = make_vault();
    assert!(!store.exists(&vp(DAILY)).unwrap());

    let outcome = vault
        .note_to_daily(at(8, 5), "Fresh entry", "body text")
        .unwrap();

    let content = read(&store);
    assert!(content.contains("type: daily"), "{content}");
    let notes = content.find("## Notes").expect("## Notes created");
    let logs_at = content.find("## Logs").expect("## Logs present");
    assert!(notes < logs_at, "## Notes above ## Logs:\n{content}");
    let logs = logs_of(&content);
    let log_lines: Vec<&str> = logs.lines().filter(|l| l.starts_with("- ")).collect();
    let expected = format!("- **08:05**: {}", outcome.log_line);
    assert_eq!(
        log_lines,
        vec![expected.as_str()],
        "the pointer is the only log line:\n{content}"
    );
}

#[test]
fn entry_is_atomic_with_its_pointer() {
    // Fail the second store write after `Vault::new`. `note_to_daily`
    // writes the daily note once, so it succeeds with both halves
    // present; were the pointer staged in a separate commit, the first
    // commit would land the entry and the second write would fail,
    // leaving an entry with no pointer — the state this test forbids.
    let backing = Arc::new(MemoryVaultStore::new());
    let store: Arc<dyn VaultStore> = Arc::new(FailingStore::new(backing.clone(), 2));
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");

    let result = vault.note_to_daily(at(14, 32), "Atomic entry", "see [[projects/a]]");

    let path = vp(DAILY);
    let content = if backing.exists(&path).unwrap() {
        backing.read_file(&path).unwrap()
    } else {
        String::new()
    };
    let has_entry = content.contains("### Atomic entry");
    let has_pointer = content.contains("noted [[journal/2026/daily/2026-09-27#Atomic entry]]");
    assert_eq!(
        has_entry, has_pointer,
        "entry and pointer must land together or not at all:\n{content}"
    );
    match result {
        Ok(_) => assert!(has_entry && has_pointer, "{content}"),
        Err(_) => assert!(content.is_empty(), "nothing may land:\n{content}"),
    }

    // Failing the very first write: the call errors and nothing lands.
    let backing = Arc::new(MemoryVaultStore::new());
    let store: Arc<dyn VaultStore> = Arc::new(FailingStore::new(backing.clone(), 1));
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    let err = vault
        .note_to_daily(at(14, 32), "Atomic entry", "see [[projects/a]]")
        .unwrap_err();
    assert!(matches!(err, DomainError::Transaction(_)), "got {err:?}");
    assert!(!backing.exists(&path).unwrap(), "nothing may land");
}

#[test]
fn reindex_resolves_the_pointer_to_the_daily_note() {
    let (vault, store, _index) = make_vault();
    let outcome = vault
        .note_to_daily(at(14, 32), "Woodbury identity", "body")
        .unwrap();
    drop(vault);

    // Reindex: rebuild a fresh index from the files alone, as
    // `cdno reindex` (or startup reconciliation over a lost cache) does.
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (_vault, _report) = Vault::new(
        Arc::clone(&store),
        Arc::clone(&index),
        VaultConfig::default(),
    )
    .expect("Vault::new reconciles");

    let daily = vp(DAILY);
    let links = index.find_outgoing_links(&daily).unwrap();
    let pointer = links
        .iter()
        .find(|l| l.target_raw == outcome.target)
        .unwrap_or_else(|| panic!("pointer link row missing: {links:?}"));
    assert_eq!(pointer.resolved_path.as_ref(), Some(&daily));
    assert!(index.find_backlinks(&daily).unwrap().contains(&daily));
}

/// Wraps a `MemoryVaultStore`, failing the Nth write/append/move/delete
/// so the transaction rollback path can be exercised at the domain
/// level. Reads, `exists`, and directory walks never fail or count, so
/// `Vault::new` reconciliation runs cleanly before the counter matters.
///
/// A local copy of the one in `actions_tests.rs`: the shared
/// `tests/unit/support.rs` (#642) is not on `main` yet.
struct FailingStore {
    inner: Arc<MemoryVaultStore>,
    fail_on: usize,
    count: Mutex<usize>,
}

impl FailingStore {
    fn new(inner: Arc<MemoryVaultStore>, fail_on: usize) -> Self {
        Self {
            inner,
            fail_on,
            count: Mutex::new(0),
        }
    }

    /// Increment the write counter; return true exactly when this is
    /// the write that should fail.
    fn tick(&self) -> bool {
        let mut c = self.count.lock().unwrap();
        *c += 1;
        *c == self.fail_on
    }
}

impl VaultStore for FailingStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        self.inner.read_file(path)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        self.inner.read_bytes(path)
    }
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        if self.tick() {
            return Err(StoreError::PermissionDenied(path.to_string()));
        }
        self.inner.write_file(path, content)
    }
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        if self.tick() {
            return Err(StoreError::PermissionDenied(path.to_string()));
        }
        self.inner.append_to_file(path, content)
    }
    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError> {
        if self.tick() {
            return Err(StoreError::PermissionDenied(src.to_string()));
        }
        self.inner.move_file(src, dest)
    }
    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError> {
        if self.tick() {
            return Err(StoreError::PermissionDenied(path.to_string()));
        }
        self.inner.delete_file(path)
    }
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        self.inner.exists(path)
    }
    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.list_dir(path)
    }
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.walk_dir(path)
    }
    fn metadata(&self, path: &VaultPath) -> Result<FileMeta, StoreError> {
        self.inner.metadata(path)
    }
    fn import_external(&self, src: &std::path::Path, dest: &VaultPath) -> Result<(), StoreError> {
        if self.tick() {
            return Err(StoreError::PermissionDenied(dest.to_string()));
        }
        self.inner.import_external(src, dest)
    }
}
