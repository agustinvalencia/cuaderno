//! A header too damaged for the running server to write over (#526
//! review), which until #1000 stopped the server's repairs and asked for a
//! restart. Now the file is set aside and the index reopened on a fresh
//! one, without a restart. Its own target, with one test, because the hint
//! a client error carries is process-wide.
//!
//! The recipe: a second connection opened before the damage, a checkpoint
//! so page 1 is no longer in the WAL, the header damaged in the main file,
//! then a commit through the second connection, so the server's page cache
//! is stale and its next read of page 1 comes from the damaged file. The
//! check and then the in-place copy both read that header and fail; a
//! fresh connection cannot open the file either, so a reopen alone would
//! not do: the file has to go.

use std::path::Path;

use cdno_core::index::PREVIOUS_SUFFIX;
use cdno_core::paths;
use cdno_mcp::bootstrap::open_vault_checked;
use cdno_mcp::index_health::{IndexHealth, RepairPolicy, RepairState, damage_hint};
use tempfile::TempDir;

#[test]
fn a_header_the_server_cannot_write_over_is_set_aside_and_replaced_without_a_restart() {
    let dir = TempDir::new().unwrap();
    cdno_cli::commands::init::run(dir.path()).expect("cdno init");
    let daily = dir.path().join("journal/2026/daily");
    std::fs::create_dir_all(&daily).unwrap();
    std::fs::write(
        daily.join("2026-03-02.md"),
        "---\ntype: daily\n---\n# 2026-03-02\n\n## Logs\n\n- **09:00**: surrogate model\n",
    )
    .unwrap();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let indexed = opened.index.list_all_paths().unwrap().len();
    assert!(indexed > 0);
    let health = IndexHealth::from_opened(&opened).with_policy(RepairPolicy {
        min_rebuild_interval: std::time::Duration::ZERO,
        ..RepairPolicy::default()
    });
    let path = dir.path().join(paths::INDEX_DB);

    let other = rusqlite::Connection::open(&path).unwrap();
    other
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    damage_header(&path);
    other
        .execute(
            "INSERT INTO index_meta (key, value) VALUES ('probe', 'stale the cache')",
            [],
        )
        .unwrap();

    health.sweep();

    let status = health.status();
    let rebuild = status.last_rebuild.clone().expect("the sweep repaired");
    assert!(rebuild.rebuilt, "{:?}", rebuild.failure);
    assert!(rebuild.set_aside, "{rebuild:?}");
    assert_eq!(rebuild.notes, indexed);
    assert!(!rebuild.snapshots_lost.is_empty(), "the loss is said");
    assert!(status.healthy, "{status:?}");
    assert!(status.files.held);
    assert_eq!(status.repair, RepairState::Automatic);
    assert!(status.advice.is_none());
    assert_eq!(status.damage_rebuilds_recent, 1, "it was a repair");
    assert_eq!(status.reopens_recent, 0, "not a reopen of moved files");
    assert_eq!(opened.index.list_all_paths().unwrap().len(), indexed);
    let hint = damage_hint();
    assert!(hint.contains("rebuilds it if it is"), "{hint}");

    // The damaged file is kept for diagnosis; the file at the path is new
    // and sound for any other process.
    let mut previous = path.clone().into_os_string();
    previous.push(PREVIOUS_SUFFIX);
    assert_eq!(&std::fs::read(&previous).unwrap()[..4], &[0xA5; 4]);
    let fresh = rusqlite::Connection::open(&path).unwrap();
    let check: String = fresh
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(check, "ok");
    drop(other);
}

/// Overwrite the first 100 bytes, the database header, of the file.
fn damage_header(path: &Path) {
    let mut bytes = std::fs::read(path).unwrap();
    bytes[..100].fill(0xA5);
    std::fs::write(path, bytes).unwrap();
}
