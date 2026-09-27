//! Tests for `Vault::revise_note` (RFC 0002 §5.3, §6.2): the logged,
//! hash-guarded in-place edit for mutable custom notes.

use std::sync::{Arc, Mutex};

use cdno_core::config::{CustomNoteType, VaultConfig};
use cdno_core::error::{ManipulationError, StoreError};
use cdno_core::file_meta::FileMeta;
use cdno_core::hash::content_hash;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore, VaultWriteLock};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::vault::Revision;
use chrono::{NaiveDate, NaiveDateTime};

const CONCEPT_PATH: &str = "concepts/woodbury-identity.md";
const DAILY_PATH: &str = "journal/2026/daily/2026-09-27.md";

const FRONTMATTER: &str = "---\ntype: concept\ncreated: 2026-09-01\ntags: [linear-algebra]\norigin: \"[[journal/2026/daily/2026-09-01]]\"\n---\n";

const BODY: &str = "\n# Woodbury identity\n\n## Statement\n\nThe inverse of a rank-k correction.\n\n## Why it matters\n\nCheap updates.\n\n## See also\n\n- [[concepts/sherman-morrison]]\n";

fn concept_note() -> String {
    format!("{FRONTMATTER}{BODY}")
}

const PERSON_NOTE: &str =
    "---\ntype: person\ncreated: 2026-09-01\n---\n\n# Ada\n\n## Log\n\nMet.\n";

const ACTIVE_PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Foo\n\n## Current State\nGoing.\n\n## Next Actions\n";

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn at() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 9, 27)
        .unwrap()
        .and_hms_opt(14, 32, 0)
        .unwrap()
}

fn config() -> VaultConfig {
    let mut config = VaultConfig::default();
    config.note_types.insert(
        "concept".to_owned(),
        CustomNoteType {
            folder: "concepts".to_owned(),
            required: vec!["created".to_owned()],
            optional: vec!["tags".to_owned(), "origin".to_owned()],
            template: None,
            append_only: false,
            title_field: None,
            date_field: None,
        },
    );
    config.note_types.insert(
        "person".to_owned(),
        CustomNoteType {
            folder: "people".to_owned(),
            required: vec!["created".to_owned()],
            optional: vec![],
            template: None,
            append_only: true,
            title_field: None,
            date_field: None,
        },
    );
    config
}

fn seed(store: &dyn VaultStore) {
    store
        .write_file(&vp(CONCEPT_PATH), &concept_note())
        .unwrap();
    store.write_file(&vp("people/ada.md"), PERSON_NOTE).unwrap();
    store
        .write_file(&vp("projects/foo.md"), ACTIVE_PROJECT)
        .unwrap();
}

fn vault() -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    seed(&*store);
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(Arc::clone(&store), index, config()).expect("Vault::new");
    (vault, store)
}

/// The `## Logs` lines of today's daily note that record a revision.
fn revised_lines(store: &dyn VaultStore) -> Vec<String> {
    match store.read_file(&vp(DAILY_PATH)) {
        Ok(daily) => daily
            .lines()
            .filter(|l| l.contains("revised [["))
            .map(str::to_owned)
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn body_revision_logs_one_revised_line() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();
    let new_body = "\n# Woodbury identity\n\n## Statement\n\nA better account.\n";

    let outcome = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Body(new_body.to_owned()),
            "rewrote the\n  statement",
            at(),
        )
        .expect("revise");

    let written = store.read_file(&path).unwrap();
    // Frontmatter preserved byte-for-byte; the body is exactly what was given.
    assert_eq!(written, format!("{FRONTMATTER}{new_body}"));
    assert!(outcome.changed);
    assert_eq!(outcome.new_hash, content_hash(&written));
    assert_eq!(
        outcome.log_line.as_deref(),
        Some("revised [[concepts/woodbury-identity]] \u{2014} rewrote the statement")
    );
    assert_eq!(
        revised_lines(&*store),
        vec![
            "- **14:32**: revised [[concepts/woodbury-identity]] \u{2014} rewrote the statement"
                .to_owned()
        ]
    );
    // The hash `read_note` now reports is the one the outcome returned.
    assert_eq!(
        vault.read_note(&path).unwrap().content_hash,
        outcome.new_hash
    );
}

#[test]
fn section_revision_logs_the_anchored_form() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();

    let outcome = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Section {
                heading: "Why it matters".to_owned(),
                content: "Cheap updates: O(k^3) instead of O(n^3).".to_owned(),
            },
            "added the cost comparison",
            at(),
        )
        .expect("revise");

    let written = store.read_file(&path).unwrap();
    assert_eq!(
        written,
        concept_note().replace(
            "Cheap updates.\n",
            "Cheap updates: O(k^3) instead of O(n^3).\n"
        )
    );
    assert!(outcome.changed);
    assert_eq!(
        revised_lines(&*store),
        vec![
            "- **14:32**: revised [[concepts/woodbury-identity#Why it matters]] \u{2014} added the cost comparison"
                .to_owned()
        ]
    );
}

#[test]
fn identical_revision_writes_nothing_and_logs_nothing() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();

    let body_outcome = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Body(view.body.clone()),
            "no change",
            at(),
        )
        .expect("revise body");
    let section_outcome = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Section {
                heading: "Statement".to_owned(),
                content: "The inverse of a rank-k correction.".to_owned(),
            },
            "no change",
            at(),
        )
        .expect("revise section");

    for outcome in [body_outcome, section_outcome] {
        assert!(!outcome.changed);
        assert_eq!(outcome.log_line, None);
        assert_eq!(outcome.new_hash, view.content_hash);
    }
    assert_eq!(store.read_file(&path).unwrap(), concept_note());
    assert!(
        !store.exists(&vp(DAILY_PATH)).unwrap(),
        "an identical revision must not create or touch the daily note"
    );
}

#[test]
fn stale_hash_is_refused_and_file_is_untouched() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();

    // An editor saves in between.
    let edited = concept_note().replace("Cheap updates.", "Edited elsewhere.");
    store.write_file(&path, &edited).unwrap();

    let err = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Body("\n# Clobber\n".to_owned()),
            "would lose the edit",
            at(),
        )
        .unwrap_err();

    match err {
        DomainError::StaleRevision {
            path: p,
            expected,
            actual,
        } => {
            assert_eq!(p, CONCEPT_PATH);
            assert_eq!(expected, view.content_hash);
            assert_eq!(actual, content_hash(&edited));
        }
        other => panic!("expected StaleRevision, got {other:?}"),
    }
    assert_eq!(store.read_file(&path).unwrap(), edited);
    assert!(revised_lines(&*store).is_empty());
}

#[test]
fn none_hash_skips_the_check() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    let edited = concept_note().replace("Cheap updates.", "Edited elsewhere.");
    store.write_file(&path, &edited).unwrap();

    let outcome = vault
        .revise_note(
            &path,
            None,
            Revision::Section {
                heading: "Statement".to_owned(),
                content: "Restated.".to_owned(),
            },
            "restated",
            at(),
        )
        .expect("revise without a hash");

    assert!(outcome.changed);
    let written = store.read_file(&path).unwrap();
    assert!(written.contains("Restated."), "{written}");
    assert!(written.contains("Edited elsewhere."), "{written}");
    assert_eq!(revised_lines(&*store).len(), 1);
}

#[test]
fn builtin_type_is_refused() {
    let (vault, store) = vault();
    let path = vp("projects/foo.md");
    let err = vault
        .revise_note(
            &path,
            None,
            Revision::Section {
                heading: "Current State".to_owned(),
                content: "Bypassing the state log.".to_owned(),
            },
            "sneaky",
            at(),
        )
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::NoteNotRevisable { path, .. } if path == "projects/foo.md"),
        "got {err:?}"
    );
    assert_eq!(store.read_file(&path).unwrap(), ACTIVE_PROJECT);
    assert!(!store.exists(&vp(DAILY_PATH)).unwrap());
}

#[test]
fn append_only_custom_type_is_refused() {
    let (vault, store) = vault();
    let path = vp("people/ada.md");
    let err = vault
        .revise_note(
            &path,
            None,
            Revision::Body("\n# Ada\n".to_owned()),
            "rewrite history",
            at(),
        )
        .unwrap_err();
    assert!(
        matches!(&err, DomainError::NoteNotRevisable { reason, .. } if reason.contains("append-only")),
        "got {err:?}"
    );
    assert_eq!(store.read_file(&path).unwrap(), PERSON_NOTE);
    assert!(!store.exists(&vp(DAILY_PATH)).unwrap());
}

#[test]
fn empty_reason_is_refused() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    for reason in ["", "  \n\t "] {
        let err = vault
            .revise_note(
                &path,
                None,
                Revision::Body("\n# Changed\n".to_owned()),
                reason,
                at(),
            )
            .unwrap_err();
        assert!(
            matches!(err, DomainError::EmptyField { field: "reason" }),
            "got {err:?}"
        );
    }
    assert_eq!(store.read_file(&path).unwrap(), concept_note());
    assert!(!store.exists(&vp(DAILY_PATH)).unwrap());
}

#[test]
fn missing_section_is_refused_and_nothing_is_written() {
    let (vault, store) = vault();
    let path = vp(CONCEPT_PATH);
    let err = vault
        .revise_note(
            &path,
            None,
            Revision::Section {
                heading: "Proof".to_owned(),
                content: "By expansion.".to_owned(),
            },
            "added a proof",
            at(),
        )
        .unwrap_err();
    assert!(
        matches!(
            &err,
            DomainError::Manipulation(ManipulationError::SectionNotFound(h)) if h == "Proof"
        ),
        "got {err:?}"
    );
    assert_eq!(store.read_file(&path).unwrap(), concept_note());
    assert!(!store.exists(&vp(DAILY_PATH)).unwrap());
}

#[test]
fn revision_and_log_line_are_one_commit() {
    // The commit writes the note first, then the daily note. Failing
    // the second write must roll the note back.
    let backing = Arc::new(MemoryVaultStore::new());
    seed(&*backing);
    let store: Arc<dyn VaultStore> = Arc::new(FailingStore::new(Arc::clone(&backing), 2));
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(Arc::clone(&store), index, config()).expect("Vault::new");

    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();
    let err = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Body("\n# Woodbury identity\n\nShorter.\n".to_owned()),
            "shortened",
            at(),
        )
        .unwrap_err();
    assert!(matches!(err, DomainError::Transaction(_)), "got {err:?}");
    assert_eq!(backing.read_file(&path).unwrap(), concept_note());
    assert!(!backing.exists(&vp(DAILY_PATH)).unwrap());
}

#[test]
fn hash_compare_happens_under_the_lock() {
    // Simulates an editor whose save lands after the caller read the
    // note but before `revise_note` takes the vault write lock: the
    // store performs the edit at the moment the lock is acquired. With
    // the read + compare under the lock, the revision sees the edited
    // bytes and is refused. Were the compare done before taking the
    // lock, it would pass on the stale bytes and the revision would
    // overwrite the editor's save.
    let backing = Arc::new(MemoryVaultStore::new());
    seed(&*backing);
    let racing = Arc::new(EditorRaceStore::new(Arc::clone(&backing)));
    let store: Arc<dyn VaultStore> = racing.clone();
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(Arc::clone(&store), index, config()).expect("Vault::new");

    let path = vp(CONCEPT_PATH);
    let view = vault.read_note(&path).unwrap();
    let edited = concept_note().replace("Cheap updates.", "Saved by the editor.");
    racing.save_on_next_lock(path.clone(), edited.clone());

    let err = vault
        .revise_note(
            &path,
            Some(&view.content_hash),
            Revision::Section {
                heading: "Statement".to_owned(),
                content: "Restated.".to_owned(),
            },
            "restated",
            at(),
        )
        .unwrap_err();
    assert!(
        matches!(err, DomainError::StaleRevision { .. }),
        "got {err:?}"
    );
    assert_eq!(backing.read_file(&path).unwrap(), edited);
    assert!(!backing.exists(&vp(DAILY_PATH)).unwrap());
}

/// A store that performs one pending write the moment the vault write
/// lock is next acquired — a concurrent editor's save racing the
/// caller's read-then-revise.
struct EditorRaceStore {
    inner: Arc<MemoryVaultStore>,
    pending: Mutex<Option<(VaultPath, String)>>,
}

impl EditorRaceStore {
    fn new(inner: Arc<MemoryVaultStore>) -> Self {
        Self {
            inner,
            pending: Mutex::new(None),
        }
    }

    fn save_on_next_lock(&self, path: VaultPath, content: String) {
        *self.pending.lock().unwrap() = Some((path, content));
    }
}

impl VaultStore for EditorRaceStore {
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
        self.inner.import_external(src, dest)
    }
    fn acquire_write_lock(&self) -> Result<VaultWriteLock, StoreError> {
        if let Some((path, content)) = self.pending.lock().unwrap().take() {
            self.inner.write_file(&path, &content)?;
        }
        self.inner.acquire_write_lock()
    }
}

/// Wraps a `MemoryVaultStore`, failing the Nth write/append/move/delete.
/// Copied from `actions_tests.rs` (no shared test support module exists
/// on `main`). Reads, `exists` and walks never fail or count.
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
