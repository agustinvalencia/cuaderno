use cdno_core::error::StoreError;
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

#[test]
fn new_store_is_empty() {
    let store = MemoryVaultStore::new();
    assert!(!store.exists(&vp("anything.md")).unwrap());
}

#[test]
fn read_missing_file_returns_not_found() {
    let store = MemoryVaultStore::new();
    let err = store.read_file(&vp("missing.md")).unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)));
}

#[test]
fn write_then_read_roundtrip() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("note.md"), "hello").unwrap();
    assert_eq!(store.read_file(&vp("note.md")).unwrap(), "hello");
}

#[test]
fn read_bytes_returns_content_bytes_and_not_found() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("assets/fig.png"), "PNGDATA").unwrap();
    assert_eq!(store.read_bytes(&vp("assets/fig.png")).unwrap(), b"PNGDATA");
    let err = store.read_bytes(&vp("assets/missing.png")).unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)));
}

#[test]
fn write_overwrites_existing_content() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("note.md"), "first").unwrap();
    store.write_file(&vp("note.md"), "second").unwrap();
    assert_eq!(store.read_file(&vp("note.md")).unwrap(), "second");
}

#[test]
fn append_creates_file_when_absent() {
    let store = MemoryVaultStore::new();
    store.append_to_file(&vp("log.md"), "line one").unwrap();
    assert_eq!(store.read_file(&vp("log.md")).unwrap(), "line one");
}

#[test]
fn append_concatenates_to_existing() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("log.md"), "line one\n").unwrap();
    store.append_to_file(&vp("log.md"), "line two").unwrap();
    assert_eq!(
        store.read_file(&vp("log.md")).unwrap(),
        "line one\nline two"
    );
}

#[test]
fn move_file_relocates_content() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("a.md"), "payload").unwrap();
    store.move_file(&vp("a.md"), &vp("b.md")).unwrap();
    assert!(!store.exists(&vp("a.md")).unwrap());
    assert_eq!(store.read_file(&vp("b.md")).unwrap(), "payload");
}

#[test]
fn move_file_fails_when_source_missing() {
    let store = MemoryVaultStore::new();
    let err = store
        .move_file(&vp("ghost.md"), &vp("dest.md"))
        .unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)));
}

#[test]
fn move_file_fails_when_destination_exists() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("a.md"), "A").unwrap();
    store.write_file(&vp("b.md"), "B").unwrap();
    let err = store.move_file(&vp("a.md"), &vp("b.md")).unwrap_err();
    assert!(matches!(err, StoreError::AlreadyExists(_)));
    assert_eq!(store.read_file(&vp("a.md")).unwrap(), "A");
    assert_eq!(store.read_file(&vp("b.md")).unwrap(), "B");
}

#[test]
fn exists_is_true_for_written_file() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("x.md"), "").unwrap();
    assert!(store.exists(&vp("x.md")).unwrap());
}

#[test]
fn exists_is_true_for_implicit_directory() {
    let store = MemoryVaultStore::new();
    store
        .write_file(&vp("projects/alpha.md"), "content")
        .unwrap();
    assert!(store.exists(&vp("projects")).unwrap());
}

#[test]
fn exists_is_false_for_unknown() {
    let store = MemoryVaultStore::new();
    assert!(!store.exists(&vp("nope.md")).unwrap());
}

#[test]
fn metadata_reports_size() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("s.md"), "hello").unwrap();
    let meta = store.metadata(&vp("s.md")).unwrap();
    assert_eq!(meta.size, 5);
}

#[test]
fn metadata_missing_is_not_found() {
    let store = MemoryVaultStore::new();
    let err = store.metadata(&vp("missing.md")).unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)));
}

#[test]
fn list_dir_returns_direct_children_only() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("projects/alpha.md"), "a").unwrap();
    store.write_file(&vp("projects/beta.md"), "b").unwrap();
    store
        .write_file(&vp("projects/nested/gamma.md"), "c")
        .unwrap();

    let mut children = store.list_dir(&vp("projects")).unwrap();
    children.sort_by(|a, b| a.as_path().cmp(b.as_path()));
    assert_eq!(
        children,
        vec![
            vp("projects/alpha.md"),
            vp("projects/beta.md"),
            vp("projects/nested"),
        ]
    );
}

#[test]
fn list_dir_root_returns_top_level_entries() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("top.md"), "").unwrap();
    store.write_file(&vp("projects/inner.md"), "").unwrap();

    let mut children = store.list_dir(&VaultPath::root()).unwrap();
    children.sort_by(|a, b| a.as_path().cmp(b.as_path()));
    assert_eq!(children, vec![vp("projects"), vp("top.md")]);
}

#[test]
fn list_dir_empty_for_unknown_path() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("only.md"), "").unwrap();
    let children = store.list_dir(&vp("does/not/exist")).unwrap();
    assert!(children.is_empty());
}

#[test]
fn walk_dir_returns_all_descendant_files() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("projects/alpha.md"), "a").unwrap();
    store
        .write_file(&vp("projects/nested/gamma.md"), "c")
        .unwrap();
    store.write_file(&vp("other/unrelated.md"), "u").unwrap();

    let mut descendants = store.walk_dir(&vp("projects")).unwrap();
    descendants.sort_by(|a, b| a.as_path().cmp(b.as_path()));
    assert_eq!(
        descendants,
        vec![vp("projects/alpha.md"), vp("projects/nested/gamma.md")]
    );
}

#[test]
fn walk_dir_empty_for_unknown_path() {
    let store = MemoryVaultStore::new();
    let descendants = store.walk_dir(&vp("no/such/tree")).unwrap();
    assert!(descendants.is_empty());
}

#[test]
fn delete_file_removes_existing_file() {
    let store = MemoryVaultStore::new();
    store.write_file(&vp("note.md"), "content").unwrap();
    store.delete_file(&vp("note.md")).unwrap();
    assert!(!store.exists(&vp("note.md")).unwrap());
}

#[test]
fn delete_file_fails_when_missing() {
    let store = MemoryVaultStore::new();
    let err = store.delete_file(&vp("missing.md")).unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)));
}
