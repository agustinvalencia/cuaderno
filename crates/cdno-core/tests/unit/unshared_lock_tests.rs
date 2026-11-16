//! Compare before rename (#1001): two writers over one vault folder whose
//! write locks are not shared, as when one folder is written from two
//! kernels and `flock` is not seen across them.
//!
//! Each test gives every store its own lock file
//! ([`FsVaultStore::with_lock_file`]), so both writers hold "the" lock at
//! once, exactly as on a bind mount, and drives the interleaving by hand.
//! The single-machine behaviour (shared lock, rollback, `IndexStale`) is
//! pinned at the end and in `transaction_tests`.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use cdno_core::error::{StoreError, TransactionError};
use cdno_core::file_meta::FileMeta;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::read_journal::{Observed, ReadJournals};
use cdno_core::store::{FsVaultStore, MemoryVaultStore, VaultStore, VaultWriteLock};
use cdno_core::transaction::VaultTransaction;
use tempfile::TempDir;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// A vault folder and the directory the writers' private lock files live
/// in, kept outside the vault so nothing in it is a note.
struct Folder {
    vault: TempDir,
    locks: TempDir,
}

impl Folder {
    fn new() -> Self {
        let vault = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(vault.path().join(".cuaderno")).unwrap();
        Self {
            vault,
            locks: tempfile::tempdir().unwrap(),
        }
    }

    fn root(&self) -> &Path {
        self.vault.path()
    }

    /// A store over the folder with its own lock file `name`: two stores
    /// with different names do not exclude each other.
    fn writer(&self, name: &str) -> Arc<FsVaultStore> {
        Arc::new(FsVaultStore::new(self.root()).with_lock_file(self.locks.path().join(name)))
    }

    fn put(&self, path: &str, content: &str) {
        let full = self.root().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }

    fn get(&self, path: &str) -> String {
        std::fs::read_to_string(self.root().join(path)).unwrap()
    }

    fn present(&self, path: &str) -> bool {
        self.root().join(path).exists()
    }

    /// Every file under the folder whose name marks it as a store temp
    /// file; a refused write must leave none behind.
    fn temp_files(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![self.root().to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    stack.push(entry.path());
                } else {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with(cdno_core::store::WIP_TEMP_PREFIX) {
                        out.push(name);
                    }
                }
            }
        }
        out
    }
}

fn tx(store: &Arc<FsVaultStore>) -> VaultTransaction {
    VaultTransaction::new(
        store.clone() as Arc<dyn VaultStore>,
        Arc::new(MemoryIndex::new()) as Arc<dyn VaultIndex>,
    )
    .expect("an independent lock is always free")
}

fn conflict_paths(err: TransactionError) -> Vec<String> {
    match err {
        TransactionError::Conflict { paths } => paths.iter().map(ToString::to_string).collect(),
        other => panic!("expected a conflict, got {other:?}"),
    }
}

const NOTE: &str = "journal/2026/daily/2026-06-13.md";

// ---------------------------------------------------------------------
// The lost update, refused
// ---------------------------------------------------------------------

#[test]
fn independent_lock_files_do_not_exclude_each_other() {
    // The seam the rest of this file relies on: two transactions over one
    // folder are open at the same moment.
    let folder = Folder::new();
    let a = folder.writer("a.lock");
    let b = folder.writer("b.lock");
    let _one = tx(&a);
    let _two = tx(&b);
}

#[test]
fn the_second_of_two_interleaved_read_modify_writes_is_refused() {
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    let a = folder.writer("a.lock");
    let b = folder.writer("b.lock");

    // Both writers read the same base while each holds its own lock.
    let mut first = tx(&a);
    let read_a = a.read_file(&vp(NOTE)).unwrap();
    let mut second = tx(&b);
    let read_b = b.read_file(&vp(NOTE)).unwrap();

    first.write_file(vp(NOTE), format!("{read_a}line from a\n"));
    first.commit().expect("the first writer commits");

    second.write_file(vp(NOTE), format!("{read_b}line from b\n"));
    let err = second.commit().unwrap_err();
    assert_eq!(conflict_paths(err), vec![NOTE.to_owned()]);

    // The first writer's change survives; the second wrote nothing.
    assert_eq!(folder.get(NOTE), "base\nline from a\n");
    assert!(folder.temp_files().is_empty(), "{:?}", folder.temp_files());
}

#[test]
fn the_conflict_message_says_nothing_was_written_and_to_run_again() {
    let err = TransactionError::Conflict {
        paths: vec![vp(NOTE)],
    };
    let message = err.to_string();
    assert!(message.contains(NOTE), "{message}");
    assert!(message.contains("changed on disk"), "{message}");
    assert!(message.contains("nothing was written"), "{message}");
    assert!(message.contains("run the operation again"), "{message}");
}

#[test]
fn a_retry_after_the_conflict_keeps_both_changes() {
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    let a = folder.writer("a.lock");
    let b = folder.writer("b.lock");

    let mut first = tx(&a);
    let read_a = a.read_file(&vp(NOTE)).unwrap();
    let mut second = tx(&b);
    let read_b = b.read_file(&vp(NOTE)).unwrap();
    first.write_file(vp(NOTE), format!("{read_a}a\n"));
    first.commit().unwrap();
    second.write_file(vp(NOTE), format!("{read_b}b\n"));
    assert!(matches!(
        second.commit(),
        Err(TransactionError::Conflict { .. })
    ));

    // The caller runs its operation again: read, modify, write.
    let mut again = tx(&b);
    let fresh = b.read_file(&vp(NOTE)).unwrap();
    again.write_file(vp(NOTE), format!("{fresh}b\n"));
    again
        .commit()
        .expect("the retry sees the first writer's change");
    assert_eq!(folder.get(NOTE), "base\na\nb\n");
}

#[test]
fn a_note_created_by_another_writer_is_not_overwritten() {
    // The operation saw no file (an `exists` that answered false) and
    // decided to create one; another writer created it first.
    let folder = Folder::new();
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    assert!(!a.exists(&vp(NOTE)).unwrap());
    folder.put(NOTE, "made elsewhere\n");
    t.write_file(vp(NOTE), "scaffold\n");
    assert_eq!(conflict_paths(t.commit().unwrap_err()), vec![NOTE]);
    assert_eq!(folder.get(NOTE), "made elsewhere\n");
}

#[test]
fn a_note_removed_by_another_writer_is_not_recreated() {
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    let read = a.read_file(&vp(NOTE)).unwrap();
    std::fs::remove_file(folder.root().join(NOTE)).unwrap();
    t.write_file(vp(NOTE), format!("{read}more\n"));
    assert_eq!(conflict_paths(t.commit().unwrap_err()), vec![NOTE]);
    assert!(!folder.present(NOTE));
}

#[test]
fn an_append_onto_a_changed_file_is_refused() {
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    let _ = a.read_file(&vp(NOTE)).unwrap();
    folder.put(NOTE, "base\nforeign\n");
    t.append_to_file(vp(NOTE), "mine\n");
    assert_eq!(conflict_paths(t.commit().unwrap_err()), vec![NOTE]);
    assert_eq!(folder.get(NOTE), "base\nforeign\n");
}

#[test]
fn a_delete_of_a_changed_file_is_refused() {
    let folder = Folder::new();
    folder.put("inbox/item.md", "one\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    let _ = a.read_file(&vp("inbox/item.md")).unwrap();
    folder.put("inbox/item.md", "one, edited elsewhere\n");
    t.delete_file(vp("inbox/item.md"));
    assert_eq!(
        conflict_paths(t.commit().unwrap_err()),
        vec!["inbox/item.md"]
    );
    assert_eq!(folder.get("inbox/item.md"), "one, edited elsewhere\n");
}

#[test]
fn a_move_carries_a_changed_file_along_rather_than_refusing() {
    // A bare move (an attachment following its folder) discards nothing:
    // the other writer's change travels with the file. It is not compared,
    // so it never reads an attachment's bytes.
    let folder = Folder::new();
    folder.put("portfolios/old/scan.pdf", "%PDF one\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    t.move_file(vp("portfolios/old/scan.pdf"), vp("portfolios/new/scan.pdf"));
    folder.put("portfolios/old/scan.pdf", "%PDF two\n");
    t.commit().unwrap();
    assert_eq!(folder.get("portfolios/new/scan.pdf"), "%PDF two\n");
    assert!(!folder.present("portfolios/old/scan.pdf"));
}

#[test]
fn a_note_moved_by_rewriting_it_elsewhere_is_compared_at_its_source() {
    // Notes move as a write of the new path and a delete of the old one,
    // so a change to the old note refuses the pair.
    let folder = Folder::new();
    folder.put("projects/alpha.md", "map\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    let map = a.read_file(&vp("projects/alpha.md")).unwrap();
    folder.put("projects/alpha.md", "map, edited elsewhere\n");
    assert!(!a.exists(&vp("projects/_parked/alpha.md")).unwrap());
    t.write_file(vp("projects/_parked/alpha.md"), map);
    t.delete_file(vp("projects/alpha.md"));
    assert_eq!(
        conflict_paths(t.commit().unwrap_err()),
        vec!["projects/alpha.md"]
    );
    assert_eq!(folder.get("projects/alpha.md"), "map, edited elsewhere\n");
    assert!(!folder.present("projects/_parked/alpha.md"));
}

#[test]
fn a_write_staged_without_a_read_is_checked_from_its_staging() {
    // A verb that never read the file it overwrites does not skip the
    // check: the observation is taken when the write is staged.
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    t.write_file(vp(NOTE), "blind\n");
    folder.put(NOTE, "base\nforeign\n");
    assert_eq!(conflict_paths(t.commit().unwrap_err()), vec![NOTE]);
    assert_eq!(folder.get(NOTE), "base\nforeign\n");
}

#[test]
fn a_read_made_before_the_transaction_opened_is_not_what_is_compared() {
    // Reads are recorded only while a transaction is open; a read made
    // before the lock was taken is no baseline (the #196 rule: open the
    // transaction before reading). The staging observation stands in.
    let folder = Folder::new();
    folder.put(NOTE, "old\n");
    let a = folder.writer("a.lock");
    let _ = a.read_file(&vp(NOTE)).unwrap();
    folder.put(NOTE, "new\n");
    let mut t = tx(&a);
    t.write_file(vp(NOTE), "replaced\n");
    t.commit()
        .expect("compared from the staging, which saw `new`");
    assert_eq!(folder.get(NOTE), "replaced\n");
}

// ---------------------------------------------------------------------
// All or nothing across the files of one commit
// ---------------------------------------------------------------------

#[test]
fn a_conflict_on_any_file_writes_none_of_them() {
    let folder = Folder::new();
    folder.put("projects/alpha.md", "map\n");
    folder.put(NOTE, "log\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    let map = a.read_file(&vp("projects/alpha.md")).unwrap();
    let log = a.read_file(&vp(NOTE)).unwrap();
    // The second file changes underneath; the first did not.
    folder.put(NOTE, "log\nforeign\n");
    t.write_file(vp("projects/alpha.md"), format!("{map}state changed\n"));
    t.write_file(vp(NOTE), format!("{log}state on [[alpha]]\n"));
    t.write_file(vp("projects/new.md"), "created\n");
    assert_eq!(conflict_paths(t.commit().unwrap_err()), vec![NOTE]);

    assert_eq!(folder.get("projects/alpha.md"), "map\n");
    assert_eq!(folder.get(NOTE), "log\nforeign\n");
    assert!(!folder.present("projects/new.md"));
}

#[test]
fn every_changed_file_is_named() {
    let folder = Folder::new();
    folder.put("a.md", "a\n");
    folder.put("b.md", "b\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    let _ = a.read_file(&vp("a.md")).unwrap();
    let _ = a.read_file(&vp("b.md")).unwrap();
    folder.put("a.md", "a2\n");
    folder.put("b.md", "b2\n");
    t.write_file(vp("a.md"), "mine\n");
    t.delete_file(vp("b.md"));
    assert_eq!(
        conflict_paths(t.commit().unwrap_err()),
        vec!["a.md", "b.md"]
    );
}

#[test]
fn a_file_the_commit_rewrites_twice_is_compared_once_against_what_was_read() {
    // The second op on a path is compared with the first op's result, not
    // with the read: a commit's own earlier op is no conflict.
    let folder = Folder::new();
    folder.put("a.md", "a\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    let _ = a.read_file(&vp("a.md")).unwrap();
    t.write_file(vp("a.md"), "first\n");
    t.write_file(vp("a.md"), "second\n");
    t.move_file(vp("a.md"), vp("moved/a.md"));
    t.write_file(vp("moved/a.md"), "third\n");
    t.commit().unwrap();
    assert!(!folder.present("a.md"));
    assert_eq!(folder.get("moved/a.md"), "third\n");
}

/// A store over a real folder that lets a test change a file at a precise
/// point inside the commit: right after the `nth` conditional write
/// lands, `interfere` runs. That is a change in the window between the
/// commit's first check and a later op's check.
struct Interfering {
    inner: FsVaultStore,
    writes: AtomicUsize,
    nth: usize,
    interfere: Box<dyn Fn() + Send + Sync>,
}

impl VaultStore for Interfering {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        self.inner.read_file(path)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        self.inner.read_bytes(path)
    }
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.write_file(path, content)
    }
    fn write_file_expecting(
        &self,
        path: &VaultPath,
        content: &str,
        expected: Observed,
    ) -> Result<(), StoreError> {
        self.inner.write_file_expecting(path, content, expected)?;
        if self.writes.fetch_add(1, Ordering::SeqCst) + 1 == self.nth {
            (self.interfere)();
        }
        Ok(())
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
    fn import_external(&self, src: &Path, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.import_external(src, dest)
    }
    fn acquire_write_lock(&self) -> Result<VaultWriteLock, StoreError> {
        self.inner.acquire_write_lock()
    }
    fn read_journals(&self) -> Option<&ReadJournals> {
        self.inner.read_journals()
    }
    fn append_to_file_expecting(
        &self,
        path: &VaultPath,
        content: &str,
        expected: cdno_core::read_journal::Observed,
    ) -> Result<(), StoreError> {
        self.inner.append_to_file_expecting(path, content, expected)
    }
}

fn interfering(
    folder: &Folder,
    nth: usize,
    interfere: impl Fn() + Send + Sync + 'static,
) -> Arc<Interfering> {
    Arc::new(Interfering {
        inner: FsVaultStore::new(folder.root()).with_lock_file(folder.locks.path().join("i.lock")),
        writes: AtomicUsize::new(0),
        nth,
        interfere: Box::new(interfere),
    })
}

fn tx_on(store: &Arc<Interfering>) -> VaultTransaction {
    VaultTransaction::new(
        store.clone() as Arc<dyn VaultStore>,
        Arc::new(MemoryIndex::new()) as Arc<dyn VaultIndex>,
    )
    .unwrap()
}

#[test]
fn a_change_landing_mid_commit_rolls_back_the_files_already_written() {
    let folder = Folder::new();
    folder.put("a.md", "a\n");
    folder.put("b.md", "b\n");
    let root = folder.root().to_path_buf();
    // Right after `a.md` is written, another writer changes `b.md`.
    let store = interfering(&folder, 1, move || {
        std::fs::write(root.join("b.md"), "b, from elsewhere\n").unwrap();
    });
    let mut t = tx_on(&store);
    let _ = store.read_file(&vp("a.md")).unwrap();
    let _ = store.read_file(&vp("b.md")).unwrap();
    t.write_file(vp("a.md"), "a, mine\n");
    t.write_file(vp("b.md"), "b, mine\n");

    assert_eq!(conflict_paths(t.commit().unwrap_err()), vec!["b.md"]);
    assert_eq!(
        folder.get("a.md"),
        "a\n",
        "the applied write is rolled back"
    );
    assert_eq!(folder.get("b.md"), "b, from elsewhere\n");
}

#[test]
fn a_rollback_does_not_overwrite_a_change_made_after_its_write() {
    // Another writer changes `a.md` after this commit wrote it, and `b.md`
    // before this commit reaches it. Restoring `a.md` would discard that
    // writer's change, so the rollback leaves it and says so.
    let folder = Folder::new();
    folder.put("a.md", "a\n");
    folder.put("b.md", "b\n");
    let root = folder.root().to_path_buf();
    let store = interfering(&folder, 1, move || {
        std::fs::write(root.join("a.md"), "a, from elsewhere\n").unwrap();
        std::fs::write(root.join("b.md"), "b, from elsewhere\n").unwrap();
    });
    let mut t = tx_on(&store);
    let _ = store.read_file(&vp("a.md")).unwrap();
    let _ = store.read_file(&vp("b.md")).unwrap();
    t.write_file(vp("a.md"), "a, mine\n");
    t.write_file(vp("b.md"), "b, mine\n");

    match t.commit().unwrap_err() {
        TransactionError::FileWrite {
            source,
            rollback_failures,
        } => {
            assert!(
                matches!(&source, StoreError::Changed(p) if p.to_string() == "b.md"),
                "{source:?}"
            );
            assert!(
                matches!(rollback_failures.as_slice(), [StoreError::Changed(p)] if p.to_string() == "a.md"),
                "{rollback_failures:?}"
            );
        }
        other => panic!("expected a file write failure, got {other:?}"),
    }
    assert_eq!(folder.get("a.md"), "a, from elsewhere\n");
    assert_eq!(folder.get("b.md"), "b, from elsewhere\n");
}

// ---------------------------------------------------------------------
// The store's conditional write
// ---------------------------------------------------------------------

#[test]
fn a_conditional_write_refuses_a_changed_file_and_leaves_no_temp() {
    let folder = Folder::new();
    folder.put("a.md", "a\n");
    let store = FsVaultStore::new(folder.root());
    let stale = Observed::of_bytes(b"something else\n");
    let err = store
        .write_file_expecting(&vp("a.md"), "mine\n", stale)
        .unwrap_err();
    assert!(
        matches!(&err, StoreError::Changed(p) if p.to_string() == "a.md"),
        "{err:?}"
    );
    assert_eq!(folder.get("a.md"), "a\n");
    assert!(folder.temp_files().is_empty());

    store
        .write_file_expecting(&vp("a.md"), "mine\n", Observed::of_bytes(b"a\n"))
        .unwrap();
    assert_eq!(folder.get("a.md"), "mine\n");
    store
        .write_file_expecting(&vp("new.md"), "new\n", Observed::Absent)
        .unwrap();
    assert_eq!(folder.get("new.md"), "new\n");
}

#[test]
fn a_conditional_append_refuses_a_changed_file() {
    let folder = Folder::new();
    folder.put("a.md", "a\n");
    let store = FsVaultStore::new(folder.root());
    let err = store
        .append_to_file_expecting(&vp("a.md"), "b\n", Observed::Absent)
        .unwrap_err();
    assert!(matches!(err, StoreError::Changed(_)));
    assert_eq!(folder.get("a.md"), "a\n");
    store
        .append_to_file_expecting(&vp("a.md"), "b\n", Observed::of_bytes(b"a\n"))
        .unwrap();
    assert_eq!(folder.get("a.md"), "a\nb\n");
}

// ---------------------------------------------------------------------
// The read journal
// ---------------------------------------------------------------------

#[test]
fn a_journal_records_only_while_open_and_keeps_the_first_observation() {
    let folder = Folder::new();
    folder.put("a.md", "one\n");
    let store = FsVaultStore::new(folder.root());
    let journals = store.read_journals().expect("the filesystem store records");

    let _ = store.read_file(&vp("a.md")).unwrap();
    let journal = journals.open();
    assert_eq!(journal.get(&vp("a.md")), None, "a read before opening");

    let _ = store.read_file(&vp("a.md")).unwrap();
    folder.put("a.md", "two\n");
    let _ = store.read_bytes(&vp("a.md")).unwrap();
    assert_eq!(
        journal.get(&vp("a.md")),
        Some(Observed::of_bytes(b"one\n")),
        "the first observation wins"
    );
    assert!(!store.exists(&vp("gone.md")).unwrap());
    assert_eq!(journal.get(&vp("gone.md")), Some(Observed::Absent));

    drop(journal);
    let reopened = journals.open();
    let _ = store.exists(&vp("a.md")).unwrap();
    assert_eq!(
        reopened.get(&vp("a.md")),
        None,
        "an `exists` that answers true observes no content"
    );
}

// ---------------------------------------------------------------------
// Creates, and where the store compares
// ---------------------------------------------------------------------

#[test]
fn a_conditional_create_never_replaces_an_existing_file() {
    // The no-clobber rename itself, not the commit's earlier comparison:
    // swapping it for a plain rename fails this test.
    let folder = Folder::new();
    folder.put("a.md", "made elsewhere\n");
    let store = FsVaultStore::new(folder.root());
    let err = store
        .write_file_expecting(&vp("a.md"), "mine\n", Observed::Absent)
        .unwrap_err();
    assert!(
        matches!(&err, StoreError::Changed(p) if *p == vp("a.md")),
        "{err:?}"
    );
    assert_eq!(folder.get("a.md"), "made elsewhere\n");
    let err = store
        .append_to_file_expecting(&vp("a.md"), "mine\n", Observed::Absent)
        .unwrap_err();
    assert!(matches!(err, StoreError::Changed(_)), "{err:?}");
    assert_eq!(folder.get("a.md"), "made elsewhere\n");
    assert!(folder.temp_files().is_empty(), "{:?}", folder.temp_files());
}

#[test]
fn a_file_created_while_the_create_is_staged_is_kept() {
    // The other writer creates the file after the temp file is synced and
    // before the rename: only the no-clobber rename can refuse it.
    let folder = Folder::new();
    let root = folder.root().to_path_buf();
    let store = FsVaultStore::new(folder.root()).with_before_compare_hook(move || {
        std::fs::write(root.join("a.md"), "made elsewhere\n").unwrap();
    });
    let err = store
        .write_file_expecting(&vp("a.md"), "mine\n", Observed::Absent)
        .unwrap_err();
    assert!(matches!(err, StoreError::Changed(_)), "{err:?}");
    assert_eq!(folder.get("a.md"), "made elsewhere\n");
    assert!(folder.temp_files().is_empty());
}

#[test]
fn the_replace_comparison_runs_after_the_temp_file_is_synced() {
    // A change landing after the slow part of the write (temp file written
    // and synced) is still seen: the comparison is the last step before the
    // rename. A comparison moved ahead of the temp write fails this test.
    let folder = Folder::new();
    folder.put("a.md", "base\n");
    let root = folder.root().to_path_buf();
    let store = FsVaultStore::new(folder.root()).with_before_compare_hook(move || {
        std::fs::write(root.join("a.md"), "base\nforeign\n").unwrap();
    });
    let err = store
        .write_file_expecting(&vp("a.md"), "mine\n", Observed::of_bytes(b"base\n"))
        .unwrap_err();
    assert!(matches!(err, StoreError::Changed(_)), "{err:?}");
    assert_eq!(folder.get("a.md"), "base\nforeign\n");
    assert!(folder.temp_files().is_empty());
}

#[cfg(unix)]
#[test]
fn a_filesystem_without_no_clobber_renames_falls_back_to_check_then_rename() {
    let folder = Folder::new();
    folder.put("taken.md", "made elsewhere\n");
    let store = FsVaultStore::new(folder.root()).with_noclobber_error(libc::ENOTSUP);
    store
        .write_file_expecting(&vp("new.md"), "mine\n", Observed::Absent)
        .expect("absent: the fallback creates it");
    assert_eq!(folder.get("new.md"), "mine\n");
    let err = store
        .write_file_expecting(&vp("taken.md"), "mine\n", Observed::Absent)
        .unwrap_err();
    assert!(matches!(err, StoreError::Changed(_)), "{err:?}");
    assert_eq!(folder.get("taken.md"), "made elsewhere\n");
    assert!(folder.temp_files().is_empty());
}

#[cfg(unix)]
#[test]
fn any_other_no_clobber_failure_is_reported_not_retried_without_it() {
    let folder = Folder::new();
    let store = FsVaultStore::new(folder.root()).with_noclobber_error(libc::EIO);
    let err = store
        .write_file_expecting(&vp("new.md"), "mine\n", Observed::Absent)
        .unwrap_err();
    assert!(matches!(err, StoreError::Io { .. }), "{err:?}");
    assert!(!folder.present("new.md"));
    assert!(folder.temp_files().is_empty());
}

#[test]
fn create_file_refuses_a_path_the_operation_saw_taken() {
    // A create states it expects the path absent, whatever was read: a
    // path chosen as free that is not free fails instead of being replaced.
    let folder = Folder::new();
    folder.put("inbox/item.md", "made elsewhere\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    let _ = a.read_file(&vp("inbox/item.md")).unwrap();
    t.create_file(vp("inbox/item.md"), "mine\n");
    assert_eq!(
        conflict_paths(t.commit().unwrap_err()),
        vec!["inbox/item.md"]
    );
    assert_eq!(folder.get("inbox/item.md"), "made elsewhere\n");
}

#[test]
fn create_file_refuses_a_file_that_appears_after_staging() {
    let folder = Folder::new();
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    t.create_file(vp("inbox/item.md"), "mine\n");
    folder.put("inbox/item.md", "made elsewhere\n");
    assert_eq!(
        conflict_paths(t.commit().unwrap_err()),
        vec!["inbox/item.md"]
    );
    assert_eq!(folder.get("inbox/item.md"), "made elsewhere\n");
}

#[test]
fn a_move_of_a_note_the_operation_read_is_compared() {
    let folder = Folder::new();
    folder.put("projects/alpha.md", "map\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    let _ = a.read_file(&vp("projects/alpha.md")).unwrap();
    folder.put("projects/alpha.md", "map, edited elsewhere\n");
    t.move_file(vp("projects/alpha.md"), vp("projects/_parked/alpha.md"));
    assert_eq!(
        conflict_paths(t.commit().unwrap_err()),
        vec!["projects/alpha.md"]
    );
    assert_eq!(folder.get("projects/alpha.md"), "map, edited elsewhere\n");
    assert!(!folder.present("projects/_parked/alpha.md"));
}

// ---------------------------------------------------------------------
// Whose reads count, and reads that disagree
// ---------------------------------------------------------------------

#[test]
fn reads_on_another_thread_do_not_supply_the_baseline() {
    // Another thread of the process (a read-only call beside the write)
    // reads the note after another writer changed it. Its read must not
    // become the baseline: the operation composed from the older bytes.
    let folder = Folder::new();
    folder.put(NOTE, "old\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    let journals = a.read_journals().unwrap();
    // Recorded on this thread only: a journal opened here sees nothing of
    // another thread's read.
    let probe = journals.open();
    folder.put(NOTE, "new\n");
    let other = Arc::clone(&a);
    std::thread::spawn(move || {
        let _ = other.read_file(&vp(NOTE)).unwrap();
    })
    .join()
    .unwrap();
    assert_eq!(
        probe.get(&vp(NOTE)),
        None,
        "another thread's read was recorded"
    );
    drop(probe);

    // The operation's own (later) read is the baseline, so nothing changed
    // between it and the commit: the write lands.
    let read = a.read_file(&vp(NOTE)).unwrap();
    t.write_file(vp(NOTE), format!("{read}mine\n"));
    t.commit().unwrap();
    assert_eq!(folder.get(NOTE), "new\nmine\n");
}

#[test]
fn a_note_changed_and_changed_back_between_two_reads_is_refused() {
    // The operation read the note twice and the two reads disagree: it may
    // have composed from either, so a file that holds the first again by
    // commit time is no proof of anything. The commit refuses.
    let folder = Folder::new();
    folder.put(NOTE, "one\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a);
    let _first = a.read_file(&vp(NOTE)).unwrap();
    folder.put(NOTE, "two\n");
    let second = a.read_file(&vp(NOTE)).unwrap();
    folder.put(NOTE, "one\n");
    t.write_file(vp(NOTE), format!("{second}mine\n"));
    assert_eq!(conflict_paths(t.commit().unwrap_err()), vec![NOTE]);
    assert_eq!(folder.get(NOTE), "one\n");
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "without having been read inside the transaction")]
fn a_transaction_that_requires_reads_refuses_a_blind_replace_in_debug_builds() {
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a).require_reads();
    t.write_file(vp(NOTE), "blind\n");
}

#[test]
fn a_transaction_that_requires_reads_accepts_a_read_replace_and_a_new_file() {
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    let a = folder.writer("a.lock");
    let mut t = tx(&a).require_reads();
    let read = a.read_file(&vp(NOTE)).unwrap();
    t.write_file(vp(NOTE), format!("{read}more\n"));
    t.write_file(vp("inbox/new.md"), "fresh\n");
    t.create_file(vp("inbox/other.md"), "fresh\n");
    t.commit().unwrap();
    assert_eq!(folder.get(NOTE), "base\nmore\n");
}

// ---------------------------------------------------------------------
// Many writers, retrying
// ---------------------------------------------------------------------

/// Two writers that do not exclude each other each append `lines` lines to
/// [`NOTE`] through `store_for(name)`, running the read-modify-write again
/// whenever the commit reports a conflict. Returns the lines that did not
/// land exactly once, and how many conflicts were retried.
fn race_two_appenders<S>(
    lines: usize,
    store_for: impl Fn(&str) -> Arc<S>,
) -> (Vec<String>, usize, String)
where
    S: VaultStore + 'static,
{
    let conflicts = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(AtomicBool::new(false));
    let reader = store_for("reader");
    let handles: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|name| {
            let store = store_for(name);
            let conflicts = Arc::clone(&conflicts);
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                while !start.load(Ordering::SeqCst) {
                    std::hint::spin_loop();
                }
                for n in 0..lines {
                    loop {
                        let mut t = VaultTransaction::new(
                            store.clone() as Arc<dyn VaultStore>,
                            Arc::new(MemoryIndex::new()) as Arc<dyn VaultIndex>,
                        )
                        .unwrap();
                        let read = store.read_file(&vp(NOTE)).unwrap();
                        t.write_file(vp(NOTE), format!("{read}{name} {n}\n"));
                        match t.commit() {
                            Ok(_) => break,
                            Err(TransactionError::Conflict { .. }) => {
                                conflicts.fetch_add(1, Ordering::SeqCst);
                            }
                            Err(other) => panic!("{other:?}"),
                        }
                    }
                }
            })
        })
        .collect();
    start.store(true, Ordering::SeqCst);
    for h in handles {
        h.join().unwrap();
    }
    let note = reader.read_file(&vp(NOTE)).unwrap();
    let on_disk: Vec<&str> = note.lines().collect();
    let lost = ["a", "b"]
        .into_iter()
        .flat_map(|name| (0..lines).map(move |n| format!("{name} {n}")))
        .filter(|want| on_disk.iter().filter(|l| **l == want.as_str()).count() != 1)
        .collect();
    (lost, conflicts.load(Ordering::SeqCst), note)
}

#[test]
fn two_writers_appending_without_a_shared_lock_lose_nothing_when_they_retry() {
    // One in-memory store shared by two writers stands in for one folder
    // written from two kernels: its write lock is a no-op, so the two
    // read-modify-writes interleave freely, while its conditional write is
    // one step under its mutex. That isolates the protocol from the
    // filesystem's rename gap (the next test), so the result is exact:
    // every line lands once. Without the commit's check this loses about
    // half the lines.
    const LINES: usize = 200;
    let shared = Arc::new(MemoryVaultStore::new());
    shared.write_file(&vp(NOTE), "").unwrap();
    let (lost, conflicts, note) = race_two_appenders(LINES, |_| Arc::clone(&shared));
    assert!(
        lost.is_empty(),
        "{} line(s) lost or doubled after {conflicts} retried conflicts: {lost:?}",
        lost.len()
    );
    assert_eq!(note.lines().count(), 2 * LINES);
}

/// The same race on a real folder, two stores with independent lock files,
/// as a measurement of the window that remains (docs/concurrent-writers.md).
///
/// Ignored in the normal suite: its outcome depends on scheduling, and a
/// test that can fail by chance costs more in CI than it proves. The exact
/// protocol is pinned by the in-memory race above and the hand-interleaved
/// real-folder tests. Run it with
///
/// ```text
/// cargo test -p cdno-core --test unit -- --ignored on_a_real_folder --nocapture
/// ```
///
/// Every commit here contends. The check runs a `stat` before the rename,
/// and no rename can be conditional on what it replaces, so a writer that
/// renames inside that gap is still overwritten. Measured on a laptop SSD
/// (macOS, APFS, debug build) over a dozen runs: 0 to 2 of 300 lines lost,
/// against about 150 of 300 with the check disabled.
///
/// That figure is not representative and is no guarantee. On macOS
/// `sync_all` is a full device flush (`F_FULLFSYNC`), which serialises the
/// two writers into alternation, an accidental shared lock: with a plain
/// `fsync` the same protocol lost 38 to 52 of 300 in a standalone
/// reproduction, and with a short delay before the rename nearly every
/// contended line. So this test passes or fails on the sync cost of the
/// machine it runs on; the assertion is only a loose bound. The figure that
/// counts, two kernels over a VM file share, is in the design note.
#[test]
#[ignore = "measures a scheduling-dependent window; run by hand, see the doc comment"]
fn on_a_real_folder_only_the_gap_before_the_rename_remains() {
    const LINES: usize = 150;
    let folder = Folder::new();
    folder.put(NOTE, "");
    let (lost, conflicts, _) =
        race_two_appenders(LINES, |name| folder.writer(&format!("{name}.lock")));
    eprintln!(
        "rename gap: {} of {} lines lost, {conflicts} conflicts retried",
        lost.len(),
        2 * LINES
    );
    assert!(
        conflicts > 0,
        "the writers never overlapped; nothing was tested"
    );
    assert!(
        lost.len() * 10 < 2 * LINES,
        "{} of {} lines lost after {conflicts} retried conflicts",
        lost.len(),
        2 * LINES
    );
}

// ---------------------------------------------------------------------
// One machine: behaviour unchanged
// ---------------------------------------------------------------------

#[test]
fn ordinary_commits_under_the_shared_lock_are_unchanged() {
    let folder = Folder::new();
    folder.put("a.md", "a\n");
    folder.put("b.md", "b\n");
    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(folder.root()));
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());

    for round in 0..3 {
        let mut t = VaultTransaction::new(store.clone(), index.clone()).unwrap();
        let a = store.read_file(&vp("a.md")).unwrap();
        t.write_file(vp("a.md"), format!("{a}{round}\n"));
        t.append_to_file(vp("b.md"), format!("{round}\n"));
        t.write_file(vp(&format!("new-{round}.md")), "fresh\n");
        t.commit().unwrap();
    }
    assert_eq!(folder.get("a.md"), "a\n0\n1\n2\n");
    assert_eq!(folder.get("b.md"), "b\n0\n1\n2\n");

    let mut t = VaultTransaction::new(store.clone(), index.clone()).unwrap();
    t.move_file(vp("new-0.md"), vp("moved/new-0.md"));
    t.delete_file(vp("new-1.md"));
    let touched = t.commit().unwrap();
    assert_eq!(
        touched,
        vec![vp("new-0.md"), vp("moved/new-0.md"), vp("new-1.md")]
    );
    assert_eq!(folder.get("moved/new-0.md"), "fresh\n");
    assert!(!folder.present("new-1.md"));
}
