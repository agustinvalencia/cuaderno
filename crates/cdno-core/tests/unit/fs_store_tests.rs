use cdno_core::error::StoreError;
use cdno_core::path::VaultPath;
use cdno_core::store::{FsVaultStore, VaultStore};
use std::fs;
use tempfile::TempDir;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn store() -> (TempDir, FsVaultStore) {
    let dir = TempDir::new().unwrap();
    let store = FsVaultStore::new(dir.path());
    (dir, store)
}

#[test]
fn new_store_is_empty() {
    let (_dir, store) = store();
    assert!(!store.exists(&vp("anything.md")).unwrap());
}

#[test]
fn read_missing_file_returns_not_found() {
    let (_dir, store) = store();
    let err = store.read_file(&vp("missing.md")).unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)));
}

#[test]
fn write_then_read_roundtrip() {
    let (_dir, store) = store();
    store.write_file(&vp("note.md"), "hello").unwrap();
    assert_eq!(store.read_file(&vp("note.md")).unwrap(), "hello");
}

#[test]
fn write_creates_parent_directories() {
    let (dir, store) = store();
    store
        .write_file(&vp("journal/daily/2026-04-18.md"), "entry")
        .unwrap();
    assert!(dir.path().join("journal/daily/2026-04-18.md").exists());
}

#[test]
fn write_overwrites_existing_content() {
    let (_dir, store) = store();
    store.write_file(&vp("note.md"), "first").unwrap();
    store.write_file(&vp("note.md"), "second").unwrap();
    assert_eq!(store.read_file(&vp("note.md")).unwrap(), "second");
}

#[test]
fn append_creates_file_when_absent() {
    let (_dir, store) = store();
    store.append_to_file(&vp("log.md"), "line one").unwrap();
    assert_eq!(store.read_file(&vp("log.md")).unwrap(), "line one");
}

#[test]
fn append_creates_parent_directories() {
    let (dir, store) = store();
    store
        .append_to_file(&vp("stewardships/health/tracking/2026-04-18.md"), "entry")
        .unwrap();
    assert!(
        dir.path()
            .join("stewardships/health/tracking/2026-04-18.md")
            .exists()
    );
}

#[test]
fn append_concatenates_to_existing() {
    let (_dir, store) = store();
    store.write_file(&vp("log.md"), "line one\n").unwrap();
    store.append_to_file(&vp("log.md"), "line two").unwrap();
    assert_eq!(
        store.read_file(&vp("log.md")).unwrap(),
        "line one\nline two"
    );
}

#[test]
fn move_file_relocates_content() {
    let (_dir, store) = store();
    store.write_file(&vp("a.md"), "payload").unwrap();
    store.move_file(&vp("a.md"), &vp("b.md")).unwrap();
    assert!(!store.exists(&vp("a.md")).unwrap());
    assert_eq!(store.read_file(&vp("b.md")).unwrap(), "payload");
}

#[test]
fn move_file_creates_destination_parents() {
    let (dir, store) = store();
    store.write_file(&vp("inbox/draft.md"), "payload").unwrap();
    store
        .move_file(&vp("inbox/draft.md"), &vp("projects/new/draft.md"))
        .unwrap();
    assert!(dir.path().join("projects/new/draft.md").exists());
}

#[test]
fn move_file_fails_when_source_missing() {
    let (_dir, store) = store();
    let err = store
        .move_file(&vp("ghost.md"), &vp("dest.md"))
        .unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)));
}

#[test]
fn move_file_fails_when_destination_exists() {
    let (_dir, store) = store();
    store.write_file(&vp("a.md"), "A").unwrap();
    store.write_file(&vp("b.md"), "B").unwrap();
    let err = store.move_file(&vp("a.md"), &vp("b.md")).unwrap_err();
    assert!(matches!(err, StoreError::AlreadyExists(_)));
    assert_eq!(store.read_file(&vp("a.md")).unwrap(), "A");
    assert_eq!(store.read_file(&vp("b.md")).unwrap(), "B");
}

#[test]
fn exists_is_true_for_written_file() {
    let (_dir, store) = store();
    store.write_file(&vp("x.md"), "").unwrap();
    assert!(store.exists(&vp("x.md")).unwrap());
}

#[test]
fn exists_is_true_for_directory() {
    let (_dir, store) = store();
    store
        .write_file(&vp("projects/alpha.md"), "content")
        .unwrap();
    assert!(store.exists(&vp("projects")).unwrap());
}

#[test]
fn exists_is_false_for_unknown() {
    let (_dir, store) = store();
    assert!(!store.exists(&vp("nope.md")).unwrap());
}

#[test]
fn metadata_reports_size() {
    let (_dir, store) = store();
    store.write_file(&vp("s.md"), "hello").unwrap();
    let meta = store.metadata(&vp("s.md")).unwrap();
    assert_eq!(meta.size, 5);
}

#[test]
fn metadata_missing_is_not_found() {
    let (_dir, store) = store();
    let err = store.metadata(&vp("missing.md")).unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)));
}

#[test]
fn list_dir_returns_direct_children_only() {
    let (_dir, store) = store();
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
    let (_dir, store) = store();
    store.write_file(&vp("top.md"), "").unwrap();
    store.write_file(&vp("projects/inner.md"), "").unwrap();

    let mut children = store.list_dir(&VaultPath::root()).unwrap();
    children.sort_by(|a, b| a.as_path().cmp(b.as_path()));
    assert_eq!(children, vec![vp("projects"), vp("top.md")]);
}

#[test]
fn list_dir_empty_for_unknown_path() {
    let (_dir, store) = store();
    store.write_file(&vp("only.md"), "").unwrap();
    let children = store.list_dir(&vp("does/not/exist")).unwrap();
    assert!(children.is_empty());
}

#[test]
fn walk_dir_returns_all_descendant_files() {
    let (_dir, store) = store();
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
    let (_dir, store) = store();
    let descendants = store.walk_dir(&vp("no/such/tree")).unwrap();
    assert!(descendants.is_empty());
}

#[test]
fn read_file_on_non_utf8_returns_io_error() {
    let (dir, store) = store();
    // Bypass the store to plant an invalid-UTF-8 file on disk.
    fs::write(dir.path().join("bad.md"), [0xFF, 0xFE, 0xFD]).unwrap();
    let err = store.read_file(&vp("bad.md")).unwrap_err();
    assert!(matches!(err, StoreError::Io { .. }));
}

#[test]
fn delete_file_removes_existing_file() {
    let (dir, store) = store();
    store.write_file(&vp("note.md"), "content").unwrap();
    store.delete_file(&vp("note.md")).unwrap();
    assert!(!dir.path().join("note.md").exists());
}

#[test]
fn delete_file_fails_when_missing() {
    let (_dir, store) = store();
    let err = store.delete_file(&vp("missing.md")).unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)));
}

// ---- import_external (#154 attachments) ---------------------------------

#[test]
fn import_external_copies_bytes_for_byte_including_non_utf8() {
    let (dir, store) = store();
    let src = dir.path().join("source.bin");
    let bytes: &[u8] = &[0x89, b'P', b'N', b'G', 0x00, 0xFF, 0xFE];
    fs::write(&src, bytes).unwrap();

    store
        .import_external(&src, &vp("portfolios/p/2026-06-13-e/fig.png"))
        .unwrap();

    let copied = fs::read(dir.path().join("portfolios/p/2026-06-13-e/fig.png")).unwrap();
    assert_eq!(copied, bytes, "artefact copied byte-for-byte");
    assert!(src.exists(), "import is a copy, not a move");
}

#[test]
fn import_external_missing_source_names_the_source() {
    let (_dir, store) = store();
    let err = store
        .import_external(
            std::path::Path::new("/no/such/source-xyz.pdf"),
            &vp("portfolios/p/e/x.pdf"),
        )
        .unwrap_err();
    let StoreError::NotFound(msg) = err else {
        panic!("expected NotFound, got {err:?}");
    };
    assert!(msg.contains("attachment source"), "names the source: {msg}");
    assert!(msg.contains("source-xyz.pdf"), "{msg}");
}

#[test]
fn import_external_refuses_to_overwrite_an_existing_dest() {
    let (dir, store) = store();
    let src = dir.path().join("s.bin");
    fs::write(&src, b"first").unwrap();
    store.import_external(&src, &vp("a/b.pdf")).unwrap();

    // Create-only: a second import to the same dest must refuse, so the
    // transaction's import rollback can't delete a pre-existing file.
    fs::write(&src, b"second").unwrap();
    let err = store.import_external(&src, &vp("a/b.pdf")).unwrap_err();
    assert!(matches!(err, StoreError::AlreadyExists(_)));
    assert_eq!(
        fs::read(dir.path().join("a/b.pdf")).unwrap(),
        b"first",
        "the original copy is untouched"
    );
}
