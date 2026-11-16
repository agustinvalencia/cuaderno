//! `restart_required` (#526 review): a header too damaged for the running
//! server to write over. Its own target, with one test, because the hint
//! a client error carries is process-wide.
//!
//! The recipe: a second connection opened before the damage, a checkpoint
//! so page 1 is no longer in the WAL, the header damaged in the main file,
//! then a commit through the second connection, so the server's page cache
//! is stale and its next read of page 1 comes from the damaged file. The
//! check and then the in-place copy both read that header and fail.

use std::path::Path;

use cdno_core::paths;
use cdno_mcp::bootstrap::open_vault_checked;
use cdno_mcp::index_health::{IndexHealth, RepairPolicy, RepairState, damage_hint};
use tempfile::TempDir;

#[test]
fn a_header_the_server_cannot_write_over_asks_for_a_restart_and_stops_repairing() {
    let dir = TempDir::new().unwrap();
    cdno_cli::commands::init::run(dir.path()).expect("cdno init");
    let opened = open_vault_checked(dir.path(), None).unwrap();
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
    let rebuild = status.last_rebuild.clone().expect("the sweep tried");
    assert!(!rebuild.rebuilt);
    assert!(rebuild.restart_required, "{:?}", rebuild.failure);
    assert!(status.restart_required);
    assert_eq!(status.repair, RepairState::RestartRequired);
    assert!(!status.healthy);
    assert!(
        status.advice.as_deref().unwrap_or("").contains("restart"),
        "{:?}",
        status.advice
    );
    let hint = damage_hint();
    assert!(hint.contains("needs a restart"), "{hint}");
    assert!(
        !hint.contains("rebuilds it"),
        "no promise of a rebuild: {hint}"
    );

    // Automatic repair is refused: no new attempt on the next sweep or
    // tool-call signal.
    let attempted = rebuild.at.clone();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    health.sweep();
    health.on_corruption_seen();
    assert_eq!(
        health.status().last_rebuild.unwrap().at,
        attempted,
        "no automatic rebuild while a restart is required"
    );
}

/// Overwrite the first 100 bytes, the database header, of the file.
fn damage_header(path: &Path) {
    let mut bytes = std::fs::read(path).unwrap();
    bytes[..100].fill(0xA5);
    std::fs::write(path, bytes).unwrap();
}
