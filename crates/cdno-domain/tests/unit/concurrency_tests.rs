//! Concurrency regression test for the vault write lock (#196).
//!
//! Multiple processes (here: threads, each with its own `FsVaultStore`
//! fd + `SqliteIndex` connection over one vault dir) hammering the same
//! daily note must not lose log lines. `log_to_daily_note` is a
//! read-modify-write full rewrite; without the cross-process lock,
//! concurrent writers clobber each other. With it, every line survives.
//! The rename (RFC 0007 T20) reads the whole vault under the same lock.

use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use cdno_core::config::VaultConfig;
use cdno_core::error::StoreError;
use cdno_core::file_meta::FileMeta;
use cdno_core::index::{SqliteIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{FsVaultStore, VaultStore, VaultWriteLock};
use cdno_domain::Vault;
use cdno_domain::frontmatter::{Context, EnergyLevel};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use tempfile::tempdir;

/// Build a vault backed by the real filesystem + SQLite at `root`. Each
/// call is an independent `FsVaultStore`/`SqliteIndex` pair, so several
/// of them over one dir model several processes.
fn build_fs_vault(root: &Path) -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(FsVaultStore::new(root));
    let index: Arc<dyn VaultIndex> =
        Arc::new(SqliteIndex::open(root.join(".cuaderno/index.db")).expect("open index"));
    let (vault, _report) =
        Vault::new(store, index, VaultConfig::default()).expect("construct vault");
    vault
}

#[test]
fn concurrent_log_appends_do_not_lose_lines() {
    const WRITERS: usize = 8;
    const PER_WRITER: usize = 5;

    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".cuaderno")).unwrap();

    let at: NaiveDateTime = NaiveDate::from_ymd_opt(2026, 6, 13)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap());

    // Build each writer's vault up front so only the logging races, not
    // vault construction (which reconciles).
    let vaults: Vec<Vault> = (0..WRITERS).map(|_| build_fs_vault(root)).collect();

    let handles: Vec<_> = vaults
        .into_iter()
        .enumerate()
        .map(|(w, vault)| {
            thread::spawn(move || {
                for n in 0..PER_WRITER {
                    vault
                        .log_to_daily_note(at, &format!("writer {w} line {n}"))
                        .expect("log_to_daily_note");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    // Every line must be present — none clobbered by a concurrent rewrite.
    let daily =
        std::fs::read_to_string(root.join("journal/2026/daily/2026-06-13.md")).expect("daily note");
    let got = daily.matches("writer ").count();
    assert_eq!(
        got,
        WRITERS * PER_WRITER,
        "lost log lines under concurrent writers (got {got}, want {}):\n{daily}",
        WRITERS * PER_WRITER
    );
}

#[test]
fn concurrent_add_action_keeps_every_bullet() {
    // A project-body read-modify-write (read the project map, append a
    // bullet to `## Next Actions`, rewrite it) — the surface the 16+
    // reorders protect. Without the lock covering the read, concurrent
    // `add_action`s clobber each other's bullets.
    const WRITERS: usize = 8;

    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".cuaderno")).unwrap();
    let at: NaiveDateTime = NaiveDate::from_ymd_opt(2026, 6, 13)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap());

    // One project to hammer.
    build_fs_vault(root)
        .create_project(at, "Concurrency", Context::Work, None)
        .expect("create project");

    let vaults: Vec<Vault> = (0..WRITERS).map(|_| build_fs_vault(root)).collect();
    let handles: Vec<_> = vaults
        .into_iter()
        .enumerate()
        .map(|(w, vault)| {
            thread::spawn(move || {
                vault
                    .add_action(
                        at,
                        "concurrency",
                        &format!("action {w}"),
                        EnergyLevel::Medium,
                    )
                    .expect("add_action");
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    let project =
        std::fs::read_to_string(root.join("projects/concurrency.md")).expect("project map");
    let got = (0..WRITERS)
        .filter(|w| project.contains(&format!("action {w}")))
        .count();
    assert_eq!(
        got, WRITERS,
        "lost action bullets under concurrent add_action (got {got}, want {WRITERS}):\n{project}"
    );
}

/// A `FsVaultStore` that, once armed, starts a second writer the first
/// time `trigger` is read, and gives it a moment to land before the read
/// returns. The second writer goes through the vault, so it honours the
/// write lock: it lands at once when the lock is free, and waits for the
/// commit when the reader holds it. `RacingStore` (support.rs) writes
/// behind the lock's back, so it cannot tell a read under the lock from
/// one before it; this store can.
struct SecondWriterStore {
    inner: FsVaultStore,
    trigger: VaultPath,
    writer: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl SecondWriterStore {
    fn arm(&self, writer: impl FnOnce() + Send + 'static) {
        *self.writer.lock().unwrap() = Some(Box::new(writer));
    }

    fn join(&self) {
        if let Some(handle) = self.handle.lock().unwrap().take() {
            handle.join().unwrap();
        }
    }
}

impl VaultStore for SecondWriterStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        let read = self.inner.read_file(path);
        if *path == self.trigger
            && let Some(writer) = self.writer.lock().unwrap().take()
        {
            let (done, landed) = mpsc::channel();
            *self.handle.lock().unwrap() = Some(thread::spawn(move || {
                writer();
                let _ = done.send(());
            }));
            // Landed, or blocked on the lock: either way, carry on.
            let _ = landed.recv_timeout(Duration::from_millis(500));
        }
        read
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
    fn import_external(&self, src: &Path, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.import_external(src, dest)
    }
    fn acquire_write_lock(&self) -> Result<VaultWriteLock, StoreError> {
        self.inner.acquire_write_lock()
    }
}

#[test]
fn a_rename_holds_the_lock_for_the_whole_plan() {
    // A second writer appends to today's daily note (which the rename also
    // rewrites) while the rename reads the vault. The snapshot is taken
    // under the lock, so that writer waits for the commit and its line
    // lands after the rename's. Read before the lock, the rename would
    // plan against the daily note without the line and overwrite it: a
    // lost update.
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".cuaderno")).unwrap();
    let at: NaiveDateTime = NaiveDate::from_ymd_opt(2026, 6, 13)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap());
    let daily = "journal/2026/daily/2026-06-13.md";

    let setup = build_fs_vault(root);
    setup
        .create_project(at, "Surrogate model", Context::Work, None)
        .expect("create project");
    setup
        .log_to_daily_note(at, "calibrating [[surrogate-model]]")
        .expect("log");
    drop(setup);

    // Built before the race, so only the writes race (opening reconciles,
    // which takes the lock too).
    let second = build_fs_vault(root);
    let store = Arc::new(SecondWriterStore {
        inner: FsVaultStore::new(root),
        // Read after the daily note in the snapshot's path order.
        trigger: VaultPath::new("projects/surrogate-model.md").unwrap(),
        writer: Mutex::new(None),
        handle: Mutex::new(None),
    });
    let index: Arc<dyn VaultIndex> =
        Arc::new(SqliteIndex::open(root.join(".cuaderno/index.db")).expect("open index"));
    let dyn_store: Arc<dyn VaultStore> = store.clone();
    let (vault, _report) =
        Vault::new(dyn_store, index, VaultConfig::default()).expect("construct vault");

    let plan = vault
        .rename_preview("surrogate-model", "surrogate-calibration", None, at)
        .expect("preview");
    store.arm(move || {
        second
            .log_to_daily_note(at, "the second writer")
            .expect("second writer");
    });
    let outcome = vault.rename_note(
        "surrogate-model",
        "surrogate-calibration",
        None,
        Some(&plan.hash),
        at,
        || Ok(()),
    );
    store.join();
    outcome.expect("the rename wrote the plan it previewed");

    let text = std::fs::read_to_string(root.join(daily)).expect("daily note");
    let renamed = text
        .find("renamed [[surrogate-calibration]]")
        .unwrap_or_else(|| panic!("no rename line:\n{text}"));
    let second_line = text
        .find("the second writer")
        .unwrap_or_else(|| panic!("the second writer's line was lost:\n{text}"));
    assert!(renamed < second_line, "{text}");
    assert!(
        text.contains("calibrating [[surrogate-calibration]]"),
        "{text}"
    );
}

/// Whether another holder has the vault's write lock right now: a fresh
/// open of `.cuaderno/.lock` that cannot take it means someone holds it.
fn lock_is_held(root: &Path) -> bool {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(root.join(".cuaderno/.lock"))
        .expect("open the lock file");
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(e) => panic!("probing the lock: {e:?}"),
    }
}

#[test]
fn the_guard_runs_under_the_lock_before_planning() {
    // RFC 0007 §5.8 (T25): the interface's paused-operation re-check runs
    // once the lock is held, and a refusal from it returns before the vault
    // is planned or anything staged. The target here does not exist, so a
    // guard run after planning would be beaten by `NoteNotFound`.
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".cuaderno")).unwrap();
    let at: NaiveDateTime = NaiveDate::from_ymd_opt(2026, 6, 13)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap());
    let vault = build_fs_vault(root);
    vault
        .create_project(at, "Surrogate model", Context::Work, None)
        .expect("create project");
    assert!(
        !lock_is_held(root),
        "nothing holds the lock before the call"
    );

    let held = std::cell::Cell::new(None);
    let err = vault
        .rename_note(
            "no-such-note",
            "surrogate-calibration",
            None,
            None,
            at,
            || {
                held.set(Some(lock_is_held(root)));
                Err(cdno_domain::error::DomainError::GitOperationInProgress {
                    reason: "a merge is in progress (.git/MERGE_HEAD present)".into(),
                })
            },
        )
        .expect_err("the guard refuses");
    assert_eq!(held.get(), Some(true), "the guard ran, under the lock");
    assert!(
        matches!(
            err,
            cdno_domain::error::DomainError::GitOperationInProgress { .. }
        ),
        "the guard's error comes back before planning: {err:?}"
    );

    // And a guard refusal on a real target writes nothing.
    let before = std::fs::read(root.join("projects/surrogate-model.md")).unwrap();
    vault
        .rename_note(
            "surrogate-model",
            "surrogate-calibration",
            None,
            None,
            at,
            || {
                Err(cdno_domain::error::DomainError::GitOperationInProgress {
                    reason: "a rebase is in progress (.git/rebase-merge present)".into(),
                })
            },
        )
        .expect_err("the guard refuses");
    assert_eq!(
        std::fs::read(root.join("projects/surrogate-model.md")).unwrap(),
        before
    );
    assert!(!root.join("projects/surrogate-calibration.md").exists());
    assert!(!lock_is_held(root), "the lock is released after a refusal");
}
