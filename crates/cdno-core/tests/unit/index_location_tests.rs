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

/// Another application's SQLite database, in rollback-journal mode as most
/// are: a valid header, no cuaderno mark.
fn write_foreign_database(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE bookmarks (url TEXT PRIMARY KEY, title TEXT);
         INSERT INTO bookmarks VALUES ('https://example.org', 'Example');",
    )
    .unwrap();
}

fn sidecars_of(path: &Path) -> Vec<PathBuf> {
    ["-wal", "-shm", "-journal"]
        .iter()
        .map(|s| {
            let mut name = path.as_os_str().to_owned();
            name.push(s);
            PathBuf::from(name)
        })
        .filter(|p| p.exists())
        .collect()
}

#[test]
fn another_applications_sqlite_database_is_refused_and_left_byte_identical() {
    let (_dir, a, _b, index) = two_vaults();
    write_foreign_database(&index);
    let before = std::fs::read(&index).unwrap();

    let err = SqliteIndex::open_location(&overridden(&index), &a).expect_err("foreign");
    assert!(matches!(err, IndexError::NotAnIndex { .. }), "{err:?}");
    assert!(matches!(
        SqliteIndex::ensure_discardable(&index),
        Err(IndexError::NotAnIndex { .. })
    ));
    assert_eq!(std::fs::read(&index).unwrap(), before, "bytes unchanged");
    assert_eq!(
        sidecars_of(&index),
        Vec::<PathBuf>::new(),
        "no sidecar created"
    );
}

#[test]
fn an_index_from_before_migration_005_is_accepted_and_bound() {
    // Recognised by migration 1's own `schema_migrations` row.
    let (_dir, a, b, index) = two_vaults();
    drop(SqliteIndex::open(&index).unwrap());
    {
        let conn = rusqlite::Connection::open(&index).unwrap();
        conn.execute_batch(
            "DROP TABLE index_meta; DELETE FROM schema_migrations WHERE version = 5;",
        )
        .unwrap();
    }
    SqliteIndex::ensure_discardable(&index).expect("a pre-005 cuaderno index");

    drop(SqliteIndex::open_location(&overridden(&index), &a).unwrap());
    assert_eq!(
        SqliteIndex::bound_vault_root(&index).unwrap(),
        Some(canonical_vault_root(&a))
    );
    assert!(matches!(
        SqliteIndex::open_location(&overridden(&index), &b),
        Err(IndexError::BoundToOtherVault { .. })
    ));
}

#[test]
fn of_two_connections_first_binding_one_file_the_second_is_refused() {
    // Both opened before either binds: the race the check-then-bind
    // transaction closes.
    let (_dir, a, b, index) = two_vaults();
    let first = SqliteIndex::open(&index).unwrap();
    let second = SqliteIndex::open(&index).unwrap();
    let (root_a, root_b) = (canonical_vault_root(&a), canonical_vault_root(&b));

    first.bind_vault_root(&index, &root_a).expect("first binds");
    let err = second
        .bind_vault_root(&index, &root_b)
        .expect_err("second must not overwrite");
    assert!(
        matches!(&err, IndexError::BoundToOtherVault { bound, .. } if *bound == root_a),
        "{err:?}"
    );
    // Binding the same root again is a no-op, not an error.
    second.bind_vault_root(&index, &root_a).unwrap();
    assert_eq!(SqliteIndex::bound_vault_root(&index).unwrap(), Some(root_a));
}

#[test]
fn opening_an_index_already_bound_to_this_vault_writes_nothing_for_the_binding() {
    let (_dir, a, _b, index) = two_vaults();
    drop(SqliteIndex::open_location(&overridden(&index), &a).unwrap());

    // Another connection holds the write lock for the whole reopen. Any
    // write by the reopen, even a binding `INSERT OR IGNORE` that changes
    // nothing, needs that lock and would fail with SQLITE_BUSY after the
    // busy timeout; a reopen that only reads goes through.
    let holder = rusqlite::Connection::open(&index).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let reopened = SqliteIndex::open_location(&overridden(&index), &a);
    holder.execute_batch("COMMIT;").unwrap();
    drop(reopened.expect("a reopen for the bound vault must not write"));
}
