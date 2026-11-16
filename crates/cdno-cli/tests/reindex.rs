//! `cdno reindex` rebuilds the index in place (#999).
//!
//! Subprocess runs of the real binary against temp vaults. A process that
//! already has the index open (here, a second SQLite connection held by the
//! test) must read the rebuilt contents afterwards, on the same file.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use cdno_core::paths;
use cdno_core::store::{FsVaultStore, VaultStore};
use predicates::prelude::*;
use tempfile::tempdir;

fn cdno(vault: &Path) -> Command {
    let mut cmd = Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd.env_remove(paths::INDEX_PATH_ENV);
    cmd.env("NO_COLOR", "1");
    cmd.arg("--vault").arg(vault);
    cmd.timeout(Duration::from_secs(60));
    cmd
}

fn init(vault: &Path) {
    Command::cargo_bin("cdno")
        .unwrap()
        .env_remove("CUADERNO_VAULT_PATH")
        .env_remove(paths::INDEX_PATH_ENV)
        .arg("init")
        .arg(vault)
        .assert()
        .success();
}

fn write_zettel(vault: &Path, stem: &str, title: &str) {
    std::fs::write(
        vault.join(format!("{stem}.md")),
        format!("---\ntype: zettel\ntitle: {title}\n---\n# {title}\n"),
    )
    .unwrap();
}

/// A vault with two zettels, indexed once.
fn indexed_vault() -> tempfile::TempDir {
    let vault = tempdir().unwrap();
    init(vault.path());
    write_zettel(vault.path(), "surrogate-model", "Surrogate model");
    write_zettel(vault.path(), "kernel-choice", "Kernel choice");
    cdno(vault.path()).arg("reindex").assert().success();
    vault
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn inode(path: &Path) -> u64 {
    std::fs::metadata(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .ino()
}

fn note_count(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
        .unwrap()
}

/// Files beside the index whose names mark a rebuild or a set-aside copy.
fn leftovers(index: &Path) -> Vec<String> {
    let name = index.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::read_dir(index.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&format!("{name}.")))
        .collect()
}

#[test]
fn a_connection_holding_the_index_reads_the_rebuilt_contents_on_the_same_file() {
    let vault = indexed_vault();
    let index = vault.path().join(paths::INDEX_DB);

    // A second process's view: a WAL connection that stays open across the
    // rebuild, with a write still in the WAL (no checkpoint), that leaves
    // its index stale (a note row gone) and marked (a row the notes do not
    // produce).
    let conn = rusqlite::Connection::open(&index).unwrap();
    conn.execute_batch(
        "PRAGMA wal_autocheckpoint = 0;
         DELETE FROM notes WHERE path = 'kernel-choice.md';
         INSERT INTO index_meta (key, value) VALUES ('probe', 'stale');",
    )
    .unwrap();
    assert_eq!(note_count(&conn), 1);
    let wal = sibling(&index, "-wal");
    let shm = sibling(&index, "-shm");
    let before = (inode(&index), inode(&wal), inode(&shm));

    cdno(vault.path())
        .arg("reindex")
        .assert()
        .success()
        .stdout(predicate::str::contains("Reindexed 2 note(s) (2 scanned)."))
        .stdout(predicate::str::contains("damaged").not())
        .stdout(predicate::str::contains("Restart").not());

    // The same connection, untouched, sees the rebuilt index.
    assert_eq!(note_count(&conn), 2, "the rebuilt rows are visible");
    let probe: i64 = conn
        .query_row(
            "SELECT count(*) FROM index_meta WHERE key = 'probe'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(probe, 0, "the stale contents were replaced");
    assert_eq!(
        (inode(&index), inode(&wal), inode(&shm)),
        before,
        "the database and its sidecars are the same files"
    );
    assert_eq!(leftovers(&index), Vec::<String>::new());
}

#[test]
fn a_held_vault_lock_makes_reindex_wait_then_fail_with_the_index_as_it_was() {
    let vault = tempdir().unwrap();
    init(vault.path());
    write_zettel(vault.path(), "surrogate-model", "Surrogate model");
    cdno(vault.path()).arg("reindex").assert().success();
    let index = vault.path().join(paths::INDEX_DB);
    let before_inode = inode(&index);

    let store = FsVaultStore::new(vault.path());
    let lock = store.acquire_write_lock().unwrap();
    let started = Instant::now();
    cdno(vault.path())
        .arg("reindex")
        .assert()
        .failure()
        .stderr(predicate::str::contains("waiting for the vault write lock"))
        .stderr(predicate::str::contains("it was left as it was"));
    assert!(
        started.elapsed() >= Duration::from_secs(5),
        "it waited for the lock"
    );
    drop(lock);

    assert_eq!(inode(&index), before_inode, "nothing deleted or replaced");
    let conn = rusqlite::Connection::open(&index).unwrap();
    assert_eq!(note_count(&conn), 1, "the index still holds its note");
    assert_eq!(
        leftovers(&index),
        Vec::<String>::new(),
        "no rebuild file left"
    );
}

#[test]
fn a_damaged_index_that_opens_is_rebuilt_in_place_and_a_copy_kept() {
    let vault = indexed_vault();
    let index = vault.path().join(paths::INDEX_DB);
    // Fold the WAL in, then overwrite the `notes` table's root page: the
    // file opens (the schema on page 1 is intact) and fails its check.
    let (root_page, page_size) = {
        let conn = rusqlite::Connection::open(&index).unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        let root: i64 = conn
            .query_row(
                "SELECT rootpage FROM sqlite_master WHERE name = 'notes'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let size: i64 = conn
            .query_row("PRAGMA page_size", [], |r| r.get(0))
            .unwrap();
        (root as u64, size as u64)
    };
    {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&index)
            .unwrap();
        file.seek(SeekFrom::Start((root_page - 1) * page_size))
            .unwrap();
        file.write_all(&vec![0xA5u8; page_size as usize]).unwrap();
    }
    let before_inode = inode(&index);

    let previous = sibling(&index, ".previous");
    cdno(vault.path())
        .arg("reindex")
        .assert()
        .success()
        .stdout(predicate::str::contains("Reindexed 2 note(s) (2 scanned)."))
        .stdout(predicate::str::contains(
            "The index was damaged; it was rebuilt in place",
        ))
        .stdout(predicate::str::contains(previous.display().to_string()))
        .stdout(predicate::str::contains("Restart").not());

    assert_eq!(inode(&index), before_inode, "rebuilt in place");
    assert!(previous.is_file(), "a copy of the damaged file is kept");
    let conn = rusqlite::Connection::open(&index).unwrap();
    assert_eq!(note_count(&conn), 2);
    let check: String = conn
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(check, "ok");
}

#[test]
fn an_index_too_damaged_to_open_is_set_aside_and_the_output_says_to_restart() {
    let vault = tempdir().unwrap();
    init(vault.path());
    write_zettel(vault.path(), "surrogate-model", "Surrogate model");
    let index = vault.path().join(paths::INDEX_DB);
    // The SQLite header, then nothing SQLite can read.
    let mut corrupt = b"SQLite format 3\0".to_vec();
    corrupt.extend(std::iter::repeat_n(0xFFu8, 4096));
    std::fs::write(&index, &corrupt).unwrap();
    // Another process with the old file open keeps it.
    let held = std::fs::File::open(&index).unwrap();
    let held_inode = held.metadata().unwrap().ino();

    let previous = sibling(&index, ".previous");
    cdno(vault.path())
        .arg("reindex")
        .assert()
        .success()
        .stdout(predicate::str::contains("Reindexed 1 note(s) (1 scanned)."))
        .stdout(predicate::str::contains("too damaged to rebuild in place"))
        .stdout(predicate::str::contains(previous.display().to_string()))
        .stdout(predicate::str::contains(
            "Restart any process that had the index open",
        ));

    assert_eq!(
        std::fs::read(&previous).unwrap(),
        corrupt,
        "moved aside as it was"
    );
    assert_eq!(inode(&previous), held_inode, "the old file is the one held");
    assert_ne!(inode(&index), held_inode, "a new index file");
    let conn = rusqlite::Connection::open(&index).unwrap();
    assert_eq!(note_count(&conn), 1);
    drop(held);
}

// ---------------------------------------------------------------------
// `--rebind` at an overridden location (#998)
// ---------------------------------------------------------------------

fn bound_root(index: &Path) -> Option<String> {
    let conn = rusqlite::Connection::open(index).unwrap();
    conn.query_row(
        "SELECT value FROM index_meta WHERE key = 'vault_root'",
        [],
        |r| r.get(0),
    )
    .ok()
}

fn marker_snapshots(index: &Path) -> i64 {
    let conn = rusqlite::Connection::open(index).unwrap();
    conn.query_row(
        "SELECT count(*) FROM archived_action_snapshots WHERE frozen_hash = 'surrogate-marker'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn rebind_drops_the_other_vaults_snapshots_and_binds_to_this_vault() {
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let index = elsewhere.path().join("index.db");
    init(a.path());
    init(b.path());
    // The same note path in both vaults, so a carried row would fit B.
    write_zettel(a.path(), "surrogate-model", "Surrogate model");
    write_zettel(b.path(), "surrogate-model", "Surrogate model");
    cdno(a.path())
        .env(paths::INDEX_PATH_ENV, &index)
        .arg("reindex")
        .assert()
        .success();
    {
        let conn = rusqlite::Connection::open(&index).unwrap();
        conn.execute(
            "INSERT INTO archived_action_snapshots \
             (note_path, frozen_size, frozen_hash, archived_at_ns) \
             VALUES ('surrogate-model.md', 3, 'surrogate-marker', 42)",
            [],
        )
        .unwrap();
    }
    // A's own reindex carries its snapshot.
    cdno(a.path())
        .env(paths::INDEX_PATH_ENV, &index)
        .arg("reindex")
        .assert()
        .success();
    assert_eq!(marker_snapshots(&index), 1, "carried for the bound vault");

    let a_root = a.path().canonicalize().unwrap();
    let b_root = b.path().canonicalize().unwrap();
    cdno(b.path())
        .env(paths::INDEX_PATH_ENV, &index)
        .args(["reindex", "--rebind"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Reindexed 1 note(s) (1 scanned)."))
        .stdout(predicate::str::contains(format!(
            "Rebound the index from the vault at {}",
            a_root.display()
        )))
        .stdout(predicate::str::contains("archival snapshots included"));

    assert_eq!(
        marker_snapshots(&index),
        0,
        "the other vault's rows are gone"
    );
    assert_eq!(
        bound_root(&index).as_deref(),
        Some(b_root.to_string_lossy().as_ref()),
        "bound to this vault"
    );
}

#[test]
fn drop_snapshots_rebuilds_in_place_without_them() {
    let vault = indexed_vault();
    let index = vault.path().join(paths::INDEX_DB);
    {
        let conn = rusqlite::Connection::open(&index).unwrap();
        conn.execute(
            "INSERT INTO archived_action_snapshots \
             (note_path, frozen_size, frozen_hash, archived_at_ns) \
             VALUES ('surrogate-model.md', 3, 'surrogate-marker', 42)",
            [],
        )
        .unwrap();
    }
    let before_inode = inode(&index);

    cdno(vault.path())
        .arg("reindex")
        .assert()
        .success()
        .stdout(predicate::str::contains("snapshot").not());
    assert_eq!(marker_snapshots(&index), 1, "a plain reindex carries it");

    cdno(vault.path())
        .args(["reindex", "--drop-snapshots"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Reindexed 2 note(s) (2 scanned)."))
        .stdout(predicate::str::contains("Dropped 1 archival snapshot(s)"));
    assert_eq!(marker_snapshots(&index), 0);
    assert_eq!(inode(&index), before_inode, "still in place");
}
