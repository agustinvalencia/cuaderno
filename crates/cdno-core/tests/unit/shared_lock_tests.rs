//! The vault write lock every kernel sees (#1012): a lock file created
//! exclusively in `.cuaderno/`, taken after the `flock` and held for the
//! life of the transaction.
//!
//! Two stores over one folder with their own `flock` files
//! ([`FsVaultStore::with_lock_file`]) stand in for two kernels, which do not
//! see each other's `flock`: only the lock file can exclude them. A lock is
//! made stale by setting its mtime into the past, which is what its age is
//! read from (against a probe file made in the same folder).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant, SystemTime};

use cdno_core::error::{StoreError, TransactionError};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::shared_lock::{LOCK_NAME, LOCK_PATH, LOCK_PREFIX, STALE_AFTER};
use cdno_core::store::{FsVaultStore, VaultStore, VaultWriteLock};
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
        std::fs::create_dir_all(vault.path().join(".cuaderno")).unwrap();
        Self {
            vault,
            flocks: tempfile::tempdir().unwrap(),
        }
    }

    fn root(&self) -> &Path {
        self.vault.path()
    }

    /// A store that stands for a writer on a kernel of its own: its own
    /// `flock`, the shared lock file.
    fn kernel(&self, name: &str) -> FsVaultStore {
        FsVaultStore::new(self.root()).with_lock_file(self.flocks.path().join(name))
    }

    fn lock(&self) -> PathBuf {
        self.root().join(".cuaderno").join(LOCK_NAME)
    }

    fn lock_record(&self) -> Option<String> {
        std::fs::read_to_string(self.lock()).ok()
    }

    /// Every file of the lock protocol in `.cuaderno/`, the lock included.
    fn lock_files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.root().join(".cuaderno"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(LOCK_PREFIX))
            .collect();
        names.sort();
        names
    }

    /// Make the lock (or any file) look `age` old.
    fn age(&self, path: &Path, age: Duration) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    fn put(&self, path: &str, content: &str) {
        let full = self.root().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }

    fn get(&self, path: &str) -> String {
        std::fs::read_to_string(self.root().join(path)).unwrap()
    }

    /// A lock file left by a writer that crashed `age` ago.
    fn crashed_lock(&self, age: Duration) {
        std::fs::write(
            self.lock(),
            "cuaderno vault write lock\ntoken: 00\nprogram: cdno 0.0.0\nprocess: 1\n\
             taken: 2026-06-13T09:00:00+00:00\n",
        )
        .unwrap();
        self.age(&self.lock(), age);
    }
}

fn tx(store: &Arc<FsVaultStore>) -> Result<VaultTransaction, StoreError> {
    VaultTransaction::new(
        store.clone() as Arc<dyn VaultStore>,
        Arc::new(MemoryIndex::new()) as Arc<dyn VaultIndex>,
    )
}

const OLD: Duration = Duration::from_secs(STALE_AFTER.as_secs() * 4);

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
    let store = folder.kernel("a.lock");
    let lock = store.acquire_write_lock().unwrap();
    let record = folder
        .lock_record()
        .expect("the lock file exists while held");
    assert!(
        record.starts_with("cuaderno vault write lock\n"),
        "{record}"
    );
    for key in ["token: ", "program: ", "process: ", "taken: "] {
        assert!(record.contains(key), "{key:?} missing from {record}");
    }
    // Only the lock itself: the temp file its record was written in is gone.
    assert_eq!(folder.lock_files(), vec![LOCK_NAME.to_owned()]);
    assert!(!lock.broke_stale_lock());
}

#[test]
fn a_store_that_predates_the_lock_file_takes_only_the_flock() {
    let folder = Folder::new();
    let store = folder.kernel("a.lock").without_shared_lock();
    let _lock = store.acquire_write_lock().unwrap();
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());
}

// ---------------------------------------------------------------------
// Stale locks
// ---------------------------------------------------------------------

#[test]
fn a_stale_lock_is_broken_by_exactly_one_of_two_contenders() {
    let folder = Folder::new();
    folder.crashed_lock(OLD);

    // Both contenders judge the lock stale before either renames it aside.
    let barrier = Arc::new(Barrier::new(2));
    let contender = |name: &str| {
        let barrier = Arc::clone(&barrier);
        let first = AtomicBool::new(true);
        Arc::new(folder.kernel(name).with_before_break_hook(move || {
            if first.swap(false, Ordering::SeqCst) {
                barrier.wait();
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
                lock.broke_stale_lock()
            })
        })
        .collect();
    let broke: Vec<bool> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(
        broke.iter().filter(|b| **b).count(),
        1,
        "exactly one contender breaks the stale lock: {broke:?}"
    );
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
        held.verify().expect("the holder still holds its lock");
    }
    assert_eq!(folder.lock_files(), vec![LOCK_NAME.to_owned()]);
}

#[test]
fn the_busy_error_names_the_lock_its_age_and_its_owner() {
    let folder = Folder::new();
    let _held = folder.kernel("a.lock").acquire_write_lock().unwrap();
    folder.age(&folder.lock(), Duration::from_secs(12));
    let err = folder
        .kernel("b.lock")
        .with_lock_timeout(Duration::from_millis(100))
        .acquire_write_lock()
        .unwrap_err();
    let message = err.to_string();
    for part in [
        "waiting for the vault write lock",
        LOCK_PATH,
        "process ",
        &format!("(process {}", std::process::id()),
        "12s old",
        &format!("{}s old", STALE_AFTER.as_secs()),
    ] {
        assert!(message.contains(part), "{part:?} missing from: {message}");
    }
}

#[test]
fn a_crashed_holders_lock_and_leftovers_go_once_it_is_stale() {
    let folder = Folder::new();
    folder.crashed_lock(OLD);
    let leftovers = [
        format!("{LOCK_PREFIX}.abc.new"),
        format!("{LOCK_PREFIX}.def.probe"),
        format!("{LOCK_PREFIX}.0123.stale"),
    ];
    for name in &leftovers {
        let path = folder.root().join(".cuaderno").join(name);
        std::fs::write(&path, "").unwrap();
        folder.age(&path, OLD + Duration::from_secs(1));
    }
    let lock = folder
        .kernel("a.lock")
        .acquire_write_lock()
        .expect("a stale lock does not hold a writer up");
    assert!(lock.broke_stale_lock());
    assert_eq!(folder.lock_files(), vec![LOCK_NAME.to_owned()]);
    drop(lock);
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());
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
    assert!(successor.broke_stale_lock());
    let record = folder.lock_record().unwrap();

    match suspended.commit() {
        Err(TransactionError::FileWrite {
            source: StoreError::LockLost { lock },
            rollback_failures,
        }) => {
            assert_eq!(lock, LOCK_PATH);
            assert!(rollback_failures.is_empty(), "{rollback_failures:?}");
        }
        other => panic!("expected the commit refused for a lost lock, got {other:?}"),
    }
    assert_eq!(
        folder.get(NOTE),
        "base\n",
        "the broken holder wrote nothing"
    );
    // Its release left the successor's lock alone.
    assert_eq!(folder.lock_record().as_deref(), Some(record.as_str()));
    successor
        .verify()
        .expect("the successor still holds its lock");
    drop(successor);
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());
}

#[test]
fn a_lock_broken_mid_commit_stops_the_commit_and_rolls_it_back() {
    // The lock is checked before every file op, not only the first: a
    // holder broken between two of its renames undoes the first.
    let folder = Folder::new();
    folder.put("a.md", "a\n");
    folder.put("b.md", "b\n");
    let lock_path = folder.lock();
    let b = folder.kernel("b.lock");
    let successor: Arc<Mutex<Option<VaultWriteLock>>> = Arc::new(Mutex::new(None));
    let fired = AtomicBool::new(false);
    let a = {
        let successor = Arc::clone(&successor);
        Arc::new(folder.kernel("a.lock").with_before_compare_hook(move || {
            if fired.swap(true, Ordering::SeqCst) {
                return;
            }
            // Inside the first write, after its own check: the holder is
            // suspended here long enough to be broken.
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(&lock_path)
                .unwrap();
            file.set_modified(SystemTime::now() - OLD).unwrap();
            *successor.lock().unwrap() = Some(b.acquire_write_lock().unwrap());
        }))
    };

    let mut t = tx(&a).unwrap();
    let read_a = a.read_file(&vp("a.md")).unwrap();
    let read_b = a.read_file(&vp("b.md")).unwrap();
    t.write_file(vp("a.md"), format!("{read_a}more\n"));
    t.write_file(vp("b.md"), format!("{read_b}more\n"));
    match t.commit() {
        Err(TransactionError::FileWrite {
            source: StoreError::LockLost { .. },
            rollback_failures,
        }) => assert!(rollback_failures.is_empty(), "{rollback_failures:?}"),
        other => panic!("expected the commit stopped for a lost lock, got {other:?}"),
    }
    assert_eq!(folder.get("a.md"), "a\n", "the first write was rolled back");
    assert_eq!(folder.get("b.md"), "b\n");
    let successor = successor.lock().unwrap().take().expect("the successor ran");
    successor.verify().unwrap();
    drop(successor);
    assert!(folder.lock_files().is_empty(), "{:?}", folder.lock_files());
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
// Cost
// ---------------------------------------------------------------------

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
    let median = |v: &mut Vec<Duration>| {
        v.sort();
        v[v.len() / 2]
    };
    let (on, off) = (median(&mut times[0]), median(&mut times[1]));
    eprintln!(
        "median write transaction over {ROUNDS} rounds: {on:?} with the lock file, {off:?} \
         without, {:?} added",
        on.saturating_sub(off)
    );
    // Lock and release alone, without the write.
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
