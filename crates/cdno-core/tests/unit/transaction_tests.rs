//! Tests for VaultTransaction: happy-path commit, file-write rollback,
//! and post-files index-stale reporting. The index-stale path is
//! exercised via a FailingIndex wrapper that lets a test force the
//! next mutating call to fail.

use std::sync::{Arc, Mutex};

use cdno_core::error::{IndexError, StoreError, TransactionError};
use cdno_core::index::{DeadlineEntry, LinkEntry, MemoryIndex, NoteEntry, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_core::transaction::VaultTransaction;
use serde_json::json;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn sample_note(path: &str, note_type: &str) -> NoteEntry {
    NoteEntry {
        path: vp(path),
        note_type: note_type.to_owned(),
        title: Some(format!("{path} title")),
        content_hash: "hash".to_owned(),
        mtime_ns: 0,
        size: 0,
        frontmatter: json!({}),
        indexed_at_ns: 0,
    }
}

// ---------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------

#[test]
fn empty_transaction_commits_successfully() {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let tx = VaultTransaction::new(store, index);
    assert!(tx.commit().is_ok());
}

#[test]
fn commit_applies_file_and_index_ops_in_order() {
    let store = Arc::new(MemoryVaultStore::new());
    let index = Arc::new(MemoryIndex::new());
    let mut tx = VaultTransaction::new(
        store.clone() as Arc<dyn VaultStore>,
        index.clone() as Arc<dyn VaultIndex>,
    );
    tx.write_file(vp("projects/foo.md"), "initial\n");
    tx.upsert_note(sample_note("projects/foo.md", "project"));
    tx.commit().unwrap();

    assert_eq!(
        store.read_file(&vp("projects/foo.md")).unwrap(),
        "initial\n"
    );
    assert!(
        index
            .find_by_path(&vp("projects/foo.md"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn commit_applies_append_on_top_of_existing_content() {
    let store = Arc::new(MemoryVaultStore::new());
    let index = Arc::new(MemoryIndex::new());
    store.write_file(&vp("log.md"), "line 1\n").unwrap();

    let mut tx = VaultTransaction::new(
        store.clone() as Arc<dyn VaultStore>,
        index.clone() as Arc<dyn VaultIndex>,
    );
    tx.append_to_file(vp("log.md"), "line 2\n");
    tx.commit().unwrap();

    assert_eq!(store.read_file(&vp("log.md")).unwrap(), "line 1\nline 2\n");
}

#[test]
fn commit_applies_move_and_delete() {
    let store = Arc::new(MemoryVaultStore::new());
    let index = Arc::new(MemoryIndex::new());
    store.write_file(&vp("a.md"), "A").unwrap();
    store.write_file(&vp("b.md"), "B").unwrap();

    let mut tx = VaultTransaction::new(
        store.clone() as Arc<dyn VaultStore>,
        index.clone() as Arc<dyn VaultIndex>,
    );
    tx.move_file(vp("a.md"), vp("c.md"));
    tx.delete_file(vp("b.md"));
    tx.commit().unwrap();

    assert_eq!(store.read_file(&vp("c.md")).unwrap(), "A");
    assert!(!store.exists(&vp("a.md")).unwrap());
    assert!(!store.exists(&vp("b.md")).unwrap());
}

// ---------------------------------------------------------------------
// File-write rollback
// ---------------------------------------------------------------------

#[test]
fn file_write_failure_rolls_back_previous_writes() {
    // A second write fails; the first should be undone. Use a
    // FailingStore wrapper configured to fail on the second write.
    let backing = Arc::new(MemoryVaultStore::new());
    backing.write_file(&vp("existing.md"), "original").unwrap();

    let store: Arc<dyn VaultStore> = Arc::new(FailingStore::new(backing.clone(), 2));
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());

    let mut tx = VaultTransaction::new(store, index);
    // Overwrites an existing file — rollback must restore the original.
    tx.write_file(vp("existing.md"), "modified");
    // This will be the 2nd write → triggers the failure.
    tx.write_file(vp("new.md"), "never landed");

    let err = tx.commit().unwrap_err();
    assert!(matches!(err, TransactionError::FileWrite { .. }));

    // Rollback restored the original content.
    assert_eq!(backing.read_file(&vp("existing.md")).unwrap(), "original");
    // The never-landed write shouldn't be on disk.
    assert!(!backing.exists(&vp("new.md")).unwrap());
}

#[test]
fn file_write_failure_rollback_deletes_newly_created_file() {
    let backing = Arc::new(MemoryVaultStore::new());
    let store: Arc<dyn VaultStore> = Arc::new(FailingStore::new(backing.clone(), 2));
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());

    let mut tx = VaultTransaction::new(store, index);
    tx.write_file(vp("new1.md"), "will land then get rolled back");
    tx.write_file(vp("new2.md"), "will never land");

    assert!(tx.commit().is_err());
    // Neither file should exist — new1 rolled back via delete, new2 never applied.
    assert!(!backing.exists(&vp("new1.md")).unwrap());
    assert!(!backing.exists(&vp("new2.md")).unwrap());
}

#[test]
fn append_failure_rolls_back_to_original_length() {
    let backing = Arc::new(MemoryVaultStore::new());
    backing.write_file(&vp("log.md"), "original\n").unwrap();

    let store: Arc<dyn VaultStore> = Arc::new(FailingStore::new(backing.clone(), 2));
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());

    let mut tx = VaultTransaction::new(store, index);
    tx.append_to_file(vp("log.md"), "appended\n"); // applies, then rolls back
    tx.write_file(vp("new.md"), "fails"); // trips the 2nd-op failure

    assert!(tx.commit().is_err());
    assert_eq!(backing.read_file(&vp("log.md")).unwrap(), "original\n");
}

#[test]
fn move_failure_rolls_back_previous_moves() {
    let backing = Arc::new(MemoryVaultStore::new());
    backing.write_file(&vp("a.md"), "A").unwrap();
    backing.write_file(&vp("b.md"), "B").unwrap();

    let store: Arc<dyn VaultStore> = Arc::new(FailingStore::new(backing.clone(), 2));
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());

    let mut tx = VaultTransaction::new(store, index);
    tx.move_file(vp("a.md"), vp("a2.md")); // applies, then gets moved back
    tx.move_file(vp("b.md"), vp("b2.md")); // fails

    assert!(tx.commit().is_err());
    assert_eq!(backing.read_file(&vp("a.md")).unwrap(), "A");
    assert!(!backing.exists(&vp("a2.md")).unwrap());
    assert_eq!(backing.read_file(&vp("b.md")).unwrap(), "B");
}

#[test]
fn first_op_failure_reports_no_rollback_steps() {
    let backing = Arc::new(MemoryVaultStore::new());
    let store: Arc<dyn VaultStore> = Arc::new(FailingStore::new(backing.clone(), 1));
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());

    let mut tx = VaultTransaction::new(store, index);
    tx.write_file(vp("never.md"), "nope");

    let err = tx.commit().unwrap_err();
    match err {
        TransactionError::FileWrite {
            rollback_failures, ..
        } => {
            // No prior ops to roll back.
            assert!(rollback_failures.is_empty());
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(!backing.exists(&vp("never.md")).unwrap());
}

// ---------------------------------------------------------------------
// Index-stale after files succeeded
// ---------------------------------------------------------------------

#[test]
fn index_failure_after_files_succeeded_reports_index_stale() {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let backing_index = Arc::new(MemoryIndex::new());
    let index: Arc<dyn VaultIndex> = Arc::new(FailingIndex::new(
        backing_index.clone(),
        FailPoint::UpsertNote,
    ));

    let mut tx = VaultTransaction::new(store.clone(), index);
    tx.write_file(vp("foo.md"), "landed");
    tx.upsert_note(sample_note("foo.md", "daily"));

    let err = tx.commit().unwrap_err();
    match err {
        TransactionError::IndexStale(errs) => assert_eq!(errs.len(), 1),
        other => panic!("unexpected error: {other:?}"),
    }
    // File landed despite the index failure.
    assert_eq!(store.read_file(&vp("foo.md")).unwrap(), "landed");
    // The upsert was not applied to the backing index.
    assert!(backing_index.find_by_path(&vp("foo.md")).unwrap().is_none());
}

#[test]
fn multiple_index_failures_are_all_collected() {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let backing_index = Arc::new(MemoryIndex::new());
    let index: Arc<dyn VaultIndex> = Arc::new(FailingIndex::new(
        backing_index.clone(),
        FailPoint::AlwaysFail,
    ));

    let mut tx = VaultTransaction::new(store, index);
    tx.upsert_note(sample_note("a.md", "daily"));
    tx.upsert_note(sample_note("b.md", "daily"));
    tx.replace_tags(vp("a.md"), vec!["x".to_owned()]);

    let err = tx.commit().unwrap_err();
    match err {
        TransactionError::IndexStale(errs) => assert_eq!(errs.len(), 3),
        other => panic!("unexpected error: {other:?}"),
    }
}

// ---------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------

/// Wraps a `MemoryVaultStore` and fails on the Nth mutating call
/// (1-indexed). Read-only calls always pass through.
struct FailingStore {
    inner: Arc<MemoryVaultStore>,
    fail_on: usize,
    count: Mutex<usize>,
}

impl FailingStore {
    fn new(inner: Arc<MemoryVaultStore>, fail_on_nth_write: usize) -> Self {
        Self {
            inner,
            fail_on: fail_on_nth_write,
            count: Mutex::new(0),
        }
    }

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
    fn metadata(&self, path: &VaultPath) -> Result<cdno_core::file_meta::FileMeta, StoreError> {
        self.inner.metadata(path)
    }
}

enum FailPoint {
    UpsertNote,
    AlwaysFail,
}

struct FailingIndex {
    inner: Arc<MemoryIndex>,
    mode: FailPoint,
}

impl FailingIndex {
    fn new(inner: Arc<MemoryIndex>, mode: FailPoint) -> Self {
        Self { inner, mode }
    }

    fn should_fail(&self, is_upsert_note: bool) -> bool {
        match self.mode {
            FailPoint::UpsertNote => is_upsert_note,
            FailPoint::AlwaysFail => true,
        }
    }
}

impl VaultIndex for FailingIndex {
    fn upsert_note(&self, entry: &NoteEntry) -> Result<(), IndexError> {
        if self.should_fail(true) {
            return Err(IndexError::Update("forced test failure".to_owned()));
        }
        self.inner.upsert_note(entry)
    }
    fn remove_note(&self, path: &VaultPath) -> Result<(), IndexError> {
        if self.should_fail(false) {
            return Err(IndexError::Update("forced test failure".to_owned()));
        }
        self.inner.remove_note(path)
    }
    fn find_by_path(&self, path: &VaultPath) -> Result<Option<NoteEntry>, IndexError> {
        self.inner.find_by_path(path)
    }
    fn list_by_type(&self, note_type: &str) -> Result<Vec<NoteEntry>, IndexError> {
        self.inner.list_by_type(note_type)
    }
    fn replace_deadlines(
        &self,
        path: &VaultPath,
        deadlines: &[DeadlineEntry],
    ) -> Result<(), IndexError> {
        if self.should_fail(false) {
            return Err(IndexError::Update("forced test failure".to_owned()));
        }
        self.inner.replace_deadlines(path, deadlines)
    }
    fn deadlines_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, DeadlineEntry)>, IndexError> {
        self.inner.deadlines_between(from, to)
    }
    fn replace_links(&self, path: &VaultPath, links: &[LinkEntry]) -> Result<(), IndexError> {
        if self.should_fail(false) {
            return Err(IndexError::Update("forced test failure".to_owned()));
        }
        self.inner.replace_links(path, links)
    }
    fn find_backlinks(&self, path: &VaultPath) -> Result<Vec<VaultPath>, IndexError> {
        self.inner.find_backlinks(path)
    }
    fn find_outgoing_links(&self, path: &VaultPath) -> Result<Vec<LinkEntry>, IndexError> {
        self.inner.find_outgoing_links(path)
    }
    fn replace_tags(&self, path: &VaultPath, tags: &[String]) -> Result<(), IndexError> {
        if self.should_fail(false) {
            return Err(IndexError::Update("forced test failure".to_owned()));
        }
        self.inner.replace_tags(path, tags)
    }
    fn find_by_tag(&self, tag: &str) -> Result<Vec<VaultPath>, IndexError> {
        self.inner.find_by_tag(tag)
    }
}
