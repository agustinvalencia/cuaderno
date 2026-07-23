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
fn add_action_with_note_suffixes_a_duplicate_title() {
    // #225 (the flagship case): two actions with the same title get distinct
    // stems (`email-advisor`, `email-advisor-2`), so when one is later
    // archived to `_done/` its `[[actions/<slug>]]` backlinks stay
    // resolvable — the stem is no longer shared.
    let (vault, _store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    let first = vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Email advisor",
            EnergyLevel::Light,
        )
        .expect("first action");
    let second = vault
        .add_action_with_note(
            dt(2026, 5, 27, 9, 0),
            "foo",
            "Email advisor",
            EnergyLevel::Light,
        )
        .expect("second same-title action suffixes");
    assert_eq!(first, vp("actions/email-advisor.md"));
    assert_eq!(second, vp("actions/email-advisor-2.md"));
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

    let outcome = vault
        .complete_action(dt(2026, 5, 27, 17, 0), "foo", "characterise")
        .expect("complete succeeds");

    // The touched set must carry the archival move's BOTH endpoints — the
    // vanished `actions/<slug>.md` and the new `_done/<year>/<slug>.md` —
    // alongside the project map and the daily-log note. This is the whole
    // point of #315: the desktop layer journals exactly these so the
    // watcher can't echo the archive writes back as external edits.
    assert!(outcome.touched());
    assert_eq!(outcome.primary, vp("projects/foo.md"));
    let touched: std::collections::HashSet<_> = outcome.paths.iter().cloned().collect();
    assert_eq!(
        touched,
        std::collections::HashSet::from([
            vp("projects/foo.md"),
            vp("actions/characterise-sample-efficiency.md"),
            vp("actions/_done/2026/characterise-sample-efficiency.md"),
            vp("journal/2026/daily/2026-05-27.md"),
        ]),
        "touched set is project + archive source + archive dest + daily",
    );

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

    let outcome = vault
        .complete_action(dt(2026, 5, 27, 17, 0), "foo", "write the tests")
        .expect("complete succeeds");

    // A plain bullet has no attached note, so the touched set is just the
    // project map and the daily — no archival endpoints.
    let touched: std::collections::HashSet<_> = outcome.paths.iter().cloned().collect();
    assert_eq!(
        touched,
        std::collections::HashSet::from([
            vp("projects/foo.md"),
            vp("journal/2026/daily/2026-05-27.md"),
        ]),
        "plain-bullet completion touches only project + daily",
    );

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

// ---------------------------------------------------------------------
// promote_action (#111)
// ---------------------------------------------------------------------

#[test]
fn promote_action_attaches_note_and_rewrites_bullet() {
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action(
            dt(2026, 5, 28, 9, 0),
            "foo",
            "Draft the methods section",
            EnergyLevel::Deep,
        )
        .unwrap();

    let note_path = vault
        .promote_action(dt(2026, 5, 28, 10, 0), "foo", "draft the methods")
        .expect("promote succeeds");

    assert_eq!(note_path, vp("actions/draft-the-methods-section.md"));
    // Note frontmatter inherits the project + energy from the bullet.
    let fm = read_action_frontmatter(&store, &note_path);
    assert_eq!(fm.status, ActionStatus::Active);
    assert_eq!(fm.project, "foo");
    assert_eq!(fm.energy, EnergyLevel::Deep);

    // Bullet was rewritten to wikilink the new note; the plain bullet
    // text is gone.
    let project = store.read_file(&vp("projects/foo.md")).unwrap();
    assert!(
        project.contains("- [ ] [[actions/draft-the-methods-section]] (deep)"),
        "project body:\n{project}"
    );
    assert!(
        !project.contains("- [ ] Draft the methods section (deep)"),
        "old plain bullet should be gone:\n{project}"
    );
}

#[test]
fn promote_then_complete_round_trip_archives_the_note() {
    // Promote a plain bullet, then complete it: the same archival path
    // exercised by add_action_with_note + complete_action should kick
    // in for the just-promoted bullet.
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action(
            dt(2026, 5, 28, 9, 0),
            "foo",
            "Draft the methods section",
            EnergyLevel::Deep,
        )
        .unwrap();
    vault
        .promote_action(dt(2026, 5, 28, 10, 0), "foo", "draft the methods")
        .unwrap();

    vault
        .complete_action(dt(2026, 5, 29, 17, 0), "foo", "draft-the-methods")
        .expect("complete succeeds");

    assert!(
        !store
            .exists(&vp("actions/draft-the-methods-section.md"))
            .unwrap(),
        "active note moved",
    );
    let done = vp("actions/_done/2026/draft-the-methods-section.md");
    let fm = read_action_frontmatter(&store, &done);
    assert_eq!(fm.status, ActionStatus::Completed);
}

#[test]
fn promote_action_errors_when_bullet_is_already_wikilinked() {
    // The bullet was already attached via add_action_with_note —
    // promoting it again should refuse rather than creating a second
    // note.
    let (vault, _store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action_with_note(
            dt(2026, 5, 28, 9, 0),
            "foo",
            "Characterise sample efficiency",
            EnergyLevel::Deep,
        )
        .unwrap();

    let err = vault
        .promote_action(dt(2026, 5, 28, 10, 0), "foo", "characterise")
        .unwrap_err();
    assert!(
        matches!(err, DomainError::ActionAlreadyPromoted { .. }),
        "got {err:?}",
    );
}

#[test]
fn promote_action_errors_when_bullet_has_no_energy_suffix() {
    // Hand-edited or migrated project with an unaffixed bullet — the
    // energy isn't a thing we want to guess on promote.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Foo\n\n## Current State\nGoing.\n\n## Next Actions\n- [ ] Plain bullet without a suffix\n";
    let (vault, _store) = vault_with(&[("projects/foo.md", project)]);

    let err = vault
        .promote_action(dt(2026, 5, 28, 10, 0), "foo", "plain bullet")
        .unwrap_err();
    assert!(
        matches!(err, DomainError::BulletMissingEnergy { .. }),
        "got {err:?}",
    );
}

#[test]
fn promote_action_errors_on_ambiguous_match() {
    let (vault, _store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action(
            dt(2026, 5, 28, 9, 0),
            "foo",
            "Draft methods section",
            EnergyLevel::Deep,
        )
        .unwrap();
    vault
        .add_action(
            dt(2026, 5, 28, 9, 5),
            "foo",
            "Draft results section",
            EnergyLevel::Deep,
        )
        .unwrap();

    let err = vault
        .promote_action(dt(2026, 5, 28, 10, 0), "foo", "draft")
        .unwrap_err();
    assert!(
        matches!(err, DomainError::AmbiguousAction { .. }),
        "got {err:?}",
    );
}

// ---------------------------------------------------------------------
// start_action
// ---------------------------------------------------------------------

#[test]
fn start_action_logs_to_daily_note() {
    let (vault, store) = vault_with(&[("projects/alpha.md", ACTIVE_PROJECT)]);

    let daily = vault
        .start_action(dt(2026, 5, 26, 9, 30), "alpha", "Draft the methods section")
        .unwrap();

    let content = store.read_file(&daily).unwrap();
    assert!(
        content.contains("- **09:30**: started [[alpha]] \u{2014} Draft the methods section"),
        "daily note carries the started line: {content}"
    );
}

#[test]
fn start_action_rejects_parked_project_and_blank_action() {
    const PARKED: &str = "---\ntype: project\ncontext: work\nstatus: parked\ncreated: 2026-04-01\n---\n\n# Beta\n\n## Current State\nOn ice.\n";
    let (vault, _store) = vault_with(&[
        ("projects/alpha.md", ACTIVE_PROJECT),
        ("projects/_parked/beta.md", PARKED),
    ]);

    let parked = vault
        .start_action(dt(2026, 5, 26, 9, 30), "beta", "Resume someday")
        .unwrap_err();
    assert!(
        matches!(parked, DomainError::ProjectNotActive(_)),
        "{parked:?}"
    );

    let blank = vault
        .start_action(dt(2026, 5, 26, 9, 30), "alpha", "   ")
        .unwrap_err();
    assert!(matches!(blank, DomainError::EmptyField { .. }), "{blank:?}");
}

#[test]
fn complete_action_accepts_the_query_it_just_handed_out() {
    // Every action the tool creates carries an energy suffix, and both
    // `list_actions` and the daily log carry the bullet verbatim — so the
    // text a caller was just shown IS the suffixed form. Rejecting it made
    // the Today page's Done button (#442) unable to close anything: the
    // needle kept its suffix while every candidate had theirs stripped.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Alpha\n\n## Next Actions\n- [ ] Draft the methods section (deep)\n";
    let (vault, store) = vault_with(&[("projects/alpha.md", project)]);

    vault
        .complete_action(
            dt(2026, 5, 2, 9, 30),
            "alpha",
            "Draft the methods section (deep)",
        )
        .expect("the suffixed query must match the bullet it came from");

    // Completion removes the bullet rather than ticking it.
    let content = store.read_file(&vp("projects/alpha.md")).unwrap();
    assert!(
        !content.contains("Draft the methods section"),
        "the bullet is gone: {content}"
    );
}

#[test]
fn complete_action_still_accepts_a_bare_query() {
    // The suffix-free phrase a person would type by hand keeps working.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Alpha\n\n## Next Actions\n- [ ] Draft the methods section (deep)\n";
    let (vault, store) = vault_with(&[("projects/alpha.md", project)]);

    vault
        .complete_action(dt(2026, 5, 2, 9, 30), "alpha", "methods section")
        .expect("a substring query still matches");

    let content = store.read_file(&vp("projects/alpha.md")).unwrap();
    assert!(
        !content.contains("Draft the methods section"),
        "content: {content}"
    );
}
