//! Compile-time checks for the `VaultStore` trait shape.
//!
//! There is no behaviour to exercise yet — concrete implementations
//! live in `FsVaultStore` (#6) and `MemoryVaultStore` (#7). These
//! tests pin the trait's dyn-compatibility and Send + Sync bounds so
//! an accidental addition of a generic method or a non-thread-safe
//! field in a future impl trips CI instead of a downstream caller.

use cdno_core::error::StoreError;
use cdno_core::file_meta::FileMeta;
use cdno_core::path::VaultPath;
use cdno_core::store::VaultStore;

struct StubStore;

impl VaultStore for StubStore {
    fn read_file(&self, _path: &VaultPath) -> Result<String, StoreError> {
        unimplemented!()
    }

    fn read_bytes(&self, _path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        unimplemented!()
    }

    fn write_file(&self, _path: &VaultPath, _content: &str) -> Result<(), StoreError> {
        unimplemented!()
    }

    fn append_to_file(&self, _path: &VaultPath, _content: &str) -> Result<(), StoreError> {
        unimplemented!()
    }

    fn move_file(&self, _src: &VaultPath, _dest: &VaultPath) -> Result<(), StoreError> {
        unimplemented!()
    }

    fn delete_file(&self, _path: &VaultPath) -> Result<(), StoreError> {
        unimplemented!()
    }

    fn exists(&self, _path: &VaultPath) -> Result<bool, StoreError> {
        unimplemented!()
    }

    fn list_dir(&self, _path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        unimplemented!()
    }

    fn walk_dir(&self, _path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        unimplemented!()
    }

    fn metadata(&self, _path: &VaultPath) -> Result<FileMeta, StoreError> {
        unimplemented!()
    }

    fn import_external(&self, _src: &std::path::Path, _dest: &VaultPath) -> Result<(), StoreError> {
        unimplemented!()
    }
}

fn assert_send_sync<T: Send + Sync>() {}
fn assert_dyn_compatible(_: &dyn VaultStore) {}

#[test]
fn trait_is_send_sync() {
    assert_send_sync::<StubStore>();
    assert_send_sync::<Box<dyn VaultStore>>();
}

#[test]
fn trait_is_dyn_compatible() {
    let stub: Box<dyn VaultStore> = Box::new(StubStore);
    assert_dyn_compatible(&*stub);
}
