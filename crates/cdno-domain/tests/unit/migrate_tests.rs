//! The method-migration runner (RFC 0006 §8.4, plan T8): stamping last, in
//! one transaction under the lock, outside the gate, idempotent, and never
//! touching the journal or an append-only note.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

use cdno_core::config::{MethodVersion, VaultConfig};
use cdno_core::error::StoreError;
use cdno_core::file_meta::FileMeta;
use cdno_core::hash::content_hash;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore, VaultWriteLock};
use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::EnergyLevel;
use cdno_domain::{
    FILE_MIGRATIONS, FileMigration, METHOD_VERSION, MigrationPlan, MigrationStep, Vault,
};

use super::support::{FailingStore, RacingStore};

const CONFIG: &str = ".cuaderno/config.toml";
const PROJECT: &str = "projects/probe.md";
const DAILY: &str = "journal/2026/daily/2026-04-20.md";

fn at() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 4, 20)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(14, 30, 0).unwrap())
}

fn path(raw: &str) -> VaultPath {
    VaultPath::new(raw).unwrap()
}

fn version(major: u16, minor: u16) -> MethodVersion {
    MethodVersion { major, minor }
}

/// A config with comments, blank lines and keys around where the stamp goes,
/// and no `method_version`: an unstamped vault.
const UNSTAMPED_CONFIG: &str = "# Cuaderno vault configuration.\n\
                                [vault]\n\
                                # Shown in the UI.\n\
                                name = \"Probe\"\n\
                                max_active_projects = 4 # four, not five\n\
                                \n\
                                # Paths the index skips.\n\
                                ignore = [\"drafts/**\"]\n\
                                \n\
                                [focus]\n\
                                carry_over_days = 2\n";

/// A config stamped `version`, with a comment the stamp must keep.
fn stamped_config(version: MethodVersion) -> String {
    format!(
        "[vault]\nname = \"Probe\"\n# The method's version, not cdno's.\nmethod_version = \"{version}\"\n"
    )
}

/// A project map the synthetic step migrates: `old wording` becomes
/// `new wording`.
const PROJECT_OLD: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-01-05\ncore_question: null\nclosed: null\n---\n\n# Probe\n\n## Current State\nold wording\n\n## Next Actions\n- [ ] Define first concrete step (light)\n\n## Waiting On\n(nothing yet)\n\n## Milestones\n\n## Links\n- Portfolio: (none yet)\n";

fn project_new() -> String {
    PROJECT_OLD.replace("old wording", "new wording")
}

/// The synthetic major step: rewrite the probe map's wording, idempotently
/// (a map already in the new wording plans nothing).
fn reword_the_map(vault: &Vault, plan: &mut MigrationPlan) -> Result<(), DomainError> {
    let map = path(PROJECT);
    if let Some(content) = plan.read(vault, &map)?
        && content.contains("old wording")
    {
        plan.write(map, content.replace("old wording", "new wording"));
    }
    Ok(())
}

const REWORD: FileMigration = FileMigration {
    to_major: 2,
    description: "reword the probe map",
    plan: reword_the_map,
};

/// A store seeded with `files`, before any wrapper sees a write.
fn seeded(files: &[(&str, &str)]) -> Arc<MemoryVaultStore> {
    let store = Arc::new(MemoryVaultStore::new());
    for (raw, content) in files {
        store.write_file(&path(raw), content).unwrap();
    }
    store
}

fn open(store: Arc<dyn VaultStore>) -> (Vault, Arc<dyn VaultIndex>) {
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) =
        Vault::new(store, Arc::clone(&index), VaultConfig::default()).expect("Vault::new");
    (vault, index)
}

/// Every file in the store and its content.
fn snapshot(store: &dyn VaultStore) -> BTreeMap<String, String> {
    store
        .walk_dir(&VaultPath::root())
        .unwrap()
        .into_iter()
        .map(|p| {
            let content = store.read_file(&p).unwrap();
            (p.to_string(), content)
        })
        .collect()
}

fn read(store: &dyn VaultStore, raw: &str) -> String {
    store.read_file(&path(raw)).unwrap()
}

#[test]
fn an_unstamped_vault_is_stamped_with_an_empty_list_and_logs_one_line() {
    assert_eq!(
        METHOD_VERSION,
        version(1, 0),
        "the probe assumes a 1.0 binary"
    );
    assert!(FILE_MIGRATIONS.is_empty(), "the list is empty for RLM 1.1");
    let store = seeded(&[(CONFIG, UNSTAMPED_CONFIG)]);
    let (vault, _index) = open(store.clone());

    let report = vault
        .migrate(at(), METHOD_VERSION, FILE_MIGRATIONS)
        .expect("an unstamped vault migrates");
    assert_eq!(report.from, None);
    assert_eq!(report.to, METHOD_VERSION);
    assert!(report.steps.is_empty());
    assert!(report.read_through.is_empty());

    // The key lands at the end of `[vault]`; every comment, blank line and
    // other key is byte-identical.
    assert_eq!(
        read(&*store, CONFIG),
        UNSTAMPED_CONFIG.replace(
            "ignore = [\"drafts/**\"]\n",
            "ignore = [\"drafts/**\"]\nmethod_version = \"1.0\"\n"
        )
    );
    assert_eq!(vault.method_status().unwrap().vault, Some(version(1, 0)));

    let daily = read(&*store, DAILY);
    let line = "- **14:30**: vault migrated\n  was: unstamped (read as 1.0)\n  now: 1.0\n";
    assert_eq!(daily.matches(line).count(), 1, "daily note:\n{daily}");
    assert_eq!(daily.matches("vault migrated").count(), 1);
}

#[test]
fn a_vault_stamped_at_the_binarys_version_is_a_no_op() {
    let store = seeded(&[
        (CONFIG, &stamped_config(METHOD_VERSION)),
        (PROJECT, PROJECT_OLD),
    ]);
    let (vault, _index) = open(store.clone());
    let before = snapshot(&*store);

    let report = vault
        .migrate(at(), METHOD_VERSION, &[REWORD])
        .expect("a stamped vault is a no-op");
    assert_eq!(report.from, Some(METHOD_VERSION));
    assert!(!report.stamps());
    assert!(report.steps.is_empty());
    assert_eq!(report.stamp_line(), None);
    assert_eq!(report.log_entry(), None);
    assert_eq!(snapshot(&*store), before, "a no-op wrote something");
}

#[test]
fn a_second_run_writes_nothing() {
    // An unstamped vault and the real list, then a stamped 1.0 vault and a
    // synthetic major step: each second run leaves the store byte-identical.
    let store = seeded(&[(CONFIG, UNSTAMPED_CONFIG)]);
    let (vault, _index) = open(store.clone());
    vault
        .migrate(at(), METHOD_VERSION, FILE_MIGRATIONS)
        .unwrap();
    let after_first = snapshot(&*store);
    let second = vault
        .migrate(at(), METHOD_VERSION, FILE_MIGRATIONS)
        .unwrap();
    assert!(!second.stamps());
    assert_eq!(snapshot(&*store), after_first);

    let store = seeded(&[
        (CONFIG, &stamped_config(version(1, 0))),
        (PROJECT, PROJECT_OLD),
    ]);
    let (vault, _index) = open(store.clone());
    vault.migrate(at(), version(2, 0), &[REWORD]).unwrap();
    let after_first = snapshot(&*store);
    let second = vault.migrate(at(), version(2, 0), &[REWORD]).unwrap();
    assert!(!second.stamps());
    assert_eq!(snapshot(&*store), after_first);
}

#[test]
fn a_synthetic_major_step_and_the_stamp_commit_together() {
    let store = seeded(&[
        (CONFIG, &stamped_config(version(1, 0))),
        (PROJECT, PROJECT_OLD),
    ]);
    let (vault, index) = open(store.clone());

    let report = vault
        .migrate(at(), version(2, 0), &[REWORD])
        .expect("the synthetic step runs");
    assert_eq!(report.from, Some(version(1, 0)));
    assert_eq!(report.to, version(2, 0));
    assert_eq!(
        report.steps,
        vec![MigrationStep {
            to_major: 2,
            description: "reword the probe map",
            writes: vec![path(PROJECT)],
        }]
    );

    assert_eq!(read(&*store, PROJECT), project_new());
    assert_eq!(read(&*store, CONFIG), stamped_config(version(2, 0)));
    let daily = read(&*store, DAILY);
    assert!(
        daily.contains("- **14:30**: vault migrated\n  was: 1.0\n  now: 2.0\n"),
        "daily note:\n{daily}"
    );
    // The step's index row was rewritten with the file.
    let row = index
        .find_by_path(&path(PROJECT))
        .unwrap()
        .expect("indexed");
    assert_eq!(row.content_hash, content_hash(&project_new()));
}

#[test]
fn the_dry_run_reports_the_steps_and_the_stamp_and_writes_nothing() {
    let store = seeded(&[
        (CONFIG, &stamped_config(version(1, 0))),
        (PROJECT, PROJECT_OLD),
    ]);
    let (vault, _index) = open(store.clone());
    let before = snapshot(&*store);

    let planned = vault.plan_migration(version(2, 0), &[REWORD]).unwrap();
    assert_eq!(snapshot(&*store), before, "the dry run wrote something");
    assert_eq!(planned.steps[0].writes, vec![path(PROJECT)]);
    assert_eq!(
        planned.stamp_line().as_deref(),
        Some("method_version = \"2.0\"")
    );
    assert_eq!(
        planned.log_entry().as_deref(),
        Some("vault migrated\n  was: 1.0\n  now: 2.0")
    );

    // The real run does what the dry run said.
    assert_eq!(
        vault.migrate(at(), version(2, 0), &[REWORD]).unwrap(),
        planned
    );
}

/// A `MemoryVaultStore` that records the path of every file write, in order.
struct RecordingStore {
    inner: MemoryVaultStore,
    writes: Mutex<Vec<String>>,
}

impl VaultStore for RecordingStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        self.inner.read_file(path)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        self.inner.read_bytes(path)
    }
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.writes.lock().unwrap().push(path.to_string());
        self.inner.write_file(path, content)
    }
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.writes.lock().unwrap().push(path.to_string());
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
        self.inner.acquire_write_lock()
    }
}

#[test]
fn the_stamp_is_the_last_write() {
    let recording = Arc::new(RecordingStore {
        inner: MemoryVaultStore::new(),
        writes: Mutex::new(Vec::new()),
    });
    recording
        .inner
        .write_file(&path(CONFIG), &stamped_config(version(1, 0)))
        .unwrap();
    recording
        .inner
        .write_file(&path(PROJECT), PROJECT_OLD)
        .unwrap();
    let (vault, _index) = open(recording.clone());

    vault.migrate(at(), version(2, 0), &[REWORD]).unwrap();
    let writes = recording.writes.lock().unwrap().clone();
    assert_eq!(
        writes,
        vec![PROJECT.to_owned(), DAILY.to_owned(), CONFIG.to_owned()],
        "the step's file, then the log line, then the stamp"
    );

    // And when the stamp's own write fails, the commit rolls back the step's
    // file and the log line: nothing changes.
    let inner = seeded(&[
        (CONFIG, &stamped_config(version(1, 0))),
        (PROJECT, PROJECT_OLD),
    ]);
    let before = snapshot(&*inner);
    let failing: Arc<dyn VaultStore> = Arc::new(FailingStore::new(inner.clone(), 3));
    let (vault, _index) = open(failing);
    let err = vault
        .migrate(at(), version(2, 0), &[REWORD])
        .expect_err("the third write, the stamp, fails");
    assert!(
        matches!(err, DomainError::Transaction(_)),
        "expected a transaction error, got: {err:?}"
    );
    assert_eq!(
        snapshot(&*inner),
        before,
        "a failed stamp left files changed"
    );
}

/// A store whose writes fail from the first write to `poison` onwards,
/// rollback writes included: what a crash mid-commit leaves behind.
struct PoisonedStore {
    inner: Arc<MemoryVaultStore>,
    poison: VaultPath,
    poisoned: Mutex<bool>,
}

impl PoisonedStore {
    fn check(&self, path: &VaultPath) -> Result<(), StoreError> {
        let mut poisoned = self.poisoned.lock().unwrap();
        if *path == self.poison {
            *poisoned = true;
        }
        if *poisoned {
            return Err(StoreError::PermissionDenied(path.to_string()));
        }
        Ok(())
    }
}

impl VaultStore for PoisonedStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        self.inner.read_file(path)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        self.inner.read_bytes(path)
    }
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.check(path)?;
        self.inner.write_file(path, content)
    }
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.check(path)?;
        self.inner.append_to_file(path, content)
    }
    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError> {
        self.check(dest)?;
        self.inner.move_file(src, dest)
    }
    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError> {
        self.check(path)?;
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
        self.check(dest)?;
        self.inner.import_external(src, dest)
    }
}

#[test]
fn a_failing_step_write_leaves_the_stamp_unchanged() {
    // The existing `FailingStore`: the step's write is the first, and fails.
    let inner = seeded(&[
        (CONFIG, &stamped_config(version(1, 0))),
        (PROJECT, PROJECT_OLD),
    ]);
    let before = snapshot(&*inner);
    let failing: Arc<dyn VaultStore> = Arc::new(FailingStore::new(inner.clone(), 1));
    let (vault, _index) = open(failing);
    vault
        .migrate(at(), version(2, 0), &[REWORD])
        .expect_err("the step's write fails");
    assert_eq!(snapshot(&*inner), before);
    assert_eq!(read(&*inner, CONFIG), stamped_config(version(1, 0)));

    // And when the rollback fails too, as a crash would leave it: the stamp
    // is still the old one, so a rerun re-plans and completes.
    let inner = seeded(&[
        (CONFIG, &stamped_config(version(1, 0))),
        (PROJECT, PROJECT_OLD),
    ]);
    let poisoned: Arc<dyn VaultStore> = Arc::new(PoisonedStore {
        inner: inner.clone(),
        poison: path(PROJECT),
        poisoned: Mutex::new(false),
    });
    let (vault, _index) = open(poisoned);
    vault
        .migrate(at(), version(2, 0), &[REWORD])
        .expect_err("the step's write fails");
    assert_eq!(
        read(&*inner, CONFIG),
        stamped_config(version(1, 0)),
        "the stamp claims a version the files never reached"
    );
}

#[test]
fn a_rerun_after_a_partial_migration_completes_it() {
    // The step's change is already on disk (applied by hand, or by a run
    // that crashed before its stamp), and the vault is still on 1.0.
    let store = seeded(&[
        (CONFIG, &stamped_config(version(1, 0))),
        (PROJECT, &project_new()),
    ]);
    let (vault, _index) = open(store.clone());

    let report = vault.migrate(at(), version(2, 0), &[REWORD]).unwrap();
    assert_eq!(report.steps.len(), 1);
    assert!(
        report.steps[0].writes.is_empty(),
        "a migrated file planned a write: {:?}",
        report.steps[0].writes
    );
    assert_eq!(read(&*store, PROJECT), project_new());
    assert_eq!(read(&*store, CONFIG), stamped_config(version(2, 0)));
}

const MONTHLY: &str = "journal/2026/monthly/2026-04.md";
const EVIDENCE: &str = "portfolios/probe/2026-04-01-finding.md";
const EVIDENCE_NOTE: &str = "---\ntype: evidence\ncreated: 2026-04-01\nsource: lab\nportfolio: \"[[probe]]\"\n---\n\n# Finding\n\nold wording\n";

fn rewrite_the_monthly(vault: &Vault, plan: &mut MigrationPlan) -> Result<(), DomainError> {
    let monthly = path(MONTHLY);
    if let Some(content) = plan.read(vault, &monthly)? {
        plan.write(monthly, content.replace("old wording", "new wording"));
    }
    Ok(())
}

fn rewrite_the_evidence(vault: &Vault, plan: &mut MigrationPlan) -> Result<(), DomainError> {
    let evidence = path(EVIDENCE);
    if let Some(content) = plan.read(vault, &evidence)? {
        plan.write(evidence, content.replace("old wording", "new wording"));
    }
    Ok(())
}

fn create_an_evidence_note(_vault: &Vault, plan: &mut MigrationPlan) -> Result<(), DomainError> {
    plan.write(
        path("portfolios/probe/2026-04-02-made-up.md"),
        EVIDENCE_NOTE.replace("Finding", "Made up"),
    );
    Ok(())
}

/// A step's planning function, as `FileMigration::plan` holds it.
type PlanFn = fn(&Vault, &mut MigrationPlan) -> Result<(), DomainError>;

#[test]
fn a_step_that_plans_a_journal_note_is_refused_before_anything_is_written() {
    let monthly_note = "---\ntype: monthly\nmonth: 2026-04\n---\n\n# April\n\nold wording\n";
    let cases: [(PlanFn, &str, &str); 3] = [
        (rewrite_the_monthly, MONTHLY, "journal"),
        (
            rewrite_the_evidence,
            EVIDENCE,
            "evidence note is append-only",
        ),
        (
            create_an_evidence_note,
            "portfolios/probe/2026-04-02-made-up.md",
            "evidence note is append-only",
        ),
    ];
    for (plan, target, reason) in cases {
        let store = seeded(&[
            (CONFIG, &stamped_config(version(1, 0))),
            (PROJECT, PROJECT_OLD),
            (MONTHLY, monthly_note),
            (EVIDENCE, EVIDENCE_NOTE),
        ]);
        let (vault, _index) = open(store.clone());
        let before = snapshot(&*store);
        // The fenced step runs after a legitimate one, so a refusal that came
        // after staging would show in the map.
        let steps = [
            REWORD,
            FileMigration {
                to_major: 2,
                description: "touch the record",
                plan,
            },
        ];

        for result in [
            vault.plan_migration(version(2, 0), &steps).map(|_| ()),
            vault.migrate(at(), version(2, 0), &steps).map(|_| ()),
        ] {
            match result {
                Err(DomainError::MigrationTouchesProtectedPath {
                    step,
                    path: refused,
                    reason: why,
                }) => {
                    assert_eq!(step, "touch the record");
                    assert_eq!(refused, target);
                    assert!(why.contains(reason), "{why:?} lacks {reason:?}");
                }
                other => panic!("expected a refusal for {target}, got: {other:?}"),
            }
        }
        assert_eq!(snapshot(&*store), before, "{target}: something was written");
    }
}

#[test]
fn a_config_edited_between_plan_and_stamp_is_refused() {
    // A `cdno config` verb writes without the lock: the edit lands right
    // after the runner's first read of the config.
    let original = stamped_config(version(1, 0));
    let edited = format!("{original}max_active_projects = 3\n");
    let inner = seeded(&[(CONFIG, &original), (PROJECT, PROJECT_OLD)]);
    let racing = Arc::new(RacingStore::new(
        inner.clone(),
        path(CONFIG),
        path(CONFIG),
        &edited,
    ));
    let (vault, _index) = open(racing.clone());
    let map_before = read(&*inner, PROJECT);

    racing.arm();
    let err = vault
        .migrate(at(), version(2, 0), &[REWORD])
        .expect_err("a concurrent config edit refuses the stamp");
    assert!(
        matches!(err, DomainError::ConfigChangedDuringMigration),
        "got: {err:?}"
    );
    assert!(err.to_string().contains("cdno migrate"), "{err}");

    // The edit survives, unstamped, and nothing else was written.
    assert_eq!(read(&*inner, CONFIG), edited);
    assert_eq!(read(&*inner, PROJECT), map_before);
    assert!(!inner.exists(&path(DAILY)).unwrap());
}

#[test]
fn the_runner_is_not_blocked_by_the_gate_on_an_older_major() {
    // A vault on 0.9 against the 1.0 binary: the gate refuses every write,
    // and `cdno migrate` is the one way across.
    let store = seeded(&[
        (CONFIG, &stamped_config(version(0, 9))),
        (PROJECT, PROJECT_OLD),
    ]);
    let (vault, _index) = open(store.clone());
    assert!(matches!(
        vault.add_action(at(), "probe", "Gated", EnergyLevel::Light),
        Err(DomainError::VaultNeedsMigration { .. })
    ));

    let report = vault
        .migrate(at(), METHOD_VERSION, FILE_MIGRATIONS)
        .expect("the runner is not gated");
    assert_eq!(report.from, Some(version(0, 9)));
    assert_eq!(read(&*store, CONFIG), stamped_config(METHOD_VERSION));
    assert!(read(&*store, DAILY).contains("vault migrated\n  was: 0.9\n  now: 1.0\n"));

    // And now the gate lets writes through.
    vault
        .add_action(at(), "probe", "Ungated", EnergyLevel::Light)
        .expect("a migrated vault writes");
}

#[test]
fn a_newer_vault_is_refused() {
    for ahead in [version(1, 1), version(2, 0)] {
        let store = seeded(&[(CONFIG, &stamped_config(ahead)), (PROJECT, PROJECT_OLD)]);
        let (vault, _index) = open(store.clone());
        let before = snapshot(&*store);

        for result in [
            vault.plan_migration(METHOD_VERSION, FILE_MIGRATIONS),
            vault.migrate(at(), METHOD_VERSION, FILE_MIGRATIONS),
        ] {
            match result {
                Err(err @ DomainError::MethodVersionAhead { vault, binary }) => {
                    assert_eq!((vault, binary), (ahead, METHOD_VERSION));
                    let message = err.to_string();
                    for needle in [format!("RLM {ahead}"), "upgrade cdno".to_owned()] {
                        assert!(message.contains(&needle), "{needle:?} not in: {message}");
                    }
                }
                other => panic!("expected MethodVersionAhead for {ahead}, got: {other:?}"),
            }
        }
        assert_eq!(snapshot(&*store), before, "a newer vault was written to");
    }
}
