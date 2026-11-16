//! The vault write lock every kernel sees (#1012): a lock file created
//! exclusively in `.cuaderno/locks/`, taken after the `flock` and held for
//! the life of the transaction.
//!
//! Two stores over one folder with their own `flock` files
//! ([`FsVaultStore::with_lock_file`], which also gives each its own kernel
//! boot identifier) stand in for two kernels, which do not see each other's
//! `flock`: only the lock file can exclude them. A lock is made stale by
//! setting its mtime into the past, which is what its age is read from
//! (against a probe file made in the same folder).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use cdno_core::error::{StoreError, TransactionError};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::shared_lock::{BreakReason, LOCK_NAME, LOCK_PATH, LOCKS_DIR, STALE_AFTER};
use cdno_core::store::{ForcedAttrs, FsVaultStore, VaultStore, VaultWriteLock};
use cdno_core::transaction::VaultTransaction;
use tempfile::TempDir;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

const NOTE: &str = "journal/2026/daily/2026-06-13.md";

/// A vault folder, and a directory outside it for the writers' private
/// `flock` files.
struct Folder {
    vault: TempDir,
    flocks: TempDir,
}

impl Folder {
    fn new() -> Self {
        let vault = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(vault.path().join(LOCKS_DIR)).unwrap();
        Self {
            vault,
            flocks: tempfile::tempdir().unwrap(),
        }
    }

    fn root(&self) -> &Path {
        self.vault.path()
    }

    /// A store that stands for a writer on a kernel of its own: its own
    /// `flock` and boot identifier, the shared lock file.
    fn kernel(&self, name: &str) -> FsVaultStore {
        FsVaultStore::new(self.root()).with_lock_file(self.flocks.path().join(name))
    }

    fn locks(&self) -> PathBuf {
        self.root().join(LOCKS_DIR)
    }

    fn lock(&self) -> PathBuf {
        self.locks().join(LOCK_NAME)
    }

    fn lock_record(&self) -> Option<String> {
        std::fs::read_to_string(self.lock()).ok()
    }

    /// Every file in the locks folder, the lock included, but not the
    /// `.gitignore` the folder keeps for itself.
    fn lock_files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.locks())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != ".gitignore")
            .collect();
        names.sort();
        names
    }

    /// Make the lock (or any file) look `age` old.
    fn age(&self, path: &Path, age: Duration) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    fn mtime(&self, path: &Path) -> SystemTime {
        std::fs::metadata(path).unwrap().modified().unwrap()
    }

    fn put(&self, path: &str, content: &str) {
        let full = self.root().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }

    fn get(&self, path: &str) -> String {
        std::fs::read_to_string(self.root().join(path)).unwrap()
    }

    /// `dev:ino` of the private `flock` file `name`, created if absent, as
    /// a store over it writes it into its record.
    fn flock_id(&self, name: &str) -> String {
        use std::os::unix::fs::MetadataExt;
        let path = self.flocks.path().join(name);
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        format!("{}:{}", meta.dev(), meta.ino())
    }

    /// A lock file left by a writer that died.
    fn dead_lock(&self, lock: &DeadLock<'_>) {
        std::fs::write(
            self.lock(),
            format!(
                "cuaderno vault write lock\nprotocol: {}\ntoken: 00ff\nprogram: cdno 0.0.0\n\
                 process: {}\ninstance: {}\nboot: {}\nflock: {}\n\
                 taken: 2026-06-13T09:00:00+00:00\n",
                lock.protocol, lock.process, lock.instance, lock.boot, lock.flock
            ),
        )
        .unwrap();
        self.age(&self.lock(), lock.age);
    }

    /// A lock file left `age` ago by a writer on a kernel no test store
    /// runs on.
    fn crashed_lock(&self, age: Duration) {
        self.dead_lock(&DeadLock {
            age,
            ..DeadLock::default()
        });
    }
}

/// The record of a dead writer's lock, for [`Folder::dead_lock`].
struct DeadLock<'a> {
    protocol: u32,
    boot: &'a str,
    flock: &'a str,
    instance: &'a str,
    process: &'a str,
    age: Duration,
}

impl Default for DeadLock<'_> {
    fn default() -> Self {
        Self {
            protocol: 1,
            boot: "some-other-kernel",
            flock: "0:0",
            instance: "another-process",
            process: "1",
            age: Duration::ZERO,
        }
    }
}

/// The value of `key` in a lock's record.
fn record_value(record: &str, key: &str) -> String {
    record
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{key}: ")))
        .unwrap_or_default()
        .to_owned()
}

fn tx(store: &Arc<FsVaultStore>) -> Result<VaultTransaction, StoreError> {
    VaultTransaction::new(
        store.clone() as Arc<dyn VaultStore>,
        Arc::new(MemoryIndex::new()) as Arc<dyn VaultIndex>,
    )
}

const OLD: Duration = Duration::from_secs(STALE_AFTER.as_secs() * 4);

/// A two-party rendezvous that fails instead of hanging when the other
/// party never arrives.
#[derive(Clone, Default)]
struct Meet(Arc<(Mutex<usize>, Condvar)>);

impl Meet {
    fn arrive(&self) {
        let (count, cv) = &*self.0;
        let mut n = count.lock().unwrap();
        *n += 1;
        cv.notify_all();
        let (n, timeout) = cv
            .wait_timeout_while(n, Duration::from_secs(10), |n| *n < 2)
            .unwrap();
        assert!(
            !timeout.timed_out() || *n >= 2,
            "the other party never arrived"
        );
    }
}

// ---------------------------------------------------------------------
// Exclusion across kernels
// ---------------------------------------------------------------------

#[test]
fn two_writers_with_independent_flocks_exclude_each_other() {
    let folder = Folder::new();
    let a = Arc::new(folder.kernel("a.lock"));
    let b = Arc::new(folder.kernel("b.lock"));
    let held = tx(&a).unwrap();

    let started = Instant::now();
    let waiter = std::thread::spawn(move || {
        let t = tx(&b).expect("the second writer gets the lock once the first lets go");
        (started.elapsed(), t)
    });
    std::thread::sleep(Duration::from_millis(300));
    drop(held);
    let (waited, second) = waiter.join().unwrap();
    assert!(
        waited >= Duration::from_millis(300),
        "the second writer took the lock after {waited:?}, while the first held it"
    );
    drop(second);
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());
}

#[test]
fn the_lock_holds_its_owner_record_from_the_moment_it_exists() {
    let folder = Folder::new();
    let store = folder.kernel("a.lock").with_boot_id(Some("kernel-a"));
    let lock = store.acquire_write_lock().unwrap();
    let record = folder
        .lock_record()
        .expect("the lock file exists while held");
    assert!(
        record.starts_with("cuaderno vault write lock\n"),
        "{record}"
    );
    for key in [
        "protocol: 1\n",
        "token: ",
        "program: ",
        "process: ",
        "instance: ",
        "boot: kernel-a\n",
        &format!("flock: {}\n", folder.flock_id("a.lock")),
        "taken: ",
    ] {
        assert!(record.contains(key), "{key:?} missing from {record}");
    }
    // Only the lock itself: the temp file its record was written in is gone.
    assert_eq!(folder.lock_files(), vec![LOCK_NAME.to_owned()]);
    assert!(lock.broke_lock().is_none());
    assert!(lock.has_lock_file());
}

#[test]
fn a_store_that_predates_the_lock_file_takes_only_the_flock() {
    let folder = Folder::new();
    let store = folder.kernel("a.lock").without_shared_lock();
    let lock = store.acquire_write_lock().unwrap();
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());
    assert!(!lock.has_lock_file());
}

#[test]
fn an_index_lock_takes_only_the_flock() {
    // Reconciliation's holders write the index and never a note: they
    // neither pay for the lock file nor keep another kernel waiting.
    let folder = Folder::new();
    let other = folder.kernel("b.lock");
    let _note_writer = other.acquire_write_lock().unwrap();
    let started = Instant::now();
    let lock = folder
        .kernel("a.lock")
        .acquire_index_lock()
        .expect("the lock file another kernel holds does not stop an index holder");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(!lock.has_lock_file());
    assert_eq!(folder.lock_files(), vec![LOCK_NAME.to_owned()]);
}

#[test]
#[should_panic(expected = "index-only transaction")]
fn an_index_only_transaction_refuses_a_file_op() {
    let folder = Folder::new();
    let store: Arc<dyn VaultStore> = Arc::new(folder.kernel("a.lock"));
    let mut t =
        VaultTransaction::index_only(store, Arc::new(MemoryIndex::new()) as Arc<dyn VaultIndex>)
            .unwrap();
    t.write_file(vp("inbox/x.md"), "x\n");
}

// ---------------------------------------------------------------------
// Stale and dead locks
// ---------------------------------------------------------------------

#[test]
fn a_stale_lock_is_broken_by_exactly_one_of_two_contenders() {
    let folder = Folder::new();
    folder.crashed_lock(OLD);

    // Both contenders judge the lock stale, and look once more, before
    // either renames it aside.
    let meet = Meet::default();
    let contender = |name: &str| {
        let meet = meet.clone();
        let first = AtomicBool::new(true);
        Arc::new(folder.kernel(name).with_before_break_hook(move || {
            if first.swap(false, Ordering::SeqCst) {
                meet.arrive();
            }
        }))
    };
    let stores = [contender("a.lock"), contender("b.lock")];
    let handles: Vec<_> = stores
        .iter()
        .map(|store| {
            let store = Arc::clone(store);
            std::thread::spawn(move || {
                let lock = store
                    .acquire_write_lock()
                    .expect("each contender gets the lock");
                std::thread::sleep(Duration::from_millis(50));
                lock.broke_lock()
            })
        })
        .collect();
    let broke: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(
        broke.iter().filter(|b| b.is_some()).count(),
        1,
        "exactly one contender breaks the stale lock: {broke:?}"
    );
    assert!(broke.contains(&Some(BreakReason::Age)), "{broke:?}");
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());
}

#[test]
fn a_young_lock_is_never_broken() {
    let folder = Folder::new();
    let holder = folder.kernel("a.lock");
    let held = holder.acquire_write_lock().unwrap();
    let record = folder.lock_record().unwrap();
    let other = folder
        .kernel("b.lock")
        .with_lock_timeout(Duration::from_millis(400));

    // Fresh, and just under the stale interval: busy both times.
    for age in [Duration::ZERO, STALE_AFTER - Duration::from_secs(5)] {
        if !age.is_zero() {
            folder.age(&folder.lock(), age);
        }
        match other.acquire_write_lock() {
            Err(StoreError::LockBusy {
                lock,
                age_secs,
                stale_after_secs,
                ..
            }) => {
                assert_eq!(lock, LOCK_PATH);
                assert!(age_secs < STALE_AFTER.as_secs(), "{age_secs}");
                assert!(age_secs + 2 >= age.as_secs(), "{age_secs} for {age:?}");
                assert_eq!(stale_after_secs, STALE_AFTER.as_secs());
            }
            other => panic!("expected the lock busy, got {other:?}"),
        }
        assert_eq!(folder.lock_record().as_deref(), Some(record.as_str()));
    }
    // The ownership check would refresh it; this test is about the waiter.
    assert!(
        held.still_held().unwrap(),
        "the holder still holds its lock"
    );
    assert_eq!(folder.lock_files(), vec![LOCK_NAME.to_owned()]);
}

#[test]
fn the_busy_error_names_the_lock_its_age_and_its_owner() {
    let folder = Folder::new();
    let _held = folder.kernel("a.lock").acquire_write_lock().unwrap();
    // Half a second past a whole number, so a filesystem that stamps the
    // probe with a coarse clock (Linux) still reads 12.
    folder.age(&folder.lock(), Duration::from_millis(12_500));
    let err = folder
        .kernel("b.lock")
        .with_lock_timeout(Duration::from_millis(100))
        .acquire_write_lock()
        .unwrap_err();
    let message = err.to_string();
    for part in [
        "waiting for the vault write lock",
        LOCK_PATH,
        &format!("(process {}", std::process::id()),
        "12s old",
        &format!("{}s old", STALE_AFTER.as_secs()),
    ] {
        assert!(message.contains(part), "{part:?} missing from: {message}");
    }
}

#[test]
fn a_crashed_holders_old_leftovers_go_with_its_lock_and_fresh_ones_stay() {
    let folder = Folder::new();
    folder.crashed_lock(OLD);
    let old = ["abc.new", "def.probe", "0123.stale"];
    for name in old {
        let path = folder.locks().join(name);
        std::fs::write(&path, "").unwrap();
        folder.age(&path, OLD + Duration::from_secs(1));
    }
    // A live contender's temp and probe files are seconds old at most, and
    // a file that is not one of the protocol's is never touched, however
    // old: the folder's contract with later releases.
    let fresh = ["live.new", "live.probe"];
    for name in fresh {
        std::fs::write(folder.locks().join(name), "").unwrap();
    }
    let foreign = folder.locks().join("write.lock.sync-conflict");
    std::fs::write(&foreign, "").unwrap();
    folder.age(&foreign, OLD + Duration::from_secs(1));
    let lock = folder
        .kernel("a.lock")
        .acquire_write_lock()
        .expect("a stale lock does not hold a writer up");
    assert_eq!(lock.broke_lock(), Some(BreakReason::Age));
    let mut expected = vec![LOCK_NAME.to_owned(), "write.lock.sync-conflict".to_owned()];
    expected.extend(fresh.iter().map(|s| (*s).to_owned()));
    expected.sort();
    assert_eq!(folder.lock_files(), expected);
    assert!(
        folder.locks().join(".gitignore").exists(),
        "the folder ignores itself in git"
    );
}

#[test]
fn the_locks_folder_ignores_itself_in_git_and_its_gitignore_is_never_rewritten() {
    let folder = Folder::new();
    let ignore = folder.locks().join(".gitignore");
    drop(folder.kernel("a.lock").acquire_write_lock().unwrap());
    let written = std::fs::read_to_string(&ignore).unwrap();
    assert!(written.lines().any(|l| l == "*"), "{written}");
    std::fs::write(&ignore, "# kept by hand\n*\n").unwrap();
    drop(folder.kernel("a.lock").acquire_write_lock().unwrap());
    assert_eq!(
        std::fs::read_to_string(&ignore).unwrap(),
        "# kept by hand\n*\n"
    );
}

#[test]
fn a_dead_holder_through_the_same_flock_is_broken_at_once() {
    // The contender holds the very `flock` file the dead holder locked,
    // which a live holder would still hold: its lock is broken however
    // young.
    let folder = Folder::new();
    let flock = folder.flock_id("a.lock");
    folder.dead_lock(&DeadLock {
        boot: "kernel-a",
        flock: &flock,
        instance: "another-process",
        ..DeadLock::default()
    });
    let started = Instant::now();
    let lock = folder
        .kernel("a.lock")
        .with_boot_id(Some("kernel-a"))
        .acquire_write_lock()
        .expect("a dead holder's lock does not hold a writer up");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(lock.broke_lock(), Some(BreakReason::DeadOnThisKernel));
}

#[test]
fn a_restarted_server_with_the_same_pid_breaks_its_predecessors_lock_at_once() {
    // A server in a container is often process 1 again after a restart:
    // the instance value, not the pid, says it is another process.
    let folder = Folder::new();
    let flock = folder.flock_id("a.lock");
    let pid = std::process::id().to_string();
    folder.dead_lock(&DeadLock {
        boot: "kernel-a",
        flock: &flock,
        instance: "the-previous-run",
        process: &pid,
        ..DeadLock::default()
    });
    let lock = folder
        .kernel("a.lock")
        .with_boot_id(Some("kernel-a"))
        .with_lock_timeout(Duration::from_millis(500))
        .acquire_write_lock()
        .expect("broken at once");
    assert_eq!(lock.broke_lock(), Some(BreakReason::DeadOnThisKernel));
}

#[test]
fn a_lock_this_process_left_behind_is_broken_at_once() {
    // A release that failed leaves this process's own lock: its token is
    // not among the locks it holds, so it is dead.
    let folder = Folder::new();
    let store = folder.kernel("a.lock").with_boot_id(Some("kernel-a"));
    let instance = {
        let lock = store.acquire_write_lock().unwrap();
        let record = folder.lock_record().unwrap();
        drop(lock);
        record_value(&record, "instance")
    };
    let flock = folder.flock_id("a.lock");
    folder.dead_lock(&DeadLock {
        boot: "kernel-a",
        flock: &flock,
        instance: &instance,
        ..DeadLock::default()
    });
    let lock = store
        .with_lock_timeout(Duration::from_millis(500))
        .acquire_write_lock()
        .expect("broken at once");
    assert_eq!(lock.broke_lock(), Some(BreakReason::DeadOnThisKernel));
}

#[test]
fn a_lock_this_process_still_holds_is_not_taken_for_a_dead_one() {
    // Where `flock` is emulated per process (Linux NFS), two threads of one
    // process can both hold it: a record of this process instance whose
    // lock this process still holds is live. Here the record is made to
    // name the contender's `flock` file, as it would on such a filesystem.
    let folder = Folder::new();
    let holder = folder.kernel("a.lock").with_boot_id(Some("kernel-a"));
    let held = holder.acquire_write_lock().unwrap();
    let record = folder.lock_record().unwrap();
    let b_flock = folder.flock_id("b.lock");
    let rewritten: String = record
        .lines()
        .map(|l| {
            if l.starts_with("flock: ") {
                format!("flock: {b_flock}\n")
            } else {
                format!("{l}\n")
            }
        })
        .collect();
    std::fs::write(folder.lock(), rewritten).unwrap();
    let err = folder
        .kernel("b.lock")
        .with_boot_id(Some("kernel-a"))
        .with_lock_timeout(Duration::from_millis(300))
        .acquire_write_lock()
        .unwrap_err();
    assert!(matches!(err, StoreError::LockBusy { .. }), "{err:?}");
    assert!(held.still_held().unwrap());
}

#[test]
fn a_live_holder_through_another_flock_file_is_not_broken_at_once() {
    // One kernel, two `flock`s on "the" file: two mounts whose locks do not
    // meet, or a `.lock` replaced under a holder. A record from another
    // process instance on this boot, which locked `a.lock`: a contender
    // through `b.lock` does not hold that `flock`, so it cannot know the
    // holder is dead and waits for the lock's age; one through `a.lock`
    // does, and breaks it at once.
    let folder = Folder::new();
    let a_flock = folder.flock_id("a.lock");
    folder.dead_lock(&DeadLock {
        boot: "kernel-a",
        flock: &a_flock,
        instance: "another-process",
        ..DeadLock::default()
    });
    let err = folder
        .kernel("b.lock")
        .with_boot_id(Some("kernel-a"))
        .with_lock_timeout(Duration::from_millis(300))
        .acquire_write_lock()
        .unwrap_err();
    assert!(matches!(err, StoreError::LockBusy { .. }), "{err:?}");
    assert!(folder.lock_record().unwrap().contains("token: 00ff"));

    let lock = folder
        .kernel("a.lock")
        .with_boot_id(Some("kernel-a"))
        .with_lock_timeout(Duration::from_millis(300))
        .acquire_write_lock()
        .expect("broken at once through the same flock");
    assert_eq!(lock.broke_lock(), Some(BreakReason::DeadOnThisKernel));
}

#[test]
fn a_young_lock_from_another_boot_waits_for_its_age() {
    let folder = Folder::new();
    let flock = folder.flock_id("a.lock");
    folder.dead_lock(&DeadLock {
        boot: "kernel-b",
        flock: &flock,
        instance: "another-process",
        ..DeadLock::default()
    });
    let err = folder
        .kernel("a.lock")
        .with_boot_id(Some("kernel-a"))
        .with_lock_timeout(Duration::from_millis(300))
        .acquire_write_lock()
        .unwrap_err();
    assert!(matches!(err, StoreError::LockBusy { .. }), "{err:?}");
    assert!(folder.lock_record().unwrap().contains("boot: kernel-b"));
}

#[test]
fn a_lock_of_a_newer_protocol_is_never_taken_by_the_fast_path() {
    let folder = Folder::new();
    let flock = folder.flock_id("a.lock");
    folder.dead_lock(&DeadLock {
        protocol: 2,
        boot: "kernel-a",
        flock: &flock,
        instance: "another-process",
        ..DeadLock::default()
    });
    let err = folder
        .kernel("a.lock")
        .with_boot_id(Some("kernel-a"))
        .with_lock_timeout(Duration::from_millis(300))
        .acquire_write_lock()
        .unwrap_err();
    let message = err.to_string();
    assert!(matches!(err, StoreError::LockBusy { .. }), "{err:?}");
    assert!(
        message.contains("version 2 of the lock's rules"),
        "{message}"
    );
}

// ---------------------------------------------------------------------
// The break race: a live lock moved aside by mistake
// ---------------------------------------------------------------------

/// Contender C judges a crashed lock stale; before C's rename, D breaks it
/// and takes a lock of its own. C's rename then moves D's live lock aside,
/// and D checks its lock in the gap. The put-back is held until D's check
/// has seen the name missing, so D's second look is what lets it commit.
#[test]
fn a_live_lock_moved_aside_by_mistake_is_put_back_and_its_holder_commits() {
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    folder.crashed_lock(OLD);

    let (to_d, d_rx) = mpsc::channel::<&'static str>();
    let (to_c, c_rx) = mpsc::channel::<&'static str>();
    let (missing_tx, missing_rx) = mpsc::channel::<()>();
    let moved_wrong = Arc::new(AtomicBool::new(false));
    let saw_missing = Arc::new(AtomicBool::new(false));
    let c_rx = Arc::new(Mutex::new(c_rx));
    let missing_rx = Arc::new(Mutex::new(missing_rx));
    let c = {
        let to_d_before = to_d.clone();
        let to_d_after = to_d.clone();
        let c_rx = Arc::clone(&c_rx);
        let missing_rx = Arc::clone(&missing_rx);
        let moved_wrong = Arc::clone(&moved_wrong);
        let first = AtomicBool::new(true);
        Arc::new(
            folder
                .kernel("c.lock")
                .with_before_break_hook(move || {
                    if first.swap(false, Ordering::SeqCst) {
                        // Let D break the lock and take its own first.
                        to_d_before.send("break it").unwrap();
                        let got = c_rx
                            .lock()
                            .unwrap()
                            .recv_timeout(Duration::from_secs(10))
                            .expect("D took the lock");
                        assert_eq!(got, "holding");
                    }
                })
                .with_after_wrong_move_hook(move || {
                    moved_wrong.store(true, Ordering::SeqCst);
                    // D commits now, while its lock is aside; it is put back
                    // only once D has found the name missing.
                    to_d_after.send("commit").unwrap();
                    missing_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(10))
                        .expect("D's check found its lock's name missing");
                }),
        )
    };
    let d = {
        let saw_missing = Arc::clone(&saw_missing);
        let first = AtomicBool::new(true);
        Arc::new(folder.kernel("d.lock").with_on_name_missing_hook(move || {
            saw_missing.store(true, Ordering::SeqCst);
            if first.swap(false, Ordering::SeqCst) {
                missing_tx.send(()).unwrap();
            }
        }))
    };

    let d_thread = {
        let d = Arc::clone(&d);
        std::thread::spawn(move || {
            assert_eq!(
                d_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
                "break it"
            );
            let mut t = tx(&d).expect("D breaks the crashed lock");
            let read = d.read_file(&vp(NOTE)).unwrap();
            t.write_file(vp(NOTE), format!("{read}line from d\n"));
            to_c.send("holding").unwrap();
            assert_eq!(
                d_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
                "commit"
            );
            t.commit()
        })
    };
    let c_lock = c
        .acquire_write_lock()
        .expect("C gets the lock after D lets go");
    let committed = d_thread.join().unwrap();
    assert!(
        moved_wrong.load(Ordering::SeqCst),
        "C moved D's live lock aside"
    );
    assert!(
        saw_missing.load(Ordering::SeqCst),
        "D's check found the name missing and looked again"
    );
    committed.expect("D's commit goes through: its lock was put back");
    assert_eq!(folder.get(NOTE), "base\nline from d\n");
    assert!(c_lock.broke_lock().is_none(), "C broke nothing");
    drop(c_lock);
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());
}

/// The same race, but a third writer takes the name while D's lock is
/// aside: the put-back never replaces it, and D refuses rather than write
/// beside the third.
#[test]
fn a_lock_put_back_never_replaces_a_third_writers_lock() {
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    folder.crashed_lock(OLD);
    let lock_path = folder.lock();

    let (to_d, d_rx) = mpsc::channel::<&'static str>();
    let (to_c, c_rx) = mpsc::channel::<&'static str>();
    let c_rx = Arc::new(Mutex::new(c_rx));
    let c = {
        let c_rx = Arc::clone(&c_rx);
        let first = AtomicBool::new(true);
        Arc::new(
            folder
                .kernel("c.lock")
                .with_lock_timeout(Duration::from_millis(500))
                .with_before_break_hook(move || {
                    if first.swap(false, Ordering::SeqCst) {
                        to_d.send("break it").unwrap();
                        c_rx.lock()
                            .unwrap()
                            .recv_timeout(Duration::from_secs(10))
                            .expect("D took the lock");
                    }
                })
                .with_after_wrong_move_hook(move || {
                    std::fs::write(
                        &lock_path,
                        "cuaderno vault write lock\ntoken: third\nprogram: cdno 0.0.0\n\
                         process: 1\ntaken: 2026-06-13T09:00:00+00:00\n",
                    )
                    .unwrap();
                }),
        )
    };
    let d = Arc::new(folder.kernel("d.lock"));
    let d_thread = {
        let d = Arc::clone(&d);
        std::thread::spawn(move || {
            assert_eq!(
                d_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
                "break it"
            );
            let mut t = tx(&d).unwrap();
            let read = d.read_file(&vp(NOTE)).unwrap();
            t.write_file(vp(NOTE), format!("{read}line from d\n"));
            to_c.send("holding").unwrap();
            t
        })
    };
    // C waits on the third writer's young lock and gives up.
    let busy = c.acquire_write_lock().unwrap_err();
    assert!(matches!(busy, StoreError::LockBusy { .. }), "{busy:?}");
    let t = d_thread.join().unwrap();
    match t.commit() {
        Err(TransactionError::LockLost {
            rollback_failures, ..
        }) => assert!(rollback_failures.is_empty()),
        other => panic!("expected D's commit refused for a lost lock, got {other:?}"),
    }
    assert_eq!(folder.get(NOTE), "base\n");
    assert!(folder.lock_record().unwrap().contains("token: third"));
    assert_eq!(folder.lock_files(), vec![LOCK_NAME.to_owned()]);
}

// ---------------------------------------------------------------------
// A holder whose lock was broken
// ---------------------------------------------------------------------

#[test]
fn a_broken_holder_refuses_to_commit_and_leaves_the_successors_lock() {
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    let a = Arc::new(folder.kernel("a.lock"));
    let b = folder.kernel("b.lock");

    let mut suspended = tx(&a).unwrap();
    let read = a.read_file(&vp(NOTE)).unwrap();
    suspended.write_file(vp(NOTE), format!("{read}line from a\n"));

    // The holder sleeps past the stale interval; the next writer breaks
    // its lock and takes one of its own.
    folder.age(&folder.lock(), OLD);
    let successor = b.acquire_write_lock().unwrap();
    assert_eq!(successor.broke_lock(), Some(BreakReason::Age));
    let record = folder.lock_record().unwrap();

    let err = suspended.commit().unwrap_err();
    let message = err.to_string();
    match err {
        TransactionError::LockLost {
            lock,
            rollback_failures,
        } => {
            assert_eq!(lock, LOCK_PATH);
            assert!(rollback_failures.is_empty(), "{rollback_failures:?}");
        }
        other => panic!("expected the commit refused for a lost lock, got {other:?}"),
    }
    assert!(message.contains("nothing it wrote was kept"), "{message}");
    assert_eq!(
        folder.get(NOTE),
        "base\n",
        "the broken holder wrote nothing"
    );
    // Its release left the successor's lock alone.
    assert_eq!(folder.lock_record().as_deref(), Some(record.as_str()));
    assert!(successor.still_held().unwrap());
    drop(successor);
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());
}

/// A store whose `before_compare` hook, on its `nth` call, ages this
/// store's lock and has another kernel break it: the holder is "suspended"
/// after it wrote and synced the temp file of that op.
fn broken_at_write(
    folder: &Folder,
    nth: usize,
) -> (Arc<FsVaultStore>, Arc<Mutex<Option<VaultWriteLock>>>) {
    let lock_path = folder.lock();
    let b = folder.kernel("b.lock");
    let successor: Arc<Mutex<Option<VaultWriteLock>>> = Arc::new(Mutex::new(None));
    let calls = AtomicUsize::new(0);
    let store = {
        let successor = Arc::clone(&successor);
        Arc::new(folder.kernel("a.lock").with_before_compare_hook(move || {
            if calls.fetch_add(1, Ordering::SeqCst) + 1 != nth {
                return;
            }
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(&lock_path)
                .unwrap();
            file.set_modified(SystemTime::now() - OLD).unwrap();
            *successor.lock().unwrap() = Some(b.acquire_write_lock().unwrap());
        }))
    };
    (store, successor)
}

#[test]
fn a_lock_broken_after_the_temp_file_is_synced_stops_the_rename() {
    // The late check, right before the comparison and the rename: the op
    // in flight when the holder was suspended does not land.
    let folder = Folder::new();
    folder.put("a.md", "a\n");
    let (a, successor) = broken_at_write(&folder, 1);
    let mut t = tx(&a).unwrap();
    let read = a.read_file(&vp("a.md")).unwrap();
    t.write_file(vp("a.md"), format!("{read}more\n"));
    match t.commit() {
        Err(TransactionError::LockLost {
            rollback_failures, ..
        }) => assert!(rollback_failures.is_empty(), "{rollback_failures:?}"),
        other => panic!("expected the commit stopped for a lost lock, got {other:?}"),
    }
    assert_eq!(folder.get("a.md"), "a\n", "the rename never ran");
    let successor = successor.lock().unwrap().take().expect("the successor ran");
    assert!(successor.still_held().unwrap());
    drop(successor);
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());
}

#[test]
fn a_lock_broken_between_two_file_ops_rolls_the_first_back() {
    // The first write lands; the holder is suspended inside the second,
    // whose late check stops it, and the first is undone.
    let folder = Folder::new();
    folder.put("a.md", "a\n");
    folder.put("b.md", "b\n");
    let (a, successor) = broken_at_write(&folder, 2);
    let mut t = tx(&a).unwrap();
    let read_a = a.read_file(&vp("a.md")).unwrap();
    let read_b = a.read_file(&vp("b.md")).unwrap();
    t.write_file(vp("a.md"), format!("{read_a}more\n"));
    t.write_file(vp("b.md"), format!("{read_b}more\n"));
    match t.commit() {
        Err(TransactionError::LockLost {
            rollback_failures, ..
        }) => assert!(rollback_failures.is_empty(), "{rollback_failures:?}"),
        other => panic!("expected the commit stopped for a lost lock, got {other:?}"),
    }
    assert_eq!(folder.get("a.md"), "a\n", "the first write was rolled back");
    assert_eq!(folder.get("b.md"), "b\n", "the second never landed");
    drop(successor.lock().unwrap().take().expect("the successor ran"));
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());
}

#[test]
fn the_ownership_check_refreshes_a_long_held_lock() {
    let folder = Folder::new();
    let store = folder.kernel("a.lock").with_refresh_after(Duration::ZERO);
    let lock = store.acquire_write_lock().unwrap();
    folder.age(&folder.lock(), STALE_AFTER - Duration::from_secs(2));
    let aged = folder.mtime(&folder.lock());
    assert!(lock.still_held().unwrap());
    let refreshed = folder.mtime(&folder.lock());
    assert!(
        refreshed.duration_since(aged).unwrap_or_default().as_secs() >= STALE_AFTER.as_secs() - 3,
        "the lock's mtime was set to now"
    );
    // So another kernel finds it young.
    let err = folder
        .kernel("b.lock")
        .with_lock_timeout(Duration::from_millis(200))
        .acquire_write_lock()
        .unwrap_err();
    match err {
        StoreError::LockBusy { age_secs, .. } => assert!(age_secs < 3, "{age_secs}"),
        other => panic!("expected busy, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// Reads that guard against a share's stale name cache
// ---------------------------------------------------------------------

#[test]
fn a_move_never_replaces_a_file_that_appeared_after_its_check() {
    // The check before a move may be answered from a share's cache; the
    // rename itself refuses to replace, decided by the file server.
    let folder = Folder::new();
    folder.put("inbox/moving.md", "moving\n");
    let dest = folder.root().join("inbox/moved.md");
    let store = {
        let dest = dest.clone();
        FsVaultStore::new(folder.root()).with_before_move_hook(move || {
            std::fs::write(&dest, "made elsewhere\n").unwrap();
        })
    };
    let err = store
        .move_file(&vp("inbox/moving.md"), &vp("inbox/moved.md"))
        .unwrap_err();
    assert!(matches!(err, StoreError::AlreadyExists(_)), "{err:?}");
    assert_eq!(folder.get("inbox/moved.md"), "made elsewhere\n");
    assert_eq!(
        folder.get("inbox/moving.md"),
        "moving\n",
        "the source stays"
    );
}

#[test]
fn a_move_onto_a_free_name_still_moves() {
    let folder = Folder::new();
    folder.put("inbox/moving.md", "moving\n");
    let store = FsVaultStore::new(folder.root());
    store
        .move_file(&vp("inbox/moving.md"), &vp("inbox/sub/moved.md"))
        .unwrap();
    assert_eq!(folder.get("inbox/sub/moved.md"), "moving\n");
    assert!(!folder.root().join("inbox/moving.md").exists());
}

/// The file server's answer for `path` as a local disk gives it: what a
/// forced `statx` would return there.
fn true_attrs(path: &Path) -> Option<ForcedAttrs> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::symlink_metadata(path).ok()?;
    Some(ForcedAttrs {
        nlink: m.nlink(),
        size: m.size(),
        dev: m.dev(),
        ino: m.ino(),
        mtime: (m.mtime(), m.mtime_nsec() as u32),
        ctime: (m.ctime(), m.ctime_nsec() as u32),
    })
}

#[test]
fn a_read_shorter_than_the_size_the_server_reports_is_read_again() {
    // A VM share was measured applying the replaced file's cached size to
    // the new file, cutting a read of the new note to the old length. Here
    // the server's forced answer says the file is longer than what was
    // read, twice, as it would while the client's size is stale; the read
    // is tried again until the two agree.
    let folder = Folder::new();
    folder.put(NOTE, "base\nlonger now\n");
    let calls = Arc::new(AtomicUsize::new(0));
    let store = {
        let calls = Arc::clone(&calls);
        folder
            .kernel("a.lock")
            .with_forced_attrs_hook(move |path, descriptor| {
                let mut attrs = true_attrs(path)?;
                if descriptor && calls.fetch_add(1, Ordering::SeqCst) < 2 {
                    attrs.size += 1;
                }
                Some(attrs)
            })
    };
    assert_eq!(store.read_file(&vp(NOTE)).unwrap(), "base\nlonger now\n");
    assert!(
        calls.load(Ordering::SeqCst) >= 3,
        "the read was tried again"
    );
}

#[test]
fn a_size_the_server_keeps_contradicting_fails_the_read_rather_than_return_it() {
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    let store = folder
        .kernel("a.lock")
        .with_forced_attrs_hook(|path, descriptor| {
            let mut attrs = true_attrs(path)?;
            if descriptor {
                attrs.size += 10;
            }
            Some(attrs)
        });
    let started = Instant::now();
    let err = store.read_file(&vp(NOTE)).unwrap_err();
    assert!(err.to_string().contains("stale view"), "{err}");
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "the retries outlast a one-second cache: {:?}",
        started.elapsed()
    );
}

#[test]
fn a_compare_refuses_a_name_the_server_says_has_no_link_left() {
    // A stale entry for a note the other side replaced: the server says
    // the file the name leads to has no link. Link counts are trusted on
    // this filesystem once a lock here reported one.
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    drop(folder.kernel("a.lock").acquire_write_lock().unwrap());
    let note = folder.root().join(NOTE);
    let store = Arc::new(folder.kernel("a.lock").with_forced_attrs_hook(
        move |path, descriptor| {
            let mut attrs = true_attrs(path)?;
            if !descriptor && path == note {
                attrs.nlink = 0;
            }
            Some(attrs)
        },
    ));
    let mut t = tx(&store).unwrap();
    let read = store.read_file(&vp(NOTE)).unwrap();
    t.write_file(vp(NOTE), format!("{read}more\n"));
    match t.commit() {
        Err(TransactionError::Conflict { paths }) => {
            assert_eq!(paths, vec![vp(NOTE)]);
        }
        other => panic!("expected the compare to refuse, got {other:?}"),
    }
    assert_eq!(folder.get(NOTE), "base\n");
}

#[test]
fn a_note_that_is_really_absent_is_reported_at_once() {
    // A "not found" is tried again only when the note's folder lists the
    // name, which a local folder never contradicts: a missing note costs
    // one listing and no pause.
    let folder = Folder::new();
    folder.put("inbox/other.md", "x\n");
    let store = folder.kernel("a.lock");
    let started = Instant::now();
    assert!(matches!(
        store.read_file(&vp("inbox/absent.md")),
        Err(StoreError::NotFound(_))
    ));
    assert!(
        started.elapsed() < Duration::from_millis(15),
        "{:?}",
        started.elapsed()
    );
}

// ---------------------------------------------------------------------
// Release
// ---------------------------------------------------------------------

#[test]
fn release_happens_on_commit_on_a_refused_commit_and_on_drop() {
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    let store = Arc::new(folder.kernel("a.lock"));

    // On commit.
    let mut t = tx(&store).unwrap();
    let read = store.read_file(&vp(NOTE)).unwrap();
    t.write_file(vp(NOTE), format!("{read}one\n"));
    assert_eq!(folder.lock_files(), vec![LOCK_NAME.to_owned()]);
    t.commit().unwrap();
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());

    // On a refused commit: the note changed under the transaction (a
    // writer that takes no lock, such as an editor).
    let mut t = tx(&store).unwrap();
    let read = store.read_file(&vp(NOTE)).unwrap();
    folder.put(NOTE, "edited elsewhere\n");
    t.write_file(vp(NOTE), format!("{read}two\n"));
    assert!(matches!(t.commit(), Err(TransactionError::Conflict { .. })));
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());

    // On drop, uncommitted.
    let t = tx(&store).unwrap();
    assert_eq!(folder.lock_files(), vec![LOCK_NAME.to_owned()]);
    drop(t);
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());

    // And the next writer on another kernel gets it at once.
    let other = Arc::new(
        folder
            .kernel("b.lock")
            .with_lock_timeout(Duration::from_millis(100)),
    );
    tx(&other).expect("nothing left held");
}

// ---------------------------------------------------------------------
// Cost and fairness
// ---------------------------------------------------------------------

fn median(v: &mut [Duration]) -> Duration {
    v.sort();
    v[v.len() / 2]
}

/// What the lock file adds to one write transaction on this machine's
/// disk: the median of `ROUNDS` transactions that each append a line to a
/// note, with and without it, interleaved so both see the same disk. Run
/// by hand (`--ignored --nocapture`); `docs/concurrent-writers.md` records
/// the figure.
#[test]
#[ignore = "a measurement, run by hand"]
fn measure_what_the_lock_file_adds_to_a_write_transaction() {
    const ROUNDS: usize = 300;
    let folder = Folder::new();
    folder.put(NOTE, "base\n");
    let with = Arc::new(folder.kernel("a.lock"));
    let without = Arc::new(folder.kernel("a.lock").without_shared_lock());
    let mut times = [Vec::new(), Vec::new()];
    for round in 0..ROUNDS {
        for (slot, store) in [&with, &without].into_iter().enumerate() {
            let started = Instant::now();
            let mut t = tx(store).unwrap();
            let read = store.read_file(&vp(NOTE)).unwrap();
            t.write_file(vp(NOTE), format!("{read}line {round}\n"));
            t.commit().unwrap();
            times[slot].push(started.elapsed());
        }
    }
    let (on, off) = (median(&mut times[0]), median(&mut times[1]));
    eprintln!(
        "median write transaction over {ROUNDS} rounds: {on:?} with the lock file, {off:?} \
         without, {:?} added",
        on.saturating_sub(off)
    );
    let mut alone = [Vec::new(), Vec::new()];
    for _ in 0..ROUNDS {
        for (slot, store) in [&with, &without].into_iter().enumerate() {
            let started = Instant::now();
            drop(store.acquire_write_lock().unwrap());
            alone[slot].push(started.elapsed());
        }
    }
    let (on, off) = (median(&mut alone[0]), median(&mut alone[1]));
    eprintln!("median lock and release: {on:?} with the lock file, {off:?} without");
}

/// The design review's starvation emulation (`fair.py`), against the real
/// code: one "kernel" takes the lock back to back (hold, then gap, then
/// again), another arrives at random moments and waits for it with the
/// production timeout. Prints the waiter's median, p90 and refusals per
/// row. Run by hand (`--ignored --nocapture`).
#[test]
#[ignore = "a measurement, run by hand"]
fn measure_how_long_a_waiter_waits_behind_a_back_to_back_holder() {
    const ARRIVALS: usize = 30;
    for (hold_ms, gap_ms) in [(0u64, 0u64), (1, 0), (5, 0), (12, 0), (12, 1)] {
        let folder = Folder::new();
        let churn = Arc::new(folder.kernel("h.lock"));
        let waiter = folder.kernel("c.lock");
        let stop = Arc::new(AtomicBool::new(false));
        let churner = {
            let churn = Arc::clone(&churn);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let Ok(lock) = churn.acquire_write_lock() else {
                        continue;
                    };
                    if hold_ms > 0 {
                        std::thread::sleep(Duration::from_millis(hold_ms));
                    }
                    drop(lock);
                    if gap_ms > 0 {
                        std::thread::sleep(Duration::from_millis(gap_ms));
                    }
                }
            })
        };
        std::thread::sleep(Duration::from_millis(200));
        let mut waits = Vec::new();
        let mut busy = 0;
        for n in 0..ARRIVALS {
            std::thread::sleep(Duration::from_millis((n as u64 * 7919) % 20));
            let started = Instant::now();
            match waiter.acquire_write_lock() {
                Ok(lock) => drop(lock),
                Err(_) => busy += 1,
            }
            waits.push(started.elapsed());
        }
        stop.store(true, Ordering::SeqCst);
        churner.join().unwrap();
        waits.sort();
        eprintln!(
            "hold={hold_ms}ms gap={gap_ms}ms n={ARRIVALS}: refused at 5 s={busy} median={:?} \
             p90={:?} max={:?}",
            waits[ARRIVALS / 2],
            waits[ARRIVALS * 9 / 10],
            waits[ARRIVALS - 1]
        );
    }
}
