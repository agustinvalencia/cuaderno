//! The index's file-identity check and its reopen (#1000), on real files:
//! what a live `SqliteIndex` does when its database file, or the `-wal` and
//! `-shm` beside it, are deleted or replaced under it.
//!
//! The deletions here are made by this process with `remove_file`, which is
//! what a process on another kernel (whose locks this one cannot see) does
//! when it closes as "the last connection", and what `cdno reindex`'s
//! set-aside and `--rebind` do to the database file.

use std::path::{Path, PathBuf};
use std::process::Command;

use cdno_core::error::IndexError;
use cdno_core::index::{
    ArchivalSnapshot, FileCheck, SqliteIndex, VaultIndex, canonical_vault_root,
};
use cdno_core::path::VaultPath;
use cdno_core::paths::IndexLocation;
use rusqlite::Connection;
use tempfile::TempDir;

fn new_index() -> (TempDir, PathBuf, SqliteIndex) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("index.db");
    let index = SqliteIndex::open(&path).unwrap();
    (dir, path, index)
}

fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn put(index: &SqliteIndex, value: &str) {
    index
        .with_connection(|c| {
            c.execute(
                "INSERT OR REPLACE INTO index_meta (key, value) VALUES ('probe', ?1)",
                [value],
            )
        })
        .unwrap();
}

fn probe(index: &SqliteIndex) -> Option<String> {
    index
        .with_connection(|c| {
            use rusqlite::OptionalExtension;
            c.query_row(
                "SELECT value FROM index_meta WHERE key = 'probe'",
                [],
                |r| r.get(0),
            )
            .optional()
        })
        .unwrap()
}

/// A finished index file at `path` holding `value`, closed, as a restore
/// or a rebuild elsewhere would leave it.
fn index_file_holding(path: &Path, value: &str) {
    let other = SqliteIndex::open(path).unwrap();
    put(&other, value);
}

/// Move the live files aside as `cdno reindex` does and put `replacement`
/// in their place.
fn replace_under(path: &Path, replacement: &Path) {
    SqliteIndex::set_aside(&IndexLocation {
        path: path.to_path_buf(),
        overridden: false,
    })
    .unwrap();
    std::fs::rename(replacement, path).unwrap();
}

const CHILD_ENV: &str = "CDNO_TEST_REOPEN_CHILD_DB";

/// Not a test of its own: the body of [`read_in_another_process`], which
/// runs this test binary again with the variable set. Without it, returns.
#[test]
fn child_reads_the_probe() {
    let Some(path) = std::env::var_os(CHILD_ENV) else {
        return;
    };
    let conn = Connection::open(path).unwrap();
    let value: String = conn
        .query_row(
            "SELECT value FROM index_meta WHERE key = 'probe'",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|e| format!("error: {e}"));
    println!("PROBE={value}");
}

/// The probe value a connection in another process reads from `path`: a
/// connection in this process would share this process's view of the
/// shared memory, which is what is under test.
fn read_in_another_process(path: &Path) -> String {
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "unit::index_reopen_tests::child_reads_the_probe",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .find_map(|l| l.split_once("PROBE=").map(|(_, v)| v))
        .unwrap_or_else(|| panic!("no probe line from the child:\n{stdout}"))
        .to_owned()
}

#[test]
fn a_freshly_opened_index_holds_its_files() {
    let (_dir, _path, index) = new_index();
    assert_eq!(index.check_files(), FileCheck::Held);
}

#[test]
fn checkpoints_and_other_connections_on_this_machine_never_look_like_moved_files() {
    let (_dir, path, index) = new_index();
    put(&index, "one");
    index
        .with_connection(|c| c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);"))
        .unwrap();
    assert_eq!(index.check_files(), FileCheck::Held);

    // Another connection writes, truncates the log and closes: it is not
    // the last connection while the index's is open, so it deletes nothing.
    {
        let other = Connection::open(&path).unwrap();
        other
            .execute(
                "INSERT OR REPLACE INTO index_meta (key, value) VALUES ('probe', 'two')",
                [],
            )
            .unwrap();
        other
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
    }
    assert!(beside(&path, "-wal").exists());
    assert!(beside(&path, "-shm").exists());
    put(&index, "three");
    assert_eq!(index.check_files(), FileCheck::Held);
    assert_eq!(probe(&index).as_deref(), Some("three"));
}

#[test]
fn a_live_index_whose_database_file_is_replaced_serves_the_new_file_after_a_reopen() {
    let (dir, path, index) = new_index();
    put(&index, "old");
    let replacement = dir.path().join("restored.db");
    index_file_holding(&replacement, "new");
    replace_under(&path, &replacement);

    // Until the reopen it still reads its own, now unlinked, file.
    assert_eq!(probe(&index).as_deref(), Some("old"));
    let found = index.check_files();
    assert!(matches!(found, FileCheck::DatabaseMoved(_)), "{found:?}");

    index.reopen().unwrap();

    assert_eq!(index.check_files(), FileCheck::Held);
    assert_eq!(probe(&index).as_deref(), Some("new"));
    put(&index, "written after the reopen");
    assert_eq!(read_in_another_process(&path), "written after the reopen");
}

#[test]
fn a_live_index_whose_database_file_is_deleted_reopens_on_a_new_empty_one() {
    let (_dir, path, index) = new_index();
    put(&index, "old");
    SqliteIndex::remove(&path);

    assert!(matches!(index.check_files(), FileCheck::DatabaseMoved(_)));
    index.reopen().unwrap();

    assert!(path.exists(), "the reopen creates the file, migrated");
    assert_eq!(probe(&index), None);
    assert_eq!(index.check_files(), FileCheck::Held);
}

#[test]
fn a_live_index_whose_sidecars_are_unlinked_reopens_and_its_next_write_is_seen_elsewhere() {
    let (_dir, path, index) = new_index();
    put(&index, "before");
    std::fs::remove_file(beside(&path, "-wal")).unwrap();
    std::fs::remove_file(beside(&path, "-shm")).unwrap();

    // It carries on with the orphans until told otherwise.
    put(&index, "into the orphaned log");
    let found = index.check_files();
    assert!(matches!(found, FileCheck::SidecarsMoved(_)), "{found:?}");
    assert!(found.finding().unwrap().contains("-wal"), "{found}");

    index.reopen().unwrap();

    assert_eq!(index.check_files(), FileCheck::Held);
    assert!(beside(&path, "-wal").exists());
    assert!(beside(&path, "-shm").exists());
    put(&index, "after the reopen");
    assert_eq!(read_in_another_process(&path), "after the reopen");
}

#[test]
fn sidecars_recreated_by_another_process_are_found_replaced() {
    let (_dir, path, index) = new_index();
    std::fs::remove_file(beside(&path, "-wal")).unwrap();
    std::fs::write(beside(&path, "-wal"), b"").unwrap();

    let found = index.check_files();
    assert!(matches!(found, FileCheck::SidecarsMoved(_)), "{found:?}");
    assert!(found.finding().unwrap().contains("replaced"), "{found}");
}

/// An open descriptor on the file at `path` now, and a way to ask later
/// whether that file still has a name: a file deleted and recreated under
/// the same name can come back with the same inode number (ext4 reuses
/// them readily), but the descriptor still points at the deleted one, whose
/// link count is then zero.
fn hold(path: &Path) -> std::fs::File {
    std::fs::File::open(path).unwrap()
}

fn still_linked(file: &std::fs::File) -> bool {
    use std::os::unix::fs::MetadataExt;
    file.metadata().unwrap().nlink() > 0
}

#[test]
fn a_reopen_after_the_sidecars_went_leaves_the_files_at_the_path_alone() {
    let (_dir, path, index) = new_index();
    put(&index, "old");
    // Another kernel's process deleted the sidecars and made its own.
    for suffix in ["-wal", "-shm"] {
        std::fs::remove_file(beside(&path, suffix)).unwrap();
        std::fs::write(beside(&path, suffix), b"").unwrap();
    }
    let wal = hold(&beside(&path, "-wal"));
    let shm = hold(&beside(&path, "-shm"));

    index.reopen().unwrap();

    // The old connection closed without deleting them.
    assert!(still_linked(&wal), "the -wal at the path was deleted");
    assert!(still_linked(&shm), "the -shm at the path was deleted");
    assert_eq!(index.check_files(), FileCheck::Held);
}

#[test]
fn dropping_an_index_that_does_not_hold_its_files_deletes_nothing_at_the_path() {
    let (_dir, path, index) = new_index();
    put(&index, "old");
    for suffix in ["-wal", "-shm"] {
        std::fs::remove_file(beside(&path, suffix)).unwrap();
        std::fs::write(beside(&path, suffix), b"someone else's").unwrap();
    }
    let wal = hold(&beside(&path, "-wal"));
    let shm = hold(&beside(&path, "-shm"));
    let main_before = std::fs::read(&path).unwrap();

    // A server shutting down on its orphans.
    drop(index);

    assert!(still_linked(&wal), "the -wal at the path was deleted");
    assert!(still_linked(&shm), "the -shm at the path was deleted");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        main_before,
        "the orphaned log was checkpointed into the shared file"
    );
}

#[test]
fn dropping_an_index_that_holds_its_files_checkpoints_and_cleans_up_as_before() {
    let (_dir, path, index) = new_index();
    put(&index, "kept");
    drop(index);
    assert!(
        !beside(&path, "-wal").exists(),
        "the last close removes its log"
    );
    let conn = Connection::open(&path).unwrap();
    let value: String = conn
        .query_row(
            "SELECT value FROM index_meta WHERE key = 'probe'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(value, "kept");
}

/// The behaviour the reopen guards against, kept as evidence: SQLite's own
/// close of the last connection on this kernel checkpoints its orphaned log
/// and deletes whatever `-wal` and `-shm` the path names, someone else's.
#[test]
fn without_the_guard_sqlite_s_close_deletes_the_sidecars_the_path_names() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plain.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE t (x); INSERT INTO t VALUES (1);")
        .unwrap();
    for suffix in ["-wal", "-shm"] {
        std::fs::remove_file(beside(&path, suffix)).unwrap();
        std::fs::write(beside(&path, suffix), b"someone else's").unwrap();
    }
    let wal = hold(&beside(&path, "-wal"));

    drop(conn);

    assert!(!still_linked(&wal));
    assert!(!beside(&path, "-shm").exists());
}

#[test]
fn a_restore_that_replaces_only_the_main_file_is_read_after_the_old_sidecars_are_set_aside() {
    let (dir, path, index) = new_index();
    put(&index, "old");
    // Keep the old commit in the log, as a busy server's would be.
    let restored = dir.path().join("backup.db");
    index_file_holding(&restored, "restored");
    // `mv backup.db index.db`: the server's own -wal and -shm stay.
    std::fs::rename(&restored, &path).unwrap();
    assert!(matches!(index.check_files(), FileCheck::DatabaseMoved(_)));

    let refused = index.reopen().unwrap_err();
    assert!(
        matches!(refused, IndexError::StaleSidecars(_)),
        "{refused:?}"
    );
    assert_eq!(probe(&index).as_deref(), Some("old"), "nothing changed");

    let reopened = index.reopen_setting_aside_stale_sidecars().unwrap();

    assert_eq!(reopened.stale_sidecars_moved_to.len(), 2);
    assert!(beside(&path, ".stale-wal").exists());
    assert_eq!(probe(&index).as_deref(), Some("restored"));
    assert_eq!(index.check_files(), FileCheck::Held);
    assert_eq!(read_in_another_process(&path), "restored");
}

#[test]
fn a_reopen_hands_back_the_snapshots_the_old_connection_could_read() {
    let (_dir, path, index) = new_index();
    let note = VaultPath::new("actions/_done/2026/surrogate-model-sweep.md").unwrap();
    let snapshot = ArchivalSnapshot {
        frozen_size: 3,
        frozen_hash: "abc".into(),
        archived_at_ns: 1,
        masked_hash: None,
        mask_version: None,
        masked_size: None,
    };
    index
        .with_connection(|c| {
            c.execute(
                "INSERT INTO notes (path, note_type, title, content_hash, mtime_ns, size, \
                 frontmatter, indexed_at_ns) VALUES (?1, 'action', 't', 'h', 0, 0, '{}', 0)",
                [note.to_string()],
            )
        })
        .unwrap();
    index.record_archival_snapshot(&note, &snapshot).unwrap();
    std::fs::remove_file(beside(&path, "-wal")).unwrap();
    std::fs::remove_file(beside(&path, "-shm")).unwrap();

    let reopened = index.reopen().unwrap();

    let salvaged = reopened.salvaged.expect("read").expect("readable");
    assert_eq!(salvaged, vec![(note.clone(), snapshot)]);
    assert_eq!(
        index.find_archival_snapshot(&note).unwrap(),
        None,
        "the row was only in the orphaned log; carrying it is the caller's"
    );
}

#[test]
fn a_sidecar_that_cannot_be_checked_is_not_taken_for_a_move() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let inner = dir.path().join("locked");
    std::fs::create_dir(&inner).unwrap();
    let path = inner.join("index.db");
    let index = SqliteIndex::open(&path).unwrap();
    // No search permission on the directory: every `stat` in it fails
    // with EACCES (not as root, which CI and contributors are not).
    std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o600)).unwrap();
    let found = index.check_files();
    std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o700)).unwrap();

    if std::fs::metadata(&path).is_ok() && found.is_held() {
        // Running as root: permissions do not apply; nothing to show.
        return;
    }
    assert!(matches!(found, FileCheck::Unchecked(_)), "{found:?}");
    assert!(!found.needs_reopen());
    assert_eq!(index.check_files(), FileCheck::Held);
}

#[test]
fn a_reopen_of_an_overridden_index_keeps_its_binding() {
    let vault = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    let location = IndexLocation {
        path: elsewhere.path().join("override.db"),
        overridden: true,
    };
    let index = SqliteIndex::open_location(&location, vault.path()).unwrap();
    SqliteIndex::remove(&location.path);

    index.reopen().unwrap();

    assert_eq!(
        SqliteIndex::bound_vault_root(&location.path).unwrap(),
        Some(canonical_vault_root(vault.path()))
    );
    assert_eq!(index.check_files(), FileCheck::Held);
}

#[test]
fn a_reopen_refuses_a_file_bound_to_another_vault_and_keeps_the_old_connection() {
    let vault = TempDir::new().unwrap();
    let other_vault = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    let location = IndexLocation {
        path: elsewhere.path().join("override.db"),
        overridden: true,
    };
    let index = SqliteIndex::open_location(&location, vault.path()).unwrap();
    put(&index, "ours");
    // `reindex --rebind` from the other vault: deleted and recreated, bound
    // to it.
    SqliteIndex::remove(&location.path);
    drop(SqliteIndex::open_location(&location, other_vault.path()).unwrap());

    let refused = index.reopen().unwrap_err();

    assert!(
        matches!(refused, IndexError::BoundToOtherVault { .. }),
        "{refused:?}"
    );
    assert_eq!(probe(&index).as_deref(), Some("ours"));
    assert!(matches!(index.check_files(), FileCheck::DatabaseMoved(_)));
}

#[test]
fn a_reopen_that_cannot_open_the_same_file_again_leaves_the_index_detached_until_one_can() {
    let (_dir, path, index) = new_index();
    std::fs::remove_file(beside(&path, "-wal")).unwrap();
    std::fs::remove_file(beside(&path, "-shm")).unwrap();
    // The same file, now unreadable to a fresh connection.
    let mut bytes = std::fs::read(&path).unwrap();
    let saved = bytes.clone();
    bytes[..100].fill(0xA5);
    std::fs::write(&path, &bytes).unwrap();

    let failed = index.reopen().unwrap_err();
    assert!(failed.is_corruption(), "{failed:?}");
    assert!(matches!(index.check_files(), FileCheck::Detached(_)));
    assert!(
        matches!(index.list_all_paths(), Err(IndexError::Detached)),
        "a detached index answers nothing, and says why"
    );

    std::fs::write(&path, saved).unwrap();
    index.reopen().unwrap();
    assert_eq!(index.check_files(), FileCheck::Held);
}
