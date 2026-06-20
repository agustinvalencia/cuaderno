use cdno_core::index::SqliteIndex;
use rusqlite::Connection;
use tempfile::TempDir;

fn new_db_path() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("index.sqlite");
    (dir, path)
}

#[test]
fn open_creates_fresh_db_with_current_schema() {
    let (_dir, path) = new_db_path();
    let _index = SqliteIndex::open(&path).unwrap();

    // Open with plain rusqlite to inspect state without going through
    // SqliteIndex's API (which only exposes open + check_integrity so far).
    let conn = Connection::open(&path).unwrap();
    let version: u32 = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .unwrap();
    // 001 initial schema + 002 FTS5 search — see SCHEMA.md.
    assert_eq!(version, 2);

    // All tables exist (`notes_fts` is the FTS5 virtual table from 002).
    for table in [
        "notes",
        "deadlines",
        "links",
        "note_tags",
        "milestones",
        "archived_action_snapshots",
        "schema_migrations",
        "notes_fts",
    ] {
        let count: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "missing table: {table}");
    }
}

#[test]
fn reopen_existing_db_is_idempotent() {
    let (_dir, path) = new_db_path();
    let _first = SqliteIndex::open(&path).unwrap();
    drop(_first);
    let _second = SqliteIndex::open(&path).unwrap();

    // schema_migrations has exactly one row per shipped migration —
    // each is applied once, not re-applied on reopen.
    let conn = Connection::open(&path).unwrap();
    let row_count: u32 = conn
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(row_count, 2);
}

#[test]
fn open_enables_wal_mode() {
    let (_dir, path) = new_db_path();
    let _index = SqliteIndex::open(&path).unwrap();

    let conn = Connection::open(&path).unwrap();
    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_lowercase(), "wal");
}

#[test]
fn open_enables_foreign_keys() {
    let (_dir, path) = new_db_path();
    let index = SqliteIndex::open(&path).unwrap();

    // Cascade behaviour depends on `PRAGMA foreign_keys = ON` being set
    // on every connection (not persisted in the file). Verify by
    // inserting dependent rows and checking cascade on delete through
    // the index's own connection.
    index
        .with_connection(|c| {
            c.execute(
                "INSERT INTO notes (path, note_type, content_hash, mtime_ns, size, frontmatter, indexed_at_ns) VALUES ('p.md', 'daily', 'h', 0, 0, '{}', 0)",
                [],
            )?;
            c.execute(
                "INSERT INTO deadlines (note_path, source, title, due_date, is_hard) VALUES ('p.md', 'commitment_note', 't', '2026-01-01', 0)",
                [],
            )?;
            c.execute("DELETE FROM notes WHERE path = 'p.md'", [])?;
            Ok(())
        })
        .unwrap();

    let remaining: u32 = index
        .with_connection(|c| c.query_row("SELECT COUNT(*) FROM deadlines", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(remaining, 0, "deadlines should cascade-delete with notes");
}

#[test]
fn check_integrity_passes_on_fresh_db() {
    let (_dir, path) = new_db_path();
    let index = SqliteIndex::open(&path).unwrap();
    assert!(index.check_integrity().unwrap());
}

#[test]
fn open_creates_parent_directories() {
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("deep/nested/.cuaderno/index.sqlite");
    let _index = SqliteIndex::open(&nested).unwrap();
    assert!(nested.exists(), "index file should exist after open");
}

#[test]
fn open_self_heals_a_corrupt_index_file() {
    // A truncated / garbage file at the index path is not a valid SQLite
    // database. The index is a rebuildable cache, so `open` must discard
    // it and lay down a fresh schema rather than erroring (#206).
    let (_dir, path) = new_db_path();
    std::fs::write(&path, b"this is definitely not a sqlite database").unwrap();

    let index = SqliteIndex::open(&path).expect("open recovers from a corrupt file");
    assert!(
        index.check_integrity().unwrap(),
        "recovered index is healthy"
    );

    let conn = Connection::open(&path).unwrap();
    let version: u32 = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(version, 2, "fresh schema applied after recovery");
}

#[test]
fn reopen_preserves_a_healthy_populated_index() {
    // A healthy existing index must take the Ok branch on open, never the
    // corruption self-heal -- so its rows survive a reopen and aren't
    // clobbered (#206).
    let (_dir, path) = new_db_path();
    {
        let index = SqliteIndex::open(&path).unwrap();
        index
            .with_connection(|c| {
                c.execute(
                    "INSERT INTO notes (path, note_type, content_hash, mtime_ns, size, frontmatter, indexed_at_ns) VALUES ('p.md', 'daily', 'h', 0, 0, '{}', 0)",
                    [],
                )
            })
            .unwrap();
    }

    let index = SqliteIndex::open(&path).unwrap();
    let count: u32 = index
        .with_connection(|c| c.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(count, 1, "healthy index must not be clobbered on reopen");
}

#[test]
fn remove_deletes_the_db_and_sidecars() {
    let (_dir, path) = new_db_path();
    let _index = SqliteIndex::open(&path).unwrap();
    // Simulate WAL + SHM sidecars alongside the db.
    let sidecar = |suffix: &str| {
        let mut p = path.clone().into_os_string();
        p.push(suffix);
        std::path::PathBuf::from(p)
    };
    let wal = sidecar("-wal");
    let shm = sidecar("-shm");
    std::fs::write(&wal, b"wal").unwrap();
    std::fs::write(&shm, b"shm").unwrap();
    assert!(path.exists());

    SqliteIndex::remove(&path);
    assert!(!path.exists(), "db file removed");
    assert!(!wal.exists(), "wal sidecar removed");
    assert!(!shm.exists(), "shm sidecar removed");
}
