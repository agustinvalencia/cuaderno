//! Writers the vault write lock does not exclude (#1001), through the real
//! domain verbs.
//!
//! Two `Vault`s over one shared in-memory store stand in for one folder
//! written from two kernels: the in-memory store's write lock is a no-op,
//! so their read-modify-writes interleave freely, while its conditional
//! write is atomic, which keeps the result exact (the filesystem's
//! remaining rename gap is measured in `cdno-core`'s `unshared_lock_tests`).
//! The deterministic tests inject the other writer's change at the moment
//! the operation reads the note.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use cdno_core::config::VaultConfig;
use cdno_core::error::StoreError;
use cdno_core::file_meta::FileMeta;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::read_journal::ReadJournals;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::vault::DailySection;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

const DAILY: &str = "journal/2026/daily/2026-06-13.md";

fn at() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 6, 13)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap())
}

fn vault_over(store: Arc<dyn VaultStore>) -> Vault {
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    Vault::new(store, index, VaultConfig::default())
        .expect("construct vault")
        .0
}

/// How many lines of `note` end with ` <text>`.
fn count(note: &str, text: &str) -> usize {
    let suffix = format!(" {text}");
    note.lines().filter(|l| l.ends_with(&suffix)).count()
}

#[test]
fn two_vaults_logging_to_one_daily_note_lose_no_lines() {
    const LINES: usize = 60;
    let shared = Arc::new(MemoryVaultStore::new());
    let vaults: Vec<Vault> = (0..2)
        .map(|_| vault_over(shared.clone() as Arc<dyn VaultStore>))
        .collect();
    let start = Arc::new(AtomicBool::new(false));
    let refused = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = vaults
        .into_iter()
        .zip(["a", "b"])
        .map(|(vault, name)| {
            let start = Arc::clone(&start);
            let refused = Arc::clone(&refused);
            std::thread::spawn(move || {
                while !start.load(Ordering::SeqCst) {
                    std::hint::spin_loop();
                }
                for n in 0..LINES {
                    // The caller's side of the contract: a concurrent
                    // change wrote nothing, so the same call is retried.
                    loop {
                        match vault.log_to_daily_note(at(), &format!("writer {name} line {n}")) {
                            Ok(_) => break,
                            Err(e) if e.is_concurrent_change() => {
                                refused.fetch_add(1, Ordering::SeqCst);
                            }
                            Err(e) => panic!("{e}"),
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

    let note = shared.read_file(&VaultPath::new(DAILY).unwrap()).unwrap();
    for name in ["a", "b"] {
        for n in 0..LINES {
            let line = format!("writer {name} line {n}");
            assert_eq!(
                count(&note, &line),
                1,
                "{line:?} lost or doubled ({} calls refused after the domain's own retries):\n{note}",
                refused.load(Ordering::SeqCst)
            );
        }
    }
}

/// When [`OtherWriter`] changes the note.
#[derive(Clone, Copy, PartialEq)]
enum Interference {
    Off,
    /// After every read of the note until the first commit compares it,
    /// so exactly one attempt composes from stale content.
    FirstAttempt,
    /// After every read of the note, so no attempt can succeed.
    Always,
}

/// A store in which another writer appends a line to `target` right after
/// each read of it returns (per [`Interference`]), so the operation
/// composes its write from content that is already stale. The commit's
/// comparison reads through `read_bytes`, which is how `FirstAttempt`
/// knows the first attempt has reached its commit.
struct OtherWriter {
    inner: Arc<MemoryVaultStore>,
    target: VaultPath,
    mode: std::sync::Mutex<Interference>,
    fired: AtomicUsize,
    /// How many times a commit compared the note: one per attempt.
    compared: AtomicUsize,
}

impl OtherWriter {
    fn new(inner: Arc<MemoryVaultStore>, target: &str) -> Self {
        Self {
            inner,
            target: VaultPath::new(target).unwrap(),
            mode: std::sync::Mutex::new(Interference::Off),
            fired: AtomicUsize::new(0),
            compared: AtomicUsize::new(0),
        }
    }

    fn arm(&self, mode: Interference) {
        *self.mode.lock().unwrap() = mode;
    }

    fn fired(&self) -> usize {
        self.fired.load(Ordering::SeqCst)
    }
}

impl VaultStore for OtherWriter {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        let read = self.inner.read_file(path)?;
        if *path == self.target && *self.mode.lock().unwrap() != Interference::Off {
            let n = self.fired.fetch_add(1, Ordering::SeqCst);
            self.inner
                .write_file(path, &format!("{read}other writer {n}\n"))?;
        }
        Ok(read)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        if *path == self.target {
            let mut mode = self.mode.lock().unwrap();
            if *mode != Interference::Off {
                self.compared.fetch_add(1, Ordering::SeqCst);
            }
            if *mode == Interference::FirstAttempt {
                *mode = Interference::Off;
            }
        }
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
    fn read_journals(&self) -> Option<&ReadJournals> {
        self.inner.read_journals()
    }
    fn write_file_expecting(
        &self,
        path: &VaultPath,
        content: &str,
        expected: cdno_core::read_journal::Observed,
    ) -> Result<(), StoreError> {
        self.inner.write_file_expecting(path, content, expected)
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

/// A vault over an [`OtherWriter`] whose daily note for [`at`] exists.
fn vault_with_other_writer() -> (Vault, Arc<OtherWriter>, Arc<MemoryVaultStore>) {
    let inner = Arc::new(MemoryVaultStore::new());
    let store = Arc::new(OtherWriter::new(inner.clone(), DAILY));
    let vault = vault_over(store.clone() as Arc<dyn VaultStore>);
    vault.log_to_daily_note(at(), "first").unwrap();
    (vault, store, inner)
}

fn daily(inner: &MemoryVaultStore) -> String {
    inner.read_file(&VaultPath::new(DAILY).unwrap()).unwrap()
}

#[test]
fn a_log_line_composed_from_a_stale_read_is_retried_onto_the_fresh_note() {
    let (vault, other, inner) = vault_with_other_writer();
    other.arm(Interference::FirstAttempt);
    vault.log_to_daily_note(at(), "mine").unwrap();

    assert!(other.fired() >= 1);
    let note = daily(&inner);
    assert_eq!(count(&note, "first"), 1, "{note}");
    assert_eq!(count(&note, "mine"), 1, "{note}");
    for n in 0..other.fired() {
        assert!(
            note.contains(&format!("other writer {n}\n")),
            "the other writer's change survives:\n{note}"
        );
    }
}

#[test]
fn a_log_that_never_converges_returns_the_conflict_and_writes_nothing() {
    let (vault, other, inner) = vault_with_other_writer();
    other.arm(Interference::Always);
    let err = vault.log_to_daily_note(at(), "mine").unwrap_err();

    assert!(err.is_concurrent_change(), "{err:?}");
    assert!(err.to_string().contains(DAILY), "{err}");
    assert!(err.to_string().contains("nothing was written"), "{err}");
    // The bound itself, written out, so a change to it is a change here too
    // (the docs state it).
    assert_eq!(cdno_domain::vault::CONFLICT_ATTEMPTS, 5);
    assert_eq!(
        other.compared.load(Ordering::SeqCst),
        cdno_domain::vault::CONFLICT_ATTEMPTS as usize,
        "one commit per attempt, and no more attempts than the bound"
    );
    let note = daily(&inner);
    assert_eq!(count(&note, "mine"), 0, "{note}");
    for n in 0..other.fired() {
        assert!(note.contains(&format!("other writer {n}\n")), "{note}");
    }
}

#[test]
fn a_section_write_from_a_stale_read_is_refused_not_retried() {
    // Replacing a section is not retried by the domain: its content may
    // have been composed by the caller from what it read, so the caller
    // decides. The other writer's line survives and nothing was written.
    let (vault, other, inner) = vault_with_other_writer();
    let before = daily(&inner);
    other.arm(Interference::FirstAttempt);
    let err = vault
        .upsert_daily_section(
            at().date(),
            DailySection::Intention,
            "Draft the surrogate-model section.",
            false,
        )
        .unwrap_err();

    assert!(err.is_concurrent_change(), "{err:?}");
    let foreign: String = (0..other.fired())
        .map(|n| format!("other writer {n}\n"))
        .collect();
    assert!(other.fired() >= 1);
    assert_eq!(daily(&inner), format!("{before}{foreign}"));
}

/// A store in which another writer creates `target` the first time an
/// operation finds it absent, right after `exists` answered: the path the
/// operation chose as free is no longer free when it writes.
struct CreatesWhenFoundFree {
    inner: Arc<MemoryVaultStore>,
    target: VaultPath,
    done: AtomicBool,
}

impl VaultStore for CreatesWhenFoundFree {
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        let exists = self.inner.exists(path)?;
        if !exists && *path == self.target && !self.done.swap(true, Ordering::SeqCst) {
            self.inner.write_file(path, "made elsewhere\n")?;
        }
        Ok(exists)
    }
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
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
    fn read_journals(&self) -> Option<&ReadJournals> {
        self.inner.read_journals()
    }
    fn write_file_expecting(
        &self,
        path: &VaultPath,
        content: &str,
        expected: cdno_core::read_journal::Observed,
    ) -> Result<(), StoreError> {
        self.inner.write_file_expecting(path, content, expected)
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

fn vault_where_found_free_is_taken(target: &str) -> (Vault, Arc<MemoryVaultStore>) {
    let inner = Arc::new(MemoryVaultStore::new());
    let store = Arc::new(CreatesWhenFoundFree {
        inner: inner.clone(),
        target: VaultPath::new(target).unwrap(),
        done: AtomicBool::new(false),
    });
    (vault_over(store as Arc<dyn VaultStore>), inner)
}

#[test]
fn a_capture_never_replaces_one_made_at_the_same_path_meanwhile() {
    let taken = "inbox/2026-06-13-buy-milk.md";
    let (vault, inner) = vault_where_found_free_is_taken(taken);
    let path = vault.capture_to_inbox(at(), "Buy milk").unwrap();

    // The commit refused the taken path; the retried capture chose the
    // next free name.
    assert_eq!(
        path,
        VaultPath::new("inbox/2026-06-13-buy-milk-2.md").unwrap()
    );
    assert_eq!(
        inner.read_file(&VaultPath::new(taken).unwrap()).unwrap(),
        "made elsewhere\n"
    );
    assert!(inner.read_file(&path).unwrap().contains("Buy milk"));
}

#[test]
fn a_stewardship_never_replaces_one_made_at_the_same_path_meanwhile() {
    use cdno_domain::frontmatter::Context;
    let taken = "stewardships/household.md";
    let (vault, inner) = vault_where_found_free_is_taken(taken);
    let err = vault
        .create_stewardship_flat(at(), "Household", Context::Household)
        .unwrap_err();
    assert!(err.is_concurrent_change(), "{err:?}");
    assert_eq!(
        inner.read_file(&VaultPath::new(taken).unwrap()).unwrap(),
        "made elsewhere\n"
    );
}

#[test]
fn a_discard_logs_the_text_it_deletes_even_if_the_item_was_just_edited() {
    let inner = Arc::new(MemoryVaultStore::new());
    let plain = vault_over(inner.clone() as Arc<dyn VaultStore>);
    let item = plain.capture_to_inbox(at(), "Call the lab").unwrap();
    drop(plain);
    let slug = item
        .as_path()
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let store = Arc::new(OtherWriter::new(inner.clone(), &item.to_string()));
    let vault = vault_over(store.clone() as Arc<dyn VaultStore>);
    store.arm(Interference::FirstAttempt);
    vault.discard_inbox_item(at(), &slug).unwrap();

    assert!(store.fired() >= 1);
    assert!(!inner.exists(&item).unwrap(), "the item is deleted");
    let note = daily(&inner);
    let line = note
        .lines()
        .find(|l| l.contains("discarded"))
        .expect("the discard is logged");
    assert!(line.contains("Call the lab"), "{line}");
    assert!(
        line.contains("other writer 0"),
        "the logged text is the text deleted, edit included: {line}"
    );
}

#[test]
fn a_notes_entry_and_a_section_append_are_retried_onto_the_fresh_note() {
    // Two of the other append-shaped verbs: their entry is applied again to
    // the note as it now is.
    let (vault, other, inner) = vault_with_other_writer();
    other.arm(Interference::FirstAttempt);
    vault
        .note_to_daily(at(), "Surrogate model sweep", "Ran the sweep.")
        .unwrap();
    other.arm(Interference::FirstAttempt);
    vault
        .upsert_daily_section(at().date(), DailySection::Meeting, "Agreed the plan.", true)
        .unwrap();

    let note = daily(&inner);
    assert!(note.contains("### Surrogate model sweep"), "{note}");
    assert!(note.contains("Agreed the plan."), "{note}");
    assert!(other.fired() >= 2);
    for n in 0..other.fired() {
        assert!(note.contains(&format!("other writer {n}\n")), "{note}");
    }
}

#[test]
fn an_action_added_from_a_stale_read_of_its_map_is_retried_onto_the_fresh_map() {
    // A retried verb other than the log: the map changes underneath the
    // first attempt, and the bullet lands on the map as it now is.
    use cdno_domain::frontmatter::EnergyLevel;
    let map = "projects/surrogate-model.md";
    let inner = Arc::new(MemoryVaultStore::new());
    inner
        .write_file(
            &VaultPath::new(map).unwrap(),
            "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-06-01\n---\n\n\
             # Surrogate model\n\n## Current State\nGoing.\n\n## Next Actions\n",
        )
        .unwrap();
    let other = Arc::new(OtherWriter::new(inner.clone(), map));
    let vault = vault_over(other.clone() as Arc<dyn VaultStore>);
    other.arm(Interference::FirstAttempt);
    vault
        .add_action(
            at(),
            "surrogate-model",
            "Run the ablation",
            EnergyLevel::Deep,
        )
        .unwrap();

    assert!(other.fired() >= 1);
    let body = inner.read_file(&VaultPath::new(map).unwrap()).unwrap();
    assert!(body.contains("Run the ablation"), "{body}");
    for n in 0..other.fired() {
        assert!(
            body.contains(&format!("other writer {n}\n")),
            "the other writer's change survives:\n{body}"
        );
    }
}
