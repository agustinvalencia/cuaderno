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
use cdno_core::store::{MemoryVaultStore, VaultStore, VaultWriteLock};
use cdno_domain::error::DomainError;
use cdno_domain::{DailySection, Vault};

use super::support::FailingStore;
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
fn pointer_line_deduplicates_links_by_target_keeping_the_first_form() {
    let (vault, _store, _index) = make_vault();

    // Same target `projects/a` three times with different labels, and an
    // embed of `b` before a plain link to it: only the first rendered form
    // of each target survives, and an embed keeps its `!` when it is first.
    let outcome = vault
        .note_to_daily(
            at(10, 0),
            "Dedup entry",
            "[[projects/a|x)y]] then [[projects/a|other]] and [[projects/a]]; ![[b]] and [[b]].",
        )
        .unwrap();

    assert_eq!(
        outcome.log_line,
        "noted [[journal/2026/daily/2026-09-27#Dedup entry]] ([[projects/a|x)y]] ![[b]])"
    );
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
    // Pins that the call writes the daily note exactly **once**.
    // `Vault::new` on an empty store performs no writes, so the store is
    // armed to fail the second write: a single-write `note_to_daily`
    // never reaches it and must succeed with both halves present. Were
    // the pointer staged as a separate write, that second write would
    // fail and the call would error — which this test forbids.
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
    assert!(
        result.is_ok(),
        "one write only, so the armed second write is never reached: {result:?}"
    );
    assert!(has_entry && has_pointer, "{content}");

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

// ── Rulings from the #647 review panel ──

#[test]
fn heading_with_inline_markup_is_refused() {
    let (vault, store, _index) = make_vault();

    for raw in ["**Bold**", "`Code`", "*Notes*", "Tom &amp; Jerry", "a\\*b"] {
        let err = vault.note_to_daily(at(10, 0), raw, "body").unwrap_err();
        match err {
            DomainError::HistoryEntryHeadingInvalid {
                ref heading,
                ref reason,
                ..
            } => {
                assert_eq!(heading, raw);
                assert!(reason.contains("inline markup"), "{reason}");
            }
            other => panic!("expected HistoryEntryHeadingInvalid, got {other:?}"),
        }
    }
    assert!(!store.exists(&vp(DAILY)).unwrap(), "nothing written");
}

#[test]
fn heading_starting_with_caret_is_refused() {
    let (vault, store, _index) = make_vault();

    let err = vault.note_to_daily(at(10, 0), "^ref", "body").unwrap_err();

    assert_heading_refused(err, "^ref");
    assert!(!store.exists(&vp(DAILY)).unwrap(), "nothing written");
}

#[test]
fn canonical_collision_with_an_earlier_entry_is_refused() {
    // `Foo` then a `### `Foo`` heading inside a later body: stripped,
    // both are `Foo`, so the second would make `section("Foo")`
    // ambiguous and orphan the first entry's pointer.
    let (vault, store, _index) = make_vault();
    vault.note_to_daily(at(10, 0), "Foo", "a").unwrap();
    let before = read(&store);

    let err = vault
        .note_to_daily(at(11, 0), "Other", "b\n\n### `Foo`\nc")
        .unwrap_err();

    assert_heading_refused(err, "Foo");
    assert_eq!(read(&store), before, "file must be byte-identical");
    MarkdownDocument::parse(before)
        .unwrap()
        .section("Foo")
        .expect("the first entry is still addressable");
}

#[test]
fn markup_heading_in_body_naming_a_section_is_refused_as_heading_invalid() {
    let (vault, store, _index) = make_vault();
    vault.log_to_daily_note(at(9, 0), "started").unwrap();
    let before = read(&store);

    for body in [
        "x\n\n### *Notes*\ny",
        "x\n\n### *Logs*\ny",
        "x\n\n### `Standup`\ny",
    ] {
        let err = vault.note_to_daily(at(10, 0), "Entry", body).unwrap_err();
        match err {
            DomainError::HistoryEntryHeadingInvalid { ref reason, .. } => {
                assert_eq!(reason, "it is the name of a daily section", "{body}");
            }
            other => panic!("expected HistoryEntryHeadingInvalid for {body:?}, got {other:?}"),
        }
    }
    assert_eq!(read(&store), before, "nothing written");
}

#[test]
fn setext_heading_in_body_is_refused_as_heading_invalid() {
    let (vault, store, _index) = make_vault();
    vault.log_to_daily_note(at(9, 0), "started").unwrap();
    let before = read(&store);

    let err = vault
        .note_to_daily(at(10, 0), "Entry", "Logs\n----\nmore")
        .unwrap_err();

    match err {
        DomainError::HistoryEntryHeadingInvalid {
            ref heading,
            ref reason,
            ..
        } => {
            assert_eq!(heading, "Logs");
            assert!(reason.contains("level-3"), "{reason}");
        }
        other => panic!("expected HistoryEntryHeadingInvalid, got {other:?}"),
    }
    assert_eq!(read(&store), before, "nothing written");
}

#[test]
fn code_fence_with_hash_comment_is_accepted() {
    let (vault, store, _index) = make_vault();

    vault
        .note_to_daily(
            at(10, 0),
            "Reindex procedure",
            "Run it like this:\n\n```bash\n# rebuild the index\ncdno reindex\n```\n",
        )
        .expect("a `#` line inside a fence is not a heading");

    let content = read(&store);
    assert!(content.contains("# rebuild the index"), "{content}");
    MarkdownDocument::parse(content)
        .unwrap()
        .section("Reindex procedure")
        .expect("the entry is addressable");
}

#[test]
fn duplicate_heading_inside_one_body_is_refused() {
    let (vault, store, _index) = make_vault();

    // Two equal sub-headings in the body.
    let err = vault
        .note_to_daily(at(10, 0), "Entry", "a\n\n### Step\nb\n\n### STEP\nc")
        .unwrap_err();
    assert_heading_refused(err, "STEP");

    // The entry heading repeated inside its own body.
    let err = vault
        .note_to_daily(at(10, 0), "Entry", "text\n\n### Entry\nmore")
        .unwrap_err();
    assert_heading_refused(err, "Entry");

    assert!(!store.exists(&vp(DAILY)).unwrap(), "nothing written");
}

#[test]
fn heading_uniqueness_folds_non_ascii_case() {
    let (vault, store, _index) = make_vault();
    vault.note_to_daily(at(10, 0), "Été", "a").unwrap();
    let before = read(&store);

    let err = vault.note_to_daily(at(11, 0), "ÉTÉ", "b").unwrap_err();

    assert_heading_refused(err, "ÉTÉ");
    assert_eq!(read(&store), before, "nothing written");
}

#[test]
fn upsert_daily_section_validates_notes_under_the_write_lock() {
    // `validate_history_entry` reads the daily note to test for
    // duplicates; that read must come after the transaction's write
    // lock is taken, or a concurrent writer could slip a duplicate in
    // between the check and the write.
    let backing = Arc::new(MemoryVaultStore::new());
    let recording = Arc::new(RecordingStore::new(backing));
    let store: Arc<dyn VaultStore> = recording.clone();
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    let date = at(10, 0).date();
    vault
        .upsert_daily_section(date, DailySection::Notes, "### First\na", true)
        .unwrap();
    recording.clear();

    vault
        .upsert_daily_section(date, DailySection::Notes, "### Second\nb", true)
        .unwrap();

    let events = recording.events();
    let lock = events
        .iter()
        .position(|e| e == "lock")
        .expect("the write lock is taken");
    let first_daily_read = events
        .iter()
        .position(|e| e == &format!("read:{DAILY}"))
        .expect("the daily note is read");
    assert!(
        lock < first_daily_read,
        "the note was read before the lock was taken: {events:?}"
    );
}

#[test]
fn note_to_daily_validates_under_the_write_lock() {
    let backing = Arc::new(MemoryVaultStore::new());
    let recording = Arc::new(RecordingStore::new(backing));
    let store: Arc<dyn VaultStore> = recording.clone();
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    vault.note_to_daily(at(10, 0), "First", "a").unwrap();
    recording.clear();

    vault.note_to_daily(at(11, 0), "Second", "b").unwrap();

    let events = recording.events();
    let lock = events.iter().position(|e| e == "lock").expect("lock taken");
    let first_daily_read = events
        .iter()
        .position(|e| e == &format!("read:{DAILY}"))
        .expect("the daily note is read");
    assert!(lock < first_daily_read, "{events:?}");
}

/// A [`VaultStore`] that records, in order, each write-lock acquisition
/// (`lock`) and each `read_file`/`exists` call (`read:<path>`), so a test
/// can check that a read happens under the lock.
struct RecordingStore {
    inner: Arc<MemoryVaultStore>,
    events: Mutex<Vec<String>>,
}

impl RecordingStore {
    fn new(inner: Arc<MemoryVaultStore>) -> Self {
        Self {
            inner,
            events: Mutex::new(Vec::new()),
        }
    }

    fn record(&self, event: String) {
        self.events.lock().unwrap().push(event);
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    fn clear(&self) {
        self.events.lock().unwrap().clear();
    }
}

impl VaultStore for RecordingStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        self.record(format!("read:{path}"));
        self.inner.read_file(path)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        self.inner.read_bytes(path)
    }
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.write_file(path, content)
    }
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.append_to_file(path, content)
    }
    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.move_file(src, dest)
    }
    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError> {
        self.inner.delete_file(path)
    }
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        self.record(format!("read:{path}"));
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
        self.inner.import_external(src, dest)
    }
    fn acquire_write_lock(&self) -> Result<VaultWriteLock, StoreError> {
        self.record("lock".to_string());
        self.inner.acquire_write_lock()
    }
}
