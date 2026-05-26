//! Unit tests for the heavy-form action lifecycle:
//! `Vault::add_action_with_note` (birth) and `complete_action`'s
//! archival of an attached note (death). `MemoryVaultStore` /
//! `MemoryIndex` keep the suite fast and deterministic.

use std::sync::{Arc, Mutex};

use cdno_core::config::VaultConfig;
use cdno_core::error::StoreError;
use cdno_core::file_meta::FileMeta;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{ActionFrontmatter, ActionStatus, EnergyLevel};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

/// Minimal active project with an empty `## Next Actions` section.
const ACTIVE_PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Foo\n\n## Current State\nGoing.\n\n## Next Actions\n";

fn vault_with(notes: &[(&str, &str)]) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store.write_file(&vp(path), body).unwrap();
    }
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    (vault, store)
}

fn read_action_frontmatter(store: &Arc<dyn VaultStore>, path: &VaultPath) -> ActionFrontmatter {
    let raw = store.read_file(path).unwrap();
    let (fm, _body) = Frontmatter::parse(&raw).unwrap();
    ActionFrontmatter::try_from(fm).unwrap()
}

// ---------------------------------------------------------------------
// Birth: add_action_with_note
// ---------------------------------------------------------------------

#[test]
fn add_action_with_note_creates_note_and_wikilinked_bullet() {
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);

    let note_path = vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Characterise sample efficiency",
            EnergyLevel::Deep,
        )
        .expect("add succeeds");

    assert_eq!(note_path, vp("actions/characterise-sample-efficiency.md"));

    // The note exists with active frontmatter pinned to the project.
    let fm = read_action_frontmatter(&store, &note_path);
    assert_eq!(fm.status, ActionStatus::Active);
    assert_eq!(fm.project, "foo");
    assert_eq!(fm.energy, EnergyLevel::Deep);
    assert_eq!(fm.created, NaiveDate::from_ymd_opt(2026, 5, 26).unwrap());
    assert!(fm.completed.is_none());
    assert!(fm.milestone.is_none());
    assert!(fm.due.is_none());

    // The project bullet wikilinks the note rather than carrying text.
    let project = store.read_file(&vp("projects/foo.md")).unwrap();
    assert!(
        project.contains("- [ ] [[actions/characterise-sample-efficiency]] (deep)"),
        "project body:\n{project}"
    );

    // And the addition is logged once to the daily note.
    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-26.md"))
        .expect("daily note exists");
    assert!(daily.contains("[[actions/characterise-sample-efficiency]]"));
}

#[test]
fn add_action_with_note_on_parked_project_errors_and_writes_nothing() {
    let parked = "---\ntype: project\ncontext: work\nstatus: parked\ncreated: 2026-04-01\n---\n\n# Foo\n\n## Next Actions\n";
    let (vault, store) = vault_with(&[("projects/_parked/foo.md", parked)]);

    let err = vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Some work",
            EnergyLevel::Light,
        )
        .unwrap_err();
    assert!(
        matches!(err, DomainError::ProjectNotActive(_)),
        "got {err:?}"
    );

    // No note file leaked from the aborted operation.
    assert!(!store.exists(&vp("actions/some-work.md")).unwrap());
}

// ---------------------------------------------------------------------
// Death: complete_action archives an attached note
// ---------------------------------------------------------------------

#[test]
fn complete_action_archives_attached_note() {
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Characterise sample efficiency",
            EnergyLevel::Deep,
        )
        .unwrap();

    vault
        .complete_action(dt(2026, 5, 27, 17, 0), "foo", "characterise")
        .expect("complete succeeds");

    // The active note is gone; the archived copy lives under _done/<year>/.
    assert!(
        !store
            .exists(&vp("actions/characterise-sample-efficiency.md"))
            .unwrap()
    );
    let done = vp("actions/_done/2026/characterise-sample-efficiency.md");
    assert!(store.exists(&done).unwrap(), "archived note should exist");

    let fm = read_action_frontmatter(&store, &done);
    assert_eq!(fm.status, ActionStatus::Completed);
    assert_eq!(
        fm.completed,
        Some(NaiveDate::from_ymd_opt(2026, 5, 27).unwrap())
    );

    // The bullet is removed from the project.
    let project = store.read_file(&vp("projects/foo.md")).unwrap();
    assert!(
        !project.contains("[[actions/characterise-sample-efficiency]]"),
        "bullet should be gone:\n{project}"
    );
}

#[test]
fn complete_action_on_plain_bullet_is_unchanged() {
    // Regression: a plain (non-wikilink) action completes exactly as
    // before — bullet removed, no action note, no _done folder.
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Write the tests",
            EnergyLevel::Light,
        )
        .unwrap();

    vault
        .complete_action(dt(2026, 5, 27, 17, 0), "foo", "write the tests")
        .expect("complete succeeds");

    let project = store.read_file(&vp("projects/foo.md")).unwrap();
    assert!(
        !project.contains("Write the tests"),
        "bullet removed:\n{project}"
    );
    // No action note machinery kicked in for a plain bullet.
    assert!(!store.exists(&vp("actions/write-the-tests.md")).unwrap());
    assert!(
        !store
            .exists(&vp("actions/_done/2026/write-the-tests.md"))
            .unwrap(),
        "plain completion must not touch _done",
    );
}

// ---------------------------------------------------------------------
// Atomicity: rollback on a mid-transaction write failure
// ---------------------------------------------------------------------

#[test]
fn add_action_with_note_rolls_back_on_write_failure() {
    // The commit writes the action note first, then the project. A
    // FailingStore set to fail on the 2nd write trips the project
    // write, so the already-written note must be rolled back, leaving
    // both files in their original state.
    let backing = Arc::new(MemoryVaultStore::new());
    backing
        .write_file(&vp("projects/foo.md"), ACTIVE_PROJECT)
        .unwrap();

    let store: Arc<dyn VaultStore> = Arc::new(FailingStore::new(backing.clone(), 2));
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");

    let err = vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Characterise sample efficiency",
            EnergyLevel::Deep,
        )
        .unwrap_err();
    assert!(matches!(err, DomainError::Transaction(_)), "got {err:?}");

    // Note write was rolled back (file deleted)...
    assert!(
        !backing
            .exists(&vp("actions/characterise-sample-efficiency.md"))
            .unwrap(),
        "rolled-back note must not linger",
    );
    // ...and the project is untouched.
    assert_eq!(
        backing.read_file(&vp("projects/foo.md")).unwrap(),
        ACTIVE_PROJECT,
    );
}

/// Wraps a `MemoryVaultStore`, failing the Nth write/append/move/delete
/// so the transaction rollback path can be exercised at the domain
/// level. Reads, `exists`, and directory walks never fail or count, so
/// `Vault::new` reconciliation runs cleanly before the counter matters.
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
}
