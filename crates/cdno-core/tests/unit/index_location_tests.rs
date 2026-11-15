//! `SqliteIndex::open_location` (#998): an overridden index is bound to one
//! vault and is only discarded when it is recognisably an index; the default
//! location is opened exactly as before.

use std::path::{Path, PathBuf};

use cdno_core::error::IndexError;
use cdno_core::index::{SqliteIndex, canonical_vault_root};
use cdno_core::paths::{INDEX_DB, IndexLocation};
use tempfile::TempDir;

fn overridden(path: &Path) -> IndexLocation {
    IndexLocation {
        path: path.to_path_buf(),
        overridden: true,
    }
}

/// Two vault roots and a directory outside both for the index.
fn two_vaults() -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("surrogate-model");
    let b = dir.path().join("other-vault");
    let cache = dir.path().join("cache");
    for d in [&a, &b, &cache] {
        std::fs::create_dir_all(d).unwrap();
    }
    (dir, a, b, cache.join("index.db"))
}

/// A row that only this test writes, to see whether the index was touched.
fn write_probe(index: &SqliteIndex) {
    index
        .with_connection(|c| {
            c.execute(
                "INSERT INTO index_meta (key, value) VALUES ('probe', 'kept')",
                [],
            )
        })
        .unwrap();
}

fn has_probe(path: &Path) -> bool {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.query_row(
        "SELECT COUNT(*) FROM index_meta WHERE key = 'probe' AND value = 'kept'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

#[test]
fn an_overridden_index_binds_to_its_vault_on_first_use() {
    let (_dir, a, _b, index) = two_vaults();
    assert_eq!(SqliteIndex::bound_vault_root(&index).unwrap(), None);

    drop(SqliteIndex::open_location(&overridden(&index), &a).unwrap());
    assert_eq!(
        SqliteIndex::bound_vault_root(&index).unwrap(),
        Some(canonical_vault_root(&a))
    );
    // The same vault opens it again.
    drop(SqliteIndex::open_location(&overridden(&index), &a).unwrap());
}

#[test]
fn an_overridden_index_opened_for_another_vault_is_refused_and_left_untouched() {
    let (_dir, a, b, index) = two_vaults();
    let opened = SqliteIndex::open_location(&overridden(&index), &a).unwrap();
    write_probe(&opened);
    drop(opened);
    let before = std::fs::read(&index).unwrap();

    let err = SqliteIndex::open_location(&overridden(&index), &b).expect_err("other vault");
    let IndexError::BoundToOtherVault {
        bound,
        root,
        index: named,
    } = &err
    else {
        panic!("expected BoundToOtherVault, got {err:?}");
    };
    assert_eq!(bound, &canonical_vault_root(&a));
    assert_eq!(root, &canonical_vault_root(&b));
    assert_eq!(named, &index.display().to_string());
    let msg = err.to_string();
    assert!(msg.contains("cdno reindex"), "{msg}");

    assert_eq!(std::fs::read(&index).unwrap(), before, "bytes unchanged");
    assert!(has_probe(&index), "the first vault's rows are still there");
    assert_eq!(
        SqliteIndex::bound_vault_root(&index).unwrap(),
        Some(canonical_vault_root(&a)),
        "still bound to the first vault"
    );
}

#[cfg(unix)]
#[test]
fn the_same_vault_through_a_symlink_is_the_same_vault() {
    let (dir, a, _b, index) = two_vaults();
    let link = dir.path().join("link-to-vault");
    std::os::unix::fs::symlink(&a, &link).unwrap();

    drop(SqliteIndex::open_location(&overridden(&index), &a).unwrap());
    drop(SqliteIndex::open_location(&overridden(&index), &link).unwrap());
}

#[test]
fn an_existing_unbound_index_is_bound_on_first_overridden_open() {
    // An index built at a default location (or before migration 005)
    // records no vault; the first overridden open binds it.
    let (_dir, a, b, index) = two_vaults();
    drop(SqliteIndex::open(&index).unwrap());
    assert_eq!(SqliteIndex::bound_vault_root(&index).unwrap(), None);

    drop(SqliteIndex::open_location(&overridden(&index), &a).unwrap());
    assert!(matches!(
        SqliteIndex::open_location(&overridden(&index), &b),
        Err(IndexError::BoundToOtherVault { .. })
    ));
}

#[cfg(unix)]
#[test]
fn the_default_location_is_never_bound_or_guarded() {
    // At the default location the vault may legitimately be reached under
    // two paths; neither records a binding nor is refused.
    let (dir, a, _b, _index) = two_vaults();
    let link = dir.path().join("link-to-vault");
    std::os::unix::fs::symlink(&a, &link).unwrap();
    let default = IndexLocation {
        path: a.join(INDEX_DB),
        overridden: false,
    };

    drop(SqliteIndex::open_location(&default, &a).unwrap());
    drop(SqliteIndex::open_location(&default, &link).unwrap());
    // Not even a different root is refused there: the guard is the override's.
    drop(SqliteIndex::open_location(&default, dir.path()).unwrap());
    assert_eq!(SqliteIndex::bound_vault_root(&default.path).unwrap(), None);
}

#[test]
fn a_file_that_is_not_an_index_is_refused_and_left_byte_identical() {
    let (_dir, a, _b, index) = two_vaults();
    let note = b"---\ntype: project\n---\n# Surrogate model\n";
    std::fs::write(&index, note).unwrap();

    let err = SqliteIndex::open_location(&overridden(&index), &a).expect_err("not an index");
    assert!(matches!(err, IndexError::NotAnIndex { .. }), "{err:?}");
    assert!(err.to_string().contains("nothing was deleted"), "{err}");
    assert_eq!(std::fs::read(&index).unwrap(), note);
    assert!(matches!(
        SqliteIndex::ensure_discardable(&index),
        Err(IndexError::NotAnIndex { .. })
    ));
}

#[test]
fn a_corrupt_sqlite_file_at_an_override_still_heals() {
    let (_dir, a, _b, index) = two_vaults();
    let mut corrupt = b"SQLite format 3\0".to_vec();
    corrupt.extend(std::iter::repeat_n(0xFFu8, 2048));
    std::fs::write(&index, &corrupt).unwrap();

    let healed = SqliteIndex::open_location(&overridden(&index), &a).expect("self-heals");
    assert!(healed.check_integrity().unwrap());
    assert_eq!(
        SqliteIndex::bound_vault_root(&index).unwrap(),
        Some(canonical_vault_root(&a))
    );
}

#[test]
fn missing_empty_and_sqlite_files_are_discardable() {
    let (_dir, a, _b, index) = two_vaults();
    SqliteIndex::ensure_discardable(&index).expect("missing");
    std::fs::write(&index, b"").unwrap();
    SqliteIndex::ensure_discardable(&index).expect("empty");
    drop(SqliteIndex::open_location(&overridden(&index), &a).unwrap());
    SqliteIndex::ensure_discardable(&index).expect("a real index");
    // A file shorter than the header is not empty and not an index.
    std::fs::write(&index, b"SQLite").unwrap();
    assert!(SqliteIndex::ensure_discardable(&index).is_err());
}
