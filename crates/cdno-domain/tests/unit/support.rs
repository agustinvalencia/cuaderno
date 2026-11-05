//! Shared test-only doubles for `cdno-domain`'s unit suite.
//!
//! Lifted out of `actions_tests.rs` and `custom_notes_tests.rs`, which
//! each carried an identical copy of [`FailingStore`] to exercise the
//! transaction-rollback path (T4 review).

use std::sync::{Arc, Mutex};

use cdno_core::error::StoreError;
use cdno_core::file_meta::FileMeta;
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};

/// Wraps a `MemoryVaultStore`, failing the Nth write/append/move/delete
/// so the transaction rollback path can be exercised at the domain
/// level. Reads, `exists`, and directory walks never fail or count, so
/// `Vault::new` reconciliation runs cleanly before the counter matters.
pub struct FailingStore {
    inner: Arc<MemoryVaultStore>,
    fail_on: usize,
    count: Mutex<usize>,
}

impl FailingStore {
    pub fn new(inner: Arc<MemoryVaultStore>, fail_on: usize) -> Self {
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

/// Wraps a `MemoryVaultStore` and, once armed, writes `occupant` to
/// `target` the first time `trigger` is read: a file appearing behind the
/// write lock's back (an editor that ignores it) between a verb's locate
/// and its destination check. Every call delegates otherwise.
pub struct RacingStore {
    inner: Arc<MemoryVaultStore>,
    trigger: VaultPath,
    target: VaultPath,
    occupant: String,
    armed: Mutex<bool>,
}

impl RacingStore {
    pub fn new(
        inner: Arc<MemoryVaultStore>,
        trigger: VaultPath,
        target: VaultPath,
        occupant: &str,
    ) -> Self {
        Self {
            inner,
            trigger,
            target,
            occupant: occupant.to_owned(),
            armed: Mutex::new(false),
        }
    }

    /// Fire on the next read of `trigger` (and only that one).
    pub fn arm(&self) {
        *self.armed.lock().unwrap() = true;
    }
}

impl VaultStore for RacingStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        let read = self.inner.read_file(path);
        let mut armed = self.armed.lock().unwrap();
        if *armed && *path == self.trigger {
            *armed = false;
            self.inner.write_file(&self.target, &self.occupant)?;
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
    fn import_external(&self, src: &std::path::Path, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.import_external(src, dest)
    }
}

/// The `today` the lint calls of the suite pass: a day before every date the
/// fixtures write, so no date reads as overdue and the parked read-through
/// rule (RFC 0006 §8.1, plan T12) stays out of tests about the other rules.
/// The read-through's own tests pass the day they mean.
pub fn lint_day() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()
}

// ---------------------------------------------------------------------
// By old name (RFC 0007 §5.5, T12)
// ---------------------------------------------------------------------
//
// The `*_by_old_name_logs_the_new_slug` tables: a note renamed `old` →
// `new` carries `aliases: [old]`, a verb is called with `"old"`, and the
// line it writes to the daily log must name `new` and never `old`.

/// When every by-old-name row writes, and so whose daily note it reads.
pub fn by_old_name_at() -> chrono::NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(2026, 10, 20)
        .unwrap()
        .and_hms_opt(10, 0, 0)
        .unwrap()
}

/// The daily note [`by_old_name_at`] writes to.
pub const BY_OLD_NAME_DAILY: &str = "journal/2026/daily/2026-10-20.md";

/// A vault seeded with `notes`, and the store it reads, so a row can read
/// back what its verb wrote.
pub fn seeded_vault(notes: &[(&str, String)]) -> (cdno_domain::Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn cdno_core::index::VaultIndex> =
        Arc::new(cdno_core::index::MemoryIndex::new());
    for (path, body) in notes {
        store
            .write_file(&VaultPath::new(*path).unwrap(), body)
            .unwrap();
    }
    let (vault, _report) = cdno_domain::Vault::new(
        Arc::clone(&store),
        index,
        cdno_core::config::VaultConfig::default(),
    )
    .expect("Vault::new");
    (vault, store)
}

/// Assert the row's daily note holds `expected` and names `old` in no
/// wikilink: neither `[[old` nor `/old]]` nor `/old#`.
pub fn assert_logs_the_new_slug(store: &Arc<dyn VaultStore>, row: &str, expected: &str) {
    let daily = store
        .read_file(&VaultPath::new(BY_OLD_NAME_DAILY).unwrap())
        .unwrap_or_else(|e| panic!("{row}: no daily note: {e}"));
    assert!(
        daily.contains(expected),
        "{row}: the log lacks `{expected}`:\n{daily}"
    );
    for old in ["[[old", "/old]]", "/old#"] {
        assert!(
            !daily.contains(old),
            "{row}: the log names the old slug (`{old}`):\n{daily}"
        );
    }
}
