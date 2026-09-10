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
use cdno_domain::vault::WriteOutcome;
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

/// An active project map whose `## Next Actions` already holds the
/// given bullets. `ACTIVE_PROJECT` above starts the section empty; the
/// drop tests need plain (unattached) bullets to match against.
fn project_with_bullets(bullets: &str) -> String {
    format!("{ACTIVE_PROJECT}{bullets}")
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
    // The action must exist on the map: a start names a bullet (#568).
    vault
        .add_action(
            dt(2026, 5, 26, 9, 0),
            "alpha",
            "Draft the methods section",
            EnergyLevel::Deep,
        )
        .unwrap();

    let daily = vault
        .start_action(dt(2026, 5, 26, 9, 30), "alpha", "Draft the methods section")
        .unwrap();

    let content = store.read_file(&daily).unwrap();
    assert!(
        content
            .contains("- **09:30**: started [[alpha]] \u{2014} Draft the methods section (deep)"),
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

#[test]
fn a_suffixed_query_picks_the_bullet_it_names_even_among_same_text_siblings() {
    // Two bullets differing only by energy strip to the same phrase, so a
    // substring match alone cannot tell them apart. The full text can, and
    // the full text is exactly what every caller has: the action list, the
    // daily log and the ambiguity picker all carry the bullet verbatim.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Alpha\n\n## Next Actions\n- [ ] Draft the methods section (deep)\n- [ ] Draft the methods section (light)\n";
    let (vault, store) = vault_with(&[("projects/alpha.md", project)]);

    vault
        .complete_action(
            dt(2026, 5, 2, 9, 30),
            "alpha",
            "Draft the methods section (deep)",
        )
        .expect("the exact bullet resolves");

    let content = store.read_file(&vp("projects/alpha.md")).unwrap();
    assert!(
        content.contains("(light)"),
        "the sibling survives: {content}"
    );
    assert!(
        !content.contains("(deep)"),
        "the named one is gone: {content}"
    );
}

#[test]
fn a_partial_query_matching_several_bullets_is_still_ambiguous() {
    // The picker exists for genuine ambiguity, and this is it: a phrase
    // that names neither bullet outright.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Alpha\n\n## Next Actions\n- [ ] Draft the methods section (deep)\n- [ ] Draft the results section (light)\n";
    let (vault, _store) = vault_with(&[("projects/alpha.md", project)]);

    let err = vault
        .complete_action(dt(2026, 5, 2, 9, 30), "alpha", "Draft the")
        .expect_err("a phrase matching both must ask");

    match err {
        DomainError::AmbiguousAction { candidates, .. } => {
            assert_eq!(candidates.len(), 2, "both are offered: {candidates:?}");
        }
        other => panic!("expected AmbiguousAction, got {other:?}"),
    }
}

#[test]
fn an_action_whose_text_ends_in_a_parenthetical_is_not_mangled() {
    // `strip_energy_suffix` only strips the three known energies, so a
    // bullet legitimately ending in brackets survives.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Alpha\n\n## Next Actions\n- [ ] Reread the appendix (draft)\n";
    let (vault, store) = vault_with(&[("projects/alpha.md", project)]);

    vault
        .complete_action(
            dt(2026, 5, 2, 9, 30),
            "alpha",
            "Reread the appendix (draft)",
        )
        .expect("a non-energy parenthetical matches verbatim");

    let content = store.read_file(&vp("projects/alpha.md")).unwrap();
    assert!(
        !content.contains("Reread the appendix"),
        "content: {content}"
    );
}

#[test]
fn choosing_a_candidate_from_the_picker_resolves_it() {
    // The ambiguity picker re-invokes with the chosen candidate's exact
    // text. If that still only substring-matched, the pick would be
    // ambiguous again and the dialog would reopen forever — the user could
    // never complete either bullet. An exact match must win outright.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Alpha\n\n## Next Actions\n- [ ] Draft the methods section (deep)\n- [ ] Draft the methods section (light)\n";
    let (vault, store) = vault_with(&[("projects/alpha.md", project)]);

    vault
        .complete_action(
            dt(2026, 5, 2, 9, 30),
            "alpha",
            "Draft the methods section (light)",
        )
        .expect("the exact candidate text resolves to that one bullet");

    let content = store.read_file(&vp("projects/alpha.md")).unwrap();
    assert!(
        content.contains("Draft the methods section (deep)"),
        "the other bullet survives: {content}"
    );
    assert!(
        !content.contains("(light)"),
        "the chosen one is gone: {content}"
    );
}

#[test]
fn promote_resolves_the_exact_bullet_like_completion_does() {
    // Completion and promotion are documented as disambiguating alike, and
    // for a while they did not: promotion kept only the substring half, so
    // two bullets differing by energy could never be told apart. Worse than
    // an error — the picker handed back its own candidate, that
    // re-ambiguated, and the dialog closed on itself. Neither bullet was
    // promotable from the UI at all.
    let project = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Alpha\n\n## Next Actions\n- [ ] Draft the methods section (deep)\n- [ ] Draft the methods section (light)\n";
    let (vault, store) = vault_with(&[("projects/alpha.md", project)]);

    vault
        .promote_action(
            dt(2026, 5, 2, 9, 30),
            "alpha",
            "Draft the methods section (light)",
        )
        .expect("the exact bullet resolves, as it does for completion");

    let content = store.read_file(&vp("projects/alpha.md")).unwrap();
    assert!(
        content.contains("Draft the methods section (deep)"),
        "the sibling survives: {content}"
    );
}

// ---------------------------------------------------------------------
// Drop: closing an action WITHOUT claiming it was done (#559)
// ---------------------------------------------------------------------

#[test]
fn drop_action_archives_its_note_as_dropped_not_completed() {
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Prepare the demo proposal",
            EnergyLevel::Deep,
        )
        .unwrap();

    let outcome = vault
        .drop_action(
            dt(2026, 5, 27, 17, 0),
            "foo",
            "demo-proposal",
            Some("superseded by the demo-planning action"),
        )
        .expect("drop succeeds");

    // Same archival mechanics as a completion: source and destination
    // both in the touched set, so the desktop watcher cannot echo them.
    assert!(outcome.touched());
    let touched: std::collections::HashSet<_> = outcome.paths.iter().cloned().collect();
    assert_eq!(
        touched,
        std::collections::HashSet::from([
            vp("projects/foo.md"),
            vp("actions/prepare-the-demo-proposal.md"),
            vp("actions/_done/2026/prepare-the-demo-proposal.md"),
            vp("journal/2026/daily/2026-05-27.md"),
        ]),
    );

    let done = vp("actions/_done/2026/prepare-the-demo-proposal.md");
    let fm = read_action_frontmatter(&store, &done);
    assert_eq!(
        fm.status,
        ActionStatus::Dropped,
        "the note records abandonment, not completion"
    );
    assert_eq!(
        fm.completed, None,
        "a dropped action has no completion date \u{2014} it was never completed"
    );
}

#[test]
fn drop_action_logs_the_drop_and_its_reason_rather_than_a_completion() {
    let body =
        project_with_bullets("- [ ] Run feature set B (deep)\n- [ ] Draft methods (medium)\n");
    let (vault, store) = vault_with(&[("projects/foo.md", &body)]);

    vault
        .drop_action(
            dt(2026, 5, 1, 16, 30),
            "foo",
            "feature set B",
            Some("superseded by the ablation run"),
        )
        .expect("drop succeeds");

    let raw = store.read_file(&vp("projects/foo.md")).unwrap();
    assert!(
        !raw.contains("Run feature set B"),
        "bullet not removed:\n{raw}"
    );
    assert!(
        raw.contains("- [ ] Draft methods (medium)"),
        "other action lost:\n{raw}"
    );

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-01.md"))
        .expect("daily note exists");
    assert!(
        daily.contains("- **16:30**: action dropped on [[foo]] \u{2014} Run feature set B (deep)"),
        "drop entry missing:\n{daily}"
    );
    assert!(
        daily.contains("reason: superseded by the ablation run"),
        "reason missing:\n{daily}"
    );
    assert!(
        !daily.contains("action done on"),
        "a drop must never be logged as a completion:\n{daily}"
    );
}

#[test]
fn drop_action_without_a_reason_logs_a_bare_entry() {
    let body = project_with_bullets("- [ ] Run feature set B (deep)\n");
    let (vault, store) = vault_with(&[("projects/foo.md", &body)]);

    vault
        .drop_action(dt(2026, 5, 1, 16, 30), "foo", "feature set B", None)
        .expect("drop succeeds");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-01.md"))
        .unwrap();
    assert!(
        daily.contains("- **16:30**: action dropped on [[foo]] \u{2014} Run feature set B (deep)"),
        "{daily}"
    );
    assert!(!daily.contains("reason:"), "no empty reason line:\n{daily}");
}

/// A newline in the reason would split one log entry into two lines,
/// and the second would not parse as an entry at all.
#[test]
fn drop_action_flattens_a_multiline_reason_into_one_entry() {
    let body = project_with_bullets("- [ ] Run feature set B (deep)\n");
    let (vault, store) = vault_with(&[("projects/foo.md", &body)]);

    vault
        .drop_action(
            dt(2026, 5, 1, 16, 30),
            "foo",
            "feature set B",
            Some("superseded\n\nby the ablation   run"),
        )
        .expect("drop succeeds");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-01.md"))
        .unwrap();
    assert!(
        daily.contains("reason: superseded by the ablation run"),
        "whitespace runs collapse to single spaces:\n{daily}"
    );
}

#[test]
fn drop_action_errors_when_action_not_found() {
    let body = project_with_bullets("- [ ] Run feature set B (deep)\n");
    let (vault, _store) = vault_with(&[("projects/foo.md", &body)]);

    let err = vault
        .drop_action(dt(2026, 5, 1, 16, 30), "foo", "nothing like this", None)
        .unwrap_err();
    assert!(
        matches!(err, DomainError::ActionNotFound { .. }),
        "got {err:?}"
    );
}

/// The PR's central negative promise, tested against the user-visible
/// query rather than a proxy: a dropped action must never turn up in
/// the weekly or monthly "what did you finish" views.
///
/// End-to-end cover only. `completed_actions_between` filters on
/// `status` *and* on the date, so this passes while either guard alone
/// holds — it cannot localise a break. The two guards are pinned
/// individually by `drop_action_clears_a_pre_existing_completed_date`
/// and `a_stale_completion_date_on_an_open_action_is_not_completed_work`.
#[test]
fn a_dropped_action_never_appears_in_completed_actions() {
    let (vault, _store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Prepare the demo proposal",
            EnergyLevel::Deep,
        )
        .unwrap();
    vault
        .drop_action(dt(2026, 5, 27, 17, 0), "foo", "demo-proposal", None)
        .expect("drop succeeds");

    let completed = vault
        .completed_actions_between(
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
        )
        .expect("completed_actions_between");
    assert!(
        completed.is_empty(),
        "a drop is not an achievement: {completed:?}"
    );
}

/// The one test that actually joins the drop writer to the focus
/// reader. Every other test on this path feeds one side a fixture of
/// what the other is assumed to emit, so the load-bearing invariant —
/// the reason rides on its own indented continuation line, leaving the
/// entry head carrying the action text alone — was left free: emitting
/// it inline as `; reason: ...` instead kept the whole suite green
/// while genuinely breaking the match, because `current_focus` reads
/// heads and an inline reason lands in the head.
#[test]
fn a_real_drop_with_a_reason_clears_the_focus_it_opened() {
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    // A plain bullet, not an attached note: `start_action` logs the raw
    // text it is handed while the close verbs log the *resolved* bullet
    // text, so the two only coincide for a bullet that is its own text.
    // With an attached note they never match and no close clears the
    // focus — a pre-existing defect (`complete_action` behaves
    // identically), filed separately rather than widened into this PR.
    vault
        .add_action(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Prepare the demo proposal",
            EnergyLevel::Deep,
        )
        .unwrap();
    vault
        .start_action(
            dt(2026, 5, 26, 9, 30),
            "foo",
            "Prepare the demo proposal (deep)",
        )
        .expect("start succeeds");

    assert!(
        vault
            .current_focus(NaiveDate::from_ymd_opt(2026, 5, 26).unwrap())
            .unwrap()
            .is_some(),
        "precondition: the start opened a focus"
    );

    vault
        .drop_action(
            dt(2026, 5, 26, 17, 0),
            "foo",
            "Prepare the demo proposal",
            Some("superseded by the ablation run"),
        )
        .expect("drop succeeds");

    assert_eq!(
        vault
            .current_focus(NaiveDate::from_ymd_opt(2026, 5, 26).unwrap())
            .unwrap(),
        None,
        "the drop the writer emitted must clear the start it names"
    );

    // Pin the shape the reader depends on, so a change to either side
    // fails here rather than silently decoupling the two again.
    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-26.md"))
        .unwrap();
    assert!(
        daily.contains("\n  reason: superseded by the ablation run"),
        "the reason belongs on its own continuation line: {daily}"
    );
}

/// The absent-key tolerance must mean "the note asserts no completion",
/// not "the rewriter could not find the line". `rewrite_field_in_frontmatter`
/// scans for a column-0 `completed:` prefix while `Frontmatter` parses
/// YAML, so a quoted key is invisible to the scan and visible to the
/// parser. Swallowing on the scan alone archived the exact
/// self-contradictory file — `status: dropped` carrying a completion
/// date — that clearing the field exists to prevent.
#[test]
fn a_drop_refuses_to_archive_a_completion_date_the_rewriter_cannot_see() {
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    let note = vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Prepare the demo proposal",
            EnergyLevel::Deep,
        )
        .unwrap();

    let raw = store.read_file(&note).unwrap();
    store
        .write_file(
            &note,
            &raw.replace("completed: null", "\"completed\": 2026-05-01"),
        )
        .unwrap();

    let err = vault
        .drop_action(dt(2026, 5, 27, 17, 0), "foo", "demo-proposal", None)
        .expect_err("a completion date the arm cannot clear must not be archived");
    assert!(
        matches!(err, DomainError::MissingFrontmatterField(_)),
        "got {err:?}"
    );
    assert!(
        !store
            .exists(&vp("actions/_done/2026/prepare-the-demo-proposal.md"))
            .unwrap(),
        "nothing is archived on the error path"
    );
}

/// The drop path must not require a `completed:` key to exist.
/// `completed` is optional in an action's frontmatter, so a note that
/// omits it parses cleanly and lints clean — an ejected
/// `.cuaderno/templates/action.md` may simply leave the line out. The
/// first `completed: null` implementation rewrote the field
/// unconditionally and so failed the whole verb on such a note: bullet
/// not removed, nothing logged, note not archived. `complete_action`
/// has always failed this way, but a drop is the escape hatch for when
/// a completion is the wrong claim, so it must not inherit that.
#[test]
fn drop_action_survives_an_action_note_with_no_completed_field() {
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    let note = vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Prepare the demo proposal",
            EnergyLevel::Deep,
        )
        .unwrap();

    let raw = store.read_file(&note).unwrap();
    let without = raw.replace("completed: null\n", "");
    assert!(
        !without.contains("completed:"),
        "fixture must actually drop the key"
    );
    store.write_file(&note, &without).unwrap();

    vault
        .drop_action(dt(2026, 5, 27, 17, 0), "foo", "demo-proposal", None)
        .expect("a drop must not require the key to be present");

    let done = vp("actions/_done/2026/prepare-the-demo-proposal.md");
    let fm = read_action_frontmatter(&store, &done);
    assert_eq!(fm.status, ActionStatus::Dropped);
    assert_eq!(fm.completed, None);
}

/// The second of `completed_actions_between`'s two guards, pinned on its
/// own. An action left `active` or `blocked` while carrying a stale
/// completion date is not finished work, and only the `status` check
/// keeps it out of the weekly and monthly views — the date check passes
/// it straight through.
#[test]
fn a_stale_completion_date_on_an_open_action_is_not_completed_work() {
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    let note = vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Prepare the demo proposal",
            EnergyLevel::Deep,
        )
        .unwrap();

    let raw = store.read_file(&note).unwrap();
    store
        .write_file(
            &note,
            &raw.replace("completed: null", "completed: 2026-05-27"),
        )
        .unwrap();

    let completed = vault
        .completed_actions_between(
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
        )
        .expect("completed_actions_between");
    assert!(
        completed.is_empty(),
        "an action that is still open has not been finished, whatever \
         date its frontmatter carries: {completed:?}"
    );
}

/// A note hand-edited to carry a completion date while still active
/// must not be archived as `dropped` *and* dated. The archival clears
/// the field rather than leaving whatever was there, so the file cannot
/// contradict itself and no reader has to check `status` first to be
/// safe.
#[test]
fn drop_action_clears_a_pre_existing_completed_date() {
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    let note = vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Prepare the demo proposal",
            EnergyLevel::Deep,
        )
        .unwrap();

    let raw = store.read_file(&note).unwrap();
    store
        .write_file(
            &note,
            &raw.replace("completed: null", "completed: 2026-01-05"),
        )
        .unwrap();

    vault
        .drop_action(dt(2026, 5, 27, 17, 0), "foo", "demo-proposal", None)
        .expect("drop succeeds");

    let done = vp("actions/_done/2026/prepare-the-demo-proposal.md");
    let fm = read_action_frontmatter(&store, &done);
    assert_eq!(fm.status, ActionStatus::Dropped);
    assert_eq!(
        fm.completed, None,
        "the stale completion date is cleared, not carried into the archive"
    );
}

/// The acceptance criterion of #568, joining the two sides rather than
/// feeding each a fixture: the text the desktop app actually passes,
/// through a real start and a real close.
#[test]
fn a_start_from_the_desktop_path_is_cleared_by_completing_it() {
    let (vault, _store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Prepare the demo proposal",
            EnergyLevel::Deep,
        )
        .unwrap();

    // Exactly what `ActionShortlist` hands `start_action`.
    let listed = vault.list_actions("foo").unwrap();
    vault
        .start_action(dt(2026, 5, 26, 9, 30), "foo", &listed[0].text)
        .expect("start succeeds");
    assert!(
        vault
            .current_focus(NaiveDate::from_ymd_opt(2026, 5, 26).unwrap())
            .unwrap()
            .is_some(),
        "precondition: the start opened a focus"
    );

    vault
        .complete_action(dt(2026, 5, 26, 17, 0), "foo", "demo-proposal")
        .expect("complete succeeds");

    assert_eq!(
        vault
            .current_focus(NaiveDate::from_ymd_opt(2026, 5, 26).unwrap())
            .unwrap(),
        None,
        "the close must clear the start it names"
    );
}

/// And by dropping it — the other terminal verb writes a different
/// prefix, so it needs its own end-to-end pass.
#[test]
fn a_start_from_the_desktop_path_is_cleared_by_dropping_it() {
    let (vault, _store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action_with_note(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Prepare the demo proposal",
            EnergyLevel::Deep,
        )
        .unwrap();
    let listed = vault.list_actions("foo").unwrap();
    vault
        .start_action(dt(2026, 5, 26, 9, 30), "foo", &listed[0].text)
        .expect("start succeeds");

    vault
        .drop_action(
            dt(2026, 5, 26, 17, 0),
            "foo",
            "demo-proposal",
            Some("superseded"),
        )
        .expect("drop succeeds");

    assert_eq!(
        vault
            .current_focus(NaiveDate::from_ymd_opt(2026, 5, 26).unwrap())
            .unwrap(),
        None,
        "a drop clears the start too"
    );
}

/// The resolution, stated as a property: what is logged is the bullet,
/// not the query. A caller passing the energy-stripped form — which
/// `TopAction::text` is — still produces an entry a close can match.
#[test]
fn start_action_logs_the_bullet_text_not_the_query_it_was_given() {
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Draft the methods section",
            EnergyLevel::Deep,
        )
        .unwrap();

    // No energy suffix, and only part of the title.
    vault
        .start_action(dt(2026, 5, 26, 9, 30), "foo", "methods")
        .expect("a substring resolves, as it does for the close verbs");

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-26.md"))
        .unwrap();
    assert!(
        daily.contains("- **09:30**: started [[foo]] \u{2014} Draft the methods section (deep)"),
        "the resolved bullet is logged, so a close can match it:\n{daily}"
    );

    vault
        .complete_action(dt(2026, 5, 26, 17, 0), "foo", "methods")
        .expect("complete succeeds");
    assert_eq!(
        vault
            .current_focus(NaiveDate::from_ymd_opt(2026, 5, 26).unwrap())
            .unwrap(),
        None,
        "and it does"
    );
}

/// Unplanned work is refused rather than logged. It was never really
/// supported: a start naming no bullet can be closed by nothing, so the
/// focus stayed open for ever and `complete_action` reported the action
/// as not found. Better to say so at the start than to leave an
/// unfixable state behind.
#[test]
fn start_action_refuses_work_that_is_not_on_the_map() {
    let (vault, store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    vault
        .add_action(
            dt(2026, 5, 26, 9, 0),
            "foo",
            "Draft the methods section",
            EnergyLevel::Deep,
        )
        .unwrap();

    let err = vault
        .start_action(dt(2026, 5, 26, 9, 30), "foo", "Buy milk")
        .expect_err("a start names a bullet");
    assert!(
        matches!(err, DomainError::ActionNotFound { .. }),
        "got {err:?}"
    );

    let daily = store
        .read_file(&vp("journal/2026/daily/2026-05-26.md"))
        .unwrap();
    assert!(
        !daily.contains("started [[foo]]"),
        "a refused start logs nothing:\n{daily}"
    );
}

/// Ambiguity is an error carrying the candidates, as it is for the
/// close verbs — starting the wrong one of two look-alike bullets puts
/// the focus on work you are not doing.
#[test]
fn start_action_refuses_an_ambiguous_query_and_offers_the_candidates() {
    let (vault, _store) = vault_with(&[("projects/foo.md", ACTIVE_PROJECT)]);
    for title in ["Draft the methods section", "Draft the results section"] {
        vault
            .add_action(dt(2026, 5, 26, 9, 0), "foo", title, EnergyLevel::Deep)
            .unwrap();
    }

    let err = vault
        .start_action(dt(2026, 5, 26, 9, 30), "foo", "Draft the")
        .expect_err("two matches must not be resolved by guessing");
    match err {
        DomainError::AmbiguousAction { candidates, .. } => {
            assert_eq!(candidates.len(), 2, "both offered: {candidates:?}");
        }
        other => panic!("got {other:?}"),
    }
}

// --- start_unplanned_action -------------------------------------------
//
// The path that makes "just start something" real (#568). The old
// free-text `start_action` only appeared to support it: the start was
// logged, and then nothing could ever close it.

#[test]
fn unplanned_start_adds_the_bullet_and_starts_it_in_one_go() {
    let (vault, store) = vault_with(&[("projects/alpha.md", ACTIVE_PROJECT)]);

    let outcome = vault
        .start_unplanned_action(
            dt(2026, 5, 26, 9, 30),
            "alpha",
            "Fix the CI badge",
            EnergyLevel::Light,
        )
        .unwrap();
    let daily = daily_path_of(&outcome);

    let map = store
        .read_file(&VaultPath::new("projects/alpha.md").unwrap())
        .unwrap();
    assert!(
        map.contains("- [ ] Fix the CI badge (light)"),
        "the work is on the map now, not just in the log: {map}"
    );

    let content = store.read_file(&daily).unwrap();
    assert!(
        content.contains("- **09:30**: started [[alpha]] \u{2014} Fix the CI badge (light)"),
        "started line carries the resolved bullet text: {content}"
    );
}

#[test]
fn unplanned_start_logs_the_bullets_origin_as_well_as_the_start() {
    // Adding the bullet mutates `## Next Actions`, so it emits its own
    // log entry — the map never gains a line from nowhere. Both entries
    // must survive: they are staged into one write, and staging them
    // separately would silently drop the first.
    let (vault, store) = vault_with(&[("projects/alpha.md", ACTIVE_PROJECT)]);

    let outcome = vault
        .start_unplanned_action(
            dt(2026, 5, 26, 9, 30),
            "alpha",
            "Fix the CI badge",
            EnergyLevel::Light,
        )
        .unwrap();
    let daily = daily_path_of(&outcome);

    let content = store.read_file(&daily).unwrap();
    assert!(
        content.contains("action added to [[alpha]] \u{2014} Fix the CI badge (light)"),
        "the addition is logged: {content}"
    );
    assert!(
        content.contains("started [[alpha]] \u{2014} Fix the CI badge (light)"),
        "the start is logged: {content}"
    );
}

#[test]
fn unplanned_work_can_actually_be_completed() {
    // The whole point. On the old free-text path this was impossible:
    // the start logged raw text, `complete_action` logged resolved
    // bullet text, the two never matched, and `current_focus` stayed
    // pinned to the unplanned work for ever.
    let (vault, _store) = vault_with(&[("projects/alpha.md", ACTIVE_PROJECT)]);

    vault
        .start_unplanned_action(
            dt(2026, 5, 26, 9, 30),
            "alpha",
            "Fix the CI badge",
            EnergyLevel::Light,
        )
        .unwrap();

    let focus = vault
        .current_focus(NaiveDate::from_ymd_opt(2026, 5, 26).unwrap())
        .unwrap();
    assert!(
        focus.is_some_and(|f| f.action.contains("Fix the CI badge")),
        "focus is on the unplanned work while it runs"
    );

    vault
        .complete_action(dt(2026, 5, 26, 11, 0), "alpha", "Fix the CI badge")
        .unwrap();

    assert!(
        vault
            .current_focus(NaiveDate::from_ymd_opt(2026, 5, 26).unwrap())
            .unwrap()
            .is_none(),
        "and it clears on completion — the invariant free text could never satisfy"
    );
}

#[test]
fn unplanned_work_can_also_be_dropped() {
    let (vault, _store) = vault_with(&[("projects/alpha.md", ACTIVE_PROJECT)]);

    vault
        .start_unplanned_action(
            dt(2026, 5, 26, 9, 30),
            "alpha",
            "Fix the CI badge",
            EnergyLevel::Light,
        )
        .unwrap();
    vault
        .drop_action(dt(2026, 5, 26, 11, 0), "alpha", "Fix the CI badge", None)
        .unwrap();

    assert!(
        vault
            .current_focus(NaiveDate::from_ymd_opt(2026, 5, 26).unwrap())
            .unwrap()
            .is_none(),
        "dropping clears the focus too"
    );
}

#[test]
fn unplanned_start_rejects_parked_project_and_blank_action() {
    const PARKED: &str = "---\ntype: project\ncontext: work\nstatus: parked\ncreated: 2026-04-01\n---\n\n# Beta\n\n## Current State\nOn ice.\n";
    let (vault, _store) = vault_with(&[
        ("projects/alpha.md", ACTIVE_PROJECT),
        ("projects/_parked/beta.md", PARKED),
    ]);

    assert!(matches!(
        vault.start_unplanned_action(
            dt(2026, 5, 26, 9, 30),
            "beta",
            "Anything",
            EnergyLevel::Light
        ),
        Err(DomainError::ProjectNotActive { .. })
    ));
    assert!(matches!(
        vault.start_unplanned_action(dt(2026, 5, 26, 9, 30), "alpha", "   ", EnergyLevel::Light),
        Err(DomainError::EmptyField { field: "action" })
    ));
}

#[test]
fn unplanned_start_appends_rather_than_replacing_existing_actions() {
    let (vault, store) = vault_with(&[("projects/alpha.md", ACTIVE_PROJECT)]);
    vault
        .add_action(
            dt(2026, 5, 26, 9, 0),
            "alpha",
            "Draft methods",
            EnergyLevel::Deep,
        )
        .unwrap();

    vault
        .start_unplanned_action(
            dt(2026, 5, 26, 9, 30),
            "alpha",
            "Fix the CI badge",
            EnergyLevel::Light,
        )
        .unwrap();

    let map = store
        .read_file(&VaultPath::new("projects/alpha.md").unwrap())
        .unwrap();
    assert!(
        map.contains("- [ ] Draft methods (deep)"),
        "planned work survives: {map}"
    );
    assert!(
        map.contains("- [ ] Fix the CI badge (light)"),
        "unplanned work added: {map}"
    );
}

/// The daily note out of a [`WriteOutcome`]'s touched set. Pulling it
/// from the outcome rather than rebuilding the path keeps these tests
/// honest about what the commit actually wrote — the same set the
/// desktop layer journals for watcher echo-suppression.
fn daily_path_of(outcome: &WriteOutcome) -> VaultPath {
    outcome
        .paths
        .iter()
        .find(|p| p.as_path().starts_with("journal"))
        .expect("the commit wrote a daily note")
        .clone()
}

#[test]
fn unplanned_start_reports_both_files_it_wrote() {
    // The desktop layer journals this set so the watcher doesn't echo
    // the writes back as external edits (#315). This op touches two
    // files, and a caller-side reconstruction would miss one.
    let (vault, _store) = vault_with(&[("projects/alpha.md", ACTIVE_PROJECT)]);

    let outcome = vault
        .start_unplanned_action(
            dt(2026, 5, 26, 9, 30),
            "alpha",
            "Fix the CI badge",
            EnergyLevel::Light,
        )
        .unwrap();

    assert_eq!(
        outcome.primary,
        VaultPath::new("projects/alpha.md").unwrap(),
        "the op is about the project map"
    );
    assert!(
        outcome
            .paths
            .contains(&VaultPath::new("projects/alpha.md").unwrap()),
        "map is in the touched set: {:?}",
        outcome.paths
    );
    assert!(
        outcome
            .paths
            .iter()
            .any(|p| p.as_path().starts_with("journal")),
        "daily note is in the touched set: {:?}",
        outcome.paths
    );
}
