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
    // 001 initial schema + 002 FTS5 search + 003 links.via_alias + 004 snapshot masked hash — see
    // SCHEMA.md.
    assert_eq!(version, 4);

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
    assert_eq!(row_count, 4);
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
    assert_eq!(version, 4, "fresh schema applied after recovery");
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

#[test]
fn migration_003_adds_via_alias() {
    // A database left at schema version 2 (001 + 002, as every vault opened
    // before RFC 0007 S3 has it), holding one link row. Opening it applies
    // 003: the column appears, and the existing row reads 0 — it resolved
    // without aliases, which is what the default records.
    let (_dir, path) = new_db_path();
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial.sql"))
            .unwrap();
        conn.execute_batch(include_str!("../../migrations/002_fts5_search.sql"))
            .unwrap();
        conn.execute(
            "INSERT INTO notes (path, note_type, content_hash, mtime_ns, size, frontmatter, indexed_at_ns) \
             VALUES ('journal/d.md', 'daily', 'h', 0, 0, '{}', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO links (source_path, target_raw, resolved_path, label) \
             VALUES ('journal/d.md', 'surrogate-model', NULL, NULL)",
            [],
        )
        .unwrap();
    }

    let index = SqliteIndex::open(&path).unwrap();
    let (version, has_column, via_alias): (u32, bool, i64) = index
        .with_connection(|c| {
            let version = c.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })?;
            let has_column = c.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('links') WHERE name = 'via_alias'",
                [],
                |r| r.get::<_, i64>(0),
            )? == 1;
            let via_alias = c.query_row("SELECT via_alias FROM links", [], |r| r.get(0))?;
            Ok((version, has_column, via_alias))
        })
        .unwrap();
    assert_eq!(version, 4);
    assert!(has_column, "links.via_alias added by 003");
    assert_eq!(
        via_alias, 0,
        "an existing row did not resolve through an alias"
    );
}

#[test]
fn migration_004_adds_nullable_columns() {
    // A database at schema version 3, holding one archived snapshot. Opening
    // it applies 004: both columns appear and the existing row reads NULL --
    // it has no masked hash until a backfill stamps one.
    let (_dir, path) = new_db_path();
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial.sql"))
            .unwrap();
        conn.execute_batch(include_str!("../../migrations/002_fts5_search.sql"))
            .unwrap();
        conn.execute_batch(include_str!("../../migrations/003_links_via_alias.sql"))
            .unwrap();
        conn.execute(
            "INSERT INTO notes (path, note_type, content_hash, mtime_ns, size, frontmatter, indexed_at_ns) \
             VALUES ('actions/_done/2026/surrogate-model.md', 'action', 'h', 0, 0, '{}', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO archived_action_snapshots (note_path, frozen_size, frozen_hash, archived_at_ns) \
             VALUES ('actions/_done/2026/surrogate-model.md', 10, 'h', 0)",
            [],
        )
        .unwrap();
    }

    let index = SqliteIndex::open(&path).unwrap();
    let (version, columns, masked, mask_version, masked_size): (
        u32,
        i64,
        Option<String>,
        Option<u32>,
        Option<i64>,
    ) = index
        .with_connection(|c| {
            let version = c.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })?;
            let columns = c.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('archived_action_snapshots') \
                 WHERE name IN ('masked_hash', 'mask_version', 'masked_size')",
                [],
                |r| r.get::<_, i64>(0),
            )?;
            let (masked, mask_version, masked_size) = c.query_row(
                "SELECT masked_hash, mask_version, masked_size FROM archived_action_snapshots",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            Ok((version, columns, masked, mask_version, masked_size))
        })
        .unwrap();
    assert_eq!(version, 4);
    assert_eq!(columns, 3, "all three columns added by 004");
    assert_eq!(masked_size, None);
    assert_eq!(masked, None);
    assert_eq!(mask_version, None);
}

// ---------------------------------------------------------------------
// Integrity checks and the in-place rebuild (#526)
// ---------------------------------------------------------------------

fn insert_note(index: &SqliteIndex, path: &str, frontmatter: &str) {
    index
        .with_connection(|c| {
            c.execute(
                "INSERT INTO notes (path, note_type, content_hash, mtime_ns, size, frontmatter, indexed_at_ns) VALUES (?1, 'daily', 'h', 0, 0, ?2, 0)",
                rusqlite::params![path, frontmatter],
            )
        })
        .unwrap();
}

fn note_paths(index: &SqliteIndex) -> Vec<String> {
    index
        .with_connection(|c| {
            let mut stmt = c.prepare("SELECT path FROM notes ORDER BY path")?;
            stmt.query_map([], |r| r.get(0))?.collect()
        })
        .unwrap()
}

fn with_suffix(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(suffix);
    std::path::PathBuf::from(p)
}

/// Fill an index with enough rows to span many pages, close it, and
/// overwrite its last page with garbage. The header and schema pages are
/// untouched, so the file still opens and migrates: the damage only
/// shows to an integrity check, the shape that went unnoticed in #526.
fn damaged_index_file(path: &std::path::Path) {
    {
        let index = SqliteIndex::open(path).unwrap();
        let padding = "x".repeat(400);
        for i in 0..400 {
            insert_note(
                &index,
                &format!("n{i:04}.md"),
                &format!("{{\"p\":\"{padding}\"}}"),
            );
        }
    }
    let page_size: u64 = Connection::open(path)
        .unwrap()
        .query_row("PRAGMA page_size", [], |r| r.get(0))
        .unwrap();
    let mut bytes = std::fs::read(path).unwrap();
    let len = bytes.len() as u64;
    assert!(len / page_size > 20, "the fixture spans many pages");
    let start = (len - page_size) as usize;
    for b in &mut bytes[start..] {
        *b = 0xA5;
    }
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn quick_check_is_empty_on_a_sound_index() {
    let (_dir, path) = new_db_path();
    let index = SqliteIndex::open(&path).unwrap();
    insert_note(&index, "a.md", "{}");
    assert!(index.quick_check().unwrap().is_empty());
    assert!(index.integrity_check().unwrap().is_empty());
}

#[test]
fn quick_check_reports_page_damage_that_open_does_not_notice() {
    let (_dir, path) = new_db_path();
    damaged_index_file(&path);

    let index = SqliteIndex::open(&path).expect("page damage does not fail open");
    let problems = index.quick_check().unwrap();
    assert!(!problems.is_empty(), "the damage is reported");
    assert!(problems.len() <= cdno_core::index::CHECK_PROBLEM_LIMIT);
    assert!(!index.check_integrity().unwrap());
}

#[test]
fn rebuild_with_swaps_the_fresh_file_in_and_keeps_the_old_one() {
    let (_dir, path) = new_db_path();
    let index = SqliteIndex::open(&path).unwrap();
    insert_note(&index, "old.md", "{}");

    index
        .rebuild_with(|fresh| {
            fresh.list_all_paths().map(|paths| assert!(paths.is_empty(), "it starts empty"))?;
            // `populate` writes through the trait in real use; raw SQL
            // through a second handle stands in for it here.
            let conn = Connection::open(with_suffix(&path, cdno_core::index::REBUILD_SUFFIX))
                .unwrap();
            conn.execute(
                "INSERT INTO notes (path, note_type, content_hash, mtime_ns, size, frontmatter, indexed_at_ns) VALUES ('new.md', 'daily', 'h', 0, 0, '{}', 0)",
                [],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();

    assert_eq!(
        note_paths(&index),
        vec!["new.md"],
        "the same handle reads the new file"
    );
    assert!(index.quick_check().unwrap().is_empty());
    let previous = with_suffix(&path, cdno_core::index::PREVIOUS_SUFFIX);
    assert!(previous.exists(), "the replaced file is kept");
    let old: Vec<String> = Connection::open(&previous)
        .unwrap()
        .prepare("SELECT path FROM notes")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(old, vec!["old.md"]);
    assert!(
        !with_suffix(&path, cdno_core::index::REBUILD_SUFFIX).exists(),
        "no rebuild file is left behind"
    );
}

#[test]
fn rebuild_with_leaves_the_live_index_alone_when_populate_fails() {
    let (_dir, path) = new_db_path();
    let index = SqliteIndex::open(&path).unwrap();
    insert_note(&index, "kept.md", "{}");

    let err = index
        .rebuild_with(|_| -> Result<(), _> {
            Err(cdno_core::error::IndexError::Query("walk failed".into()))
        })
        .unwrap_err();
    assert!(err.to_string().contains("walk failed"));
    assert_eq!(note_paths(&index), vec!["kept.md"]);
    assert!(!with_suffix(&path, cdno_core::index::REBUILD_SUFFIX).exists());
    assert!(!with_suffix(&path, cdno_core::index::PREVIOUS_SUFFIX).exists());
}

#[test]
fn rebuild_with_refuses_while_the_fresh_index_is_still_held() {
    let (_dir, path) = new_db_path();
    let index = SqliteIndex::open(&path).unwrap();
    insert_note(&index, "kept.md", "{}");

    let mut kept = None;
    let err = index
        .rebuild_with(|fresh| {
            kept = Some(fresh.clone());
            Ok(())
        })
        .unwrap_err();
    assert!(err.to_string().contains("still in use"), "{err}");
    drop(kept);
    assert_eq!(
        note_paths(&index),
        vec!["kept.md"],
        "the live index is untouched"
    );
}

#[test]
fn rebuild_with_repairs_a_damaged_index_behind_the_same_handle() {
    let (_dir, path) = new_db_path();
    damaged_index_file(&path);
    let index = SqliteIndex::open(&path).unwrap();
    assert!(!index.quick_check().unwrap().is_empty());

    index.rebuild_with(|_| Ok(())).unwrap();

    assert!(
        index.quick_check().unwrap().is_empty(),
        "the handle now reads a sound file"
    );
    assert!(note_paths(&index).is_empty());
    insert_note(&index, "after.md", "{}");
    assert_eq!(note_paths(&index), vec!["after.md"], "and writes to it");
}

#[test]
fn set_aside_moves_the_index_files_to_previous() {
    let (_dir, path) = new_db_path();
    drop(SqliteIndex::open(&path).unwrap());
    std::fs::write(with_suffix(&path, "-wal"), b"wal").unwrap();

    SqliteIndex::set_aside(&path);

    assert!(!path.exists());
    assert!(!with_suffix(&path, "-wal").exists());
    assert!(with_suffix(&path, cdno_core::index::PREVIOUS_SUFFIX).exists());
    assert!(with_suffix(&path, &format!("{}-wal", cdno_core::index::PREVIOUS_SUFFIX)).exists());
}

fn sqlite_failure(code: std::ffi::c_int) -> cdno_core::error::IndexError {
    cdno_core::error::IndexError::Sqlite(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(code),
        None,
    ))
}

#[test]
fn index_stale_names_its_first_cause() {
    use cdno_core::error::{IndexError, TransactionError};
    let err = TransactionError::IndexStale(vec![
        sqlite_failure(rusqlite::ffi::SQLITE_CORRUPT),
        IndexError::Query("second".into()),
    ]);
    let message = err.to_string();
    assert!(
        message.starts_with("index stale after successful commit (2 error(s)): SQLite error: "),
        "{message}"
    );
    assert!(!message.contains("second"), "only the first cause is named");
    assert_eq!(
        TransactionError::IndexStale(Vec::new()).to_string(),
        "index stale after successful commit (0 error(s))"
    );
}

#[test]
fn only_a_damaged_file_counts_as_index_corruption() {
    use cdno_core::error::{IndexError, TransactionError};
    assert!(sqlite_failure(rusqlite::ffi::SQLITE_CORRUPT).is_corruption());
    assert!(sqlite_failure(rusqlite::ffi::SQLITE_NOTADB).is_corruption());
    assert!(!sqlite_failure(rusqlite::ffi::SQLITE_CONSTRAINT).is_corruption());
    assert!(!IndexError::Query("x".into()).is_corruption());

    assert!(
        TransactionError::IndexStale(vec![
            IndexError::Query("x".into()),
            sqlite_failure(rusqlite::ffi::SQLITE_CORRUPT),
        ])
        .is_index_corruption(),
        "any one damaged-file error is enough"
    );
    assert!(
        !TransactionError::IndexStale(vec![IndexError::Query("x".into())]).is_index_corruption()
    );
}
