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
