//! Index health for a long-lived server (#526): a damaged index found at
//! open, a sweep that repairs one behind the live handle, the spacing
//! between rebuilds, and the two tools that report and force it.
//!
//! Real files throughout, since the point is what SQLite does with a
//! damaged database file on disk.

use std::path::Path;
use std::sync::Arc;

use cdno_core::paths;
use cdno_mcp::CuadernoServer;
use cdno_mcp::bootstrap::{open_vault_checked, open_vault_with_index};
use cdno_mcp::index_health::{CheckKind, IndexHealth, RebuildRefused, RepairPolicy, RepairState};
use cdno_mcp::server::EmptyInput;
use chrono::{Days, NaiveDate};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, RawContent};
use tempfile::TempDir;

const NOTES: usize = 300;

/// A vault with enough daily notes that its index spans many pages.
fn vault_with_notes() -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    cdno_cli::commands::init::run(dir.path()).expect("cdno init");
    let start = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let padding = "Surrogate model notes. ".repeat(30);
    for i in 0..NOTES {
        let date = start.checked_add_days(Days::new(i as u64)).unwrap();
        let folder = dir
            .path()
            .join(format!("journal/{}/daily", date.format("%Y")));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join(format!("{date}.md")),
            format!("---\ntype: daily\n---\n# {date}\n\n## Logs\n\n- **09:00**: {padding}\n"),
        )
        .unwrap();
    }
    dir
}

fn index_path(root: &Path) -> std::path::PathBuf {
    root.join(paths::INDEX_DB)
}

/// Index the vault, close it, and overwrite the last page of its
/// database with garbage. The header and schema pages survive, so the
/// file still opens: only an integrity check sees the damage.
fn damage_index(root: &Path) {
    drop(open_vault_with_index(root, None).expect("first open indexes the vault"));
    damage_last_page(&index_path(root));
}

/// Overwrite the last page of the closed database at `path` with garbage.
fn damage_last_page(path: &Path) {
    let page_size: u64 = rusqlite::Connection::open(path)
        .unwrap()
        .query_row("PRAGMA page_size", [], |r| r.get(0))
        .unwrap();
    let mut bytes = std::fs::read(path).unwrap();
    let start = bytes.len() - page_size as usize;
    for b in &mut bytes[start..] {
        *b = 0xA5;
    }
    std::fs::write(path, bytes).unwrap();
}

fn indexed_notes(opened: &cdno_mcp::bootstrap::OpenedVault) -> usize {
    opened.index.list_all_paths().unwrap().len()
}

fn decode_json(result: &CallToolResult) -> serde_json::Value {
    match &result.content[0].raw {
        RawContent::Text(t) => serde_json::from_str(&t.text).unwrap(),
        other => panic!("expected text content, got {other:?}"),
    }
}

#[test]
fn a_checked_open_rebuilds_a_damaged_index_and_keeps_a_copy() {
    let dir = vault_with_notes();
    damage_index(dir.path());

    let opened = open_vault_checked(dir.path(), None).expect("the checked open recovers");

    let damage = opened
        .damage_at_open
        .clone()
        .expect("the damage is reported");
    assert!(!damage.is_empty());
    assert!(
        opened.sqlite.quick_check().unwrap().is_empty(),
        "the new index is sound"
    );
    assert!(
        indexed_notes(&opened) >= NOTES,
        "every note is indexed again"
    );
    let mut previous = index_path(dir.path()).into_os_string();
    previous.push(cdno_core::index::PREVIOUS_SUFFIX);
    assert!(
        Path::new(&previous).exists(),
        "the damaged file is kept for diagnosis"
    );
}

#[test]
fn a_checked_open_of_a_sound_index_changes_nothing() {
    let dir = vault_with_notes();
    drop(open_vault_with_index(dir.path(), None).unwrap());

    let opened = open_vault_checked(dir.path(), None).unwrap();

    assert!(opened.damage_at_open.is_none());
    let mut previous = index_path(dir.path()).into_os_string();
    previous.push(cdno_core::index::PREVIOUS_SUFFIX);
    assert!(!Path::new(&previous).exists());
}

#[test]
fn a_sweep_rebuilds_a_damaged_index_behind_the_live_handle() {
    let dir = vault_with_notes();
    damage_index(dir.path());
    // Unchecked, as before #526: the damaged file opens and is served.
    let opened = open_vault_with_index(dir.path(), None).unwrap();
    assert!(!opened.sqlite.quick_check().unwrap().is_empty());
    let health = IndexHealth::from_opened(&opened);

    health.sweep();

    let status = health.status();
    assert!(status.check.ok, "{:?}", status.check.problems);
    let rebuild = status.last_rebuild.expect("the sweep rebuilt the index");
    assert!(rebuild.rebuilt, "{:?}", rebuild.failure);
    assert!(rebuild.notes >= NOTES);
    // The vault reads through the same handle and sees the new index.
    assert!(indexed_notes(&opened) >= NOTES);
    assert!(
        opened
            .vault
            .search("surrogate", &Default::default(), 5)
            .unwrap()
            .len()
            == 5,
        "search answers from the rebuilt index"
    );
}

#[test]
fn a_sweep_of_a_sound_index_does_not_rebuild() {
    let dir = vault_with_notes();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let health = IndexHealth::from_opened(&opened);

    health.sweep();

    let status = health.status();
    assert!(status.healthy);
    assert!(status.last_rebuild.is_none());
    let sweep = status.last_sweep.expect("the sweep is recorded");
    assert_eq!(sweep.errors, 0);
    assert!(sweep.scanned >= NOTES);
}

#[test]
fn rebuilds_are_spaced_apart() {
    let dir = vault_with_notes();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let health = IndexHealth::from_opened(&opened);

    let first = health.rebuild("test").expect("the first rebuild runs");
    assert!(first.rebuilt);
    match health.rebuild("test") {
        Err(RebuildRefused::TooSoon(wait)) => {
            assert!(wait <= cdno_mcp::index_health::MIN_REBUILD_INTERVAL)
        }
        other => panic!("a second rebuild straight away must be refused, got {other:?}"),
    }
}

#[tokio::test]
async fn index_status_and_rebuild_index_over_the_handlers() {
    let dir = vault_with_notes();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let health = Arc::new(IndexHealth::from_opened(&opened));
    let server = CuadernoServer::new(Arc::new(opened.vault), dir.path().to_path_buf())
        .with_index_health(health);

    let status = decode_json(
        &server
            .index_status(Parameters(EmptyInput {}))
            .await
            .unwrap(),
    );
    assert_eq!(status["healthy"], true, "{status}");
    assert_eq!(status["check"]["kind"], "quick_check");
    assert!(status["damage_at_open"].is_null());

    let rebuilt = decode_json(
        &server
            .rebuild_index(Parameters(EmptyInput {}))
            .await
            .unwrap(),
    );
    assert_eq!(rebuilt["rebuilt"], true, "{rebuilt}");
    assert!(rebuilt["notes"].as_u64().unwrap() >= NOTES as u64);
    assert_eq!(rebuilt["trigger"], "rebuild_index was called");

    let refused = server
        .rebuild_index(Parameters(EmptyInput {}))
        .await
        .expect_err("a second rebuild straight away is refused");
    assert!(
        refused.message.contains("rebuilt moments ago"),
        "{}",
        refused.message
    );
    let data = refused.data.expect("a refusal carries a rejection payload");
    assert!(data.to_string().contains("rebuild_refused"), "{data}");
}

#[tokio::test]
async fn the_tools_say_so_on_a_server_without_index_health() {
    let dir = vault_with_notes();
    let opened = open_vault_with_index(dir.path(), None).unwrap();
    let server = CuadernoServer::new(Arc::new(opened.vault), dir.path().to_path_buf());

    let err = server
        .index_status(Parameters(EmptyInput {}))
        .await
        .unwrap_err();
    assert!(err.message.contains("not available"), "{}", err.message);
}

#[test]
fn a_rebuild_carries_the_archival_snapshots_across() {
    // The snapshots are the one table the notes cannot regenerate, so an
    // automatic repair must not drop them the way deleting the file does.
    let dir = vault_with_notes();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let note = opened
        .index
        .list_all_paths()
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let snapshot = cdno_core::index::ArchivalSnapshot {
        frozen_size: 42,
        frozen_hash: "abc".into(),
        archived_at_ns: 7,
        masked_hash: None,
        mask_version: None,
        masked_size: None,
    };
    opened
        .index
        .record_archival_snapshot(&note, &snapshot)
        .unwrap();
    let health = IndexHealth::from_opened(&opened);

    let record = health.rebuild("test").unwrap();

    assert!(record.rebuilt, "{:?}", record.failure);
    assert_eq!(record.snapshots_carried, 1);
    assert!(
        record.snapshots_lost.is_empty(),
        "{:?}",
        record.snapshots_lost
    );
    assert_eq!(
        opened.index.find_archival_snapshot(&note).unwrap(),
        Some(snapshot),
        "the live handle reads the carried row"
    );
}

#[test]
fn a_repair_reaches_another_process_holding_the_index() {
    // A second open stands in for a second server on the same vault: it
    // holds its own connection and never rebuilds anything itself.
    let dir = vault_with_notes();
    damage_index(dir.path());
    let first = open_vault_with_index(dir.path(), None).unwrap();
    let second = open_vault_with_index(dir.path(), None).unwrap();
    assert!(!second.sqlite.quick_check().unwrap().is_empty());

    let record = IndexHealth::from_opened(&first).rebuild("test").unwrap();

    assert!(record.rebuilt, "{:?}", record.failure);
    assert!(
        second.sqlite.quick_check().unwrap().is_empty(),
        "the other process reads the repair"
    );
    assert!(indexed_notes(&second) >= NOTES);
}

#[test]
fn a_note_that_does_not_parse_does_not_make_the_index_unhealthy() {
    let dir = vault_with_notes();
    std::fs::write(
        dir.path().join("journal/2025/daily/2025-01-01.md"),
        "---\ntype: [unclosed\n---\n# broken\n",
    )
    .unwrap();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let health = IndexHealth::from_opened(&opened);

    health.sweep();

    let status = health.status();
    assert_eq!(status.notes_not_indexed, 1, "{:?}", status.last_sweep);
    assert!(status.healthy, "a malformed note is not a damaged index");
    assert!(status.last_rebuild.is_none(), "and it is not rebuilt for");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_corruption_signal_starts_a_repair_without_a_sweep_loop() {
    // The stdio server runs no sweeps; a tool call meeting damage must
    // still start the repair (PR #997 review).
    let dir = vault_with_notes();
    damage_index(dir.path());
    let opened = open_vault_with_index(dir.path(), None).unwrap();
    let health = Arc::new(IndexHealth::from_opened(&opened));
    health.spawn_loop(None);

    cdno_mcp::index_health::corruption_seen();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let rebuilt = health.status().last_rebuild.is_some_and(|r| r.rebuilt);
        if rebuilt {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no repair after the signal"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(opened.sqlite.quick_check().unwrap().is_empty());
}

#[test]
fn writes_committed_during_a_rebuild_are_in_the_index_afterwards() {
    // The final pass runs under the vault write lock, so a write that
    // reaches the notes after the unlocked passes is in the index after
    // the copy (second review round on #997: 119 of 984 captures were
    // missing without it). The seam lands the writes in exactly that
    // window, so the test cannot pass by missing it.
    let dir = vault_with_notes();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let at = NaiveDate::from_ymd_opt(2026, 3, 1)
        .unwrap()
        .and_hms_opt(9, 0, 0)
        .unwrap();
    let mut written = 0;
    let report = cdno_core::reconcile::rebuild_with(
        &opened.store,
        &opened.sqlite,
        &opened.scope,
        true,
        || {
            for i in 0..20 {
                opened
                    .vault
                    .capture_to_inbox(at, &format!("surrogate model idea {i}"))
                    .expect("capture before the lock");
                written += 1;
            }
        },
    )
    .expect("the rebuild runs");

    assert!(report.notes >= NOTES + written);
    let on_disk = std::fs::read_dir(dir.path().join("inbox")).unwrap().count();
    assert_eq!(on_disk, written);
    let indexed = opened
        .index
        .list_all_paths()
        .unwrap()
        .iter()
        .filter(|p| p.to_string().starts_with("inbox/"))
        .count();
    assert_eq!(
        indexed, on_disk,
        "every capture that reported success is indexed"
    );
}

#[test]
fn damage_to_the_file_header_under_a_live_connection_is_repaired_in_place() {
    // The server's connection holds the header in its cache, so the
    // in-place copy writes it back: the running server repairs the file,
    // and `restart_required` stays false. (That state is reached only when
    // the copy itself fails as damage; no test reproduces that with a live
    // connection.)
    let dir = vault_with_notes();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let path = index_path(dir.path());
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[..100].fill(0xA5);
    std::fs::write(&path, bytes).unwrap();
    let health = IndexHealth::from_opened(&opened);

    let record = health.rebuild("test").unwrap();

    assert!(record.rebuilt, "{:?}", record.failure);
    assert!(!record.restart_required);
    let status = health.status();
    assert!(!status.restart_required);
    assert_eq!(status.repair, RepairState::Automatic);
    // Closing the last connection checkpoints the copy into the main file.
    drop(health);
    drop(opened);
    let on_disk = std::fs::read(&path).unwrap();
    assert_eq!(
        &on_disk[..16],
        b"SQLite format 3\0",
        "the header is written back"
    );
}

/// Damage only `integrity_check` sees: an index whose declared column no
/// longer matches the b-tree it was built from, so its entries are all
/// "missing" while `quick_check`, which does not compare an index with its
/// table, passes. The #526 shape.
fn damage_an_index_btree(root: &Path) {
    drop(open_vault_with_index(root, None).expect("first open indexes the vault"));
    let conn = rusqlite::Connection::open(index_path(root)).unwrap();
    conn.execute_batch(
        "PRAGMA writable_schema = ON;
         UPDATE sqlite_master SET sql = 'CREATE INDEX idx_notes_type ON notes(title)'
             WHERE name = 'idx_notes_type';
         PRAGMA writable_schema = OFF;",
    )
    .unwrap();
}

#[test]
fn damage_only_the_thorough_check_sees_is_outstanding_until_repaired() {
    let dir = vault_with_notes();
    damage_an_index_btree(dir.path());
    let opened = open_vault_with_index(dir.path(), None).unwrap();
    let health = IndexHealth::from_opened(&opened);

    assert!(
        health.check(CheckKind::QuickCheck).ok,
        "precondition: the quick check cannot see it"
    );
    let thorough = health.check(CheckKind::IntegrityCheck);
    assert!(!thorough.ok, "the thorough check sees it");

    // A quick pass does not clear it, and `index_status` reports the
    // thorough result (reused, not rescanned) rather than a healthy quick one.
    assert!(health.check(CheckKind::QuickCheck).ok);
    let status = health.status();
    assert!(!status.healthy);
    assert_eq!(status.check.kind, CheckKind::IntegrityCheck);
    assert_eq!(status.check.at, thorough.at);

    // The sweep acts on the outstanding damage although its own quick
    // check would pass: it escalates, and repairs.
    health.sweep();
    let status = health.status();
    assert!(status.healthy, "{:?}", status.check);
    assert!(status.last_rebuild.is_some_and(|r| r.rebuilt));
    assert!(health.check(CheckKind::IntegrityCheck).ok);
}

#[test]
fn two_servers_opening_one_damaged_index_never_undo_each_others_repair() {
    // Two servers starting on one damaged file at once (PR #997 review):
    // whichever repairs second, or gives up waiting on the lock the other
    // holds, must not empty the index the other just repaired, which
    // would drop the archival snapshots it carried, nor set it aside. So
    // after both opens the file is sound, still where it was, and still
    // holds the snapshot, whatever order the two ran in.
    let dir = vault_with_notes();
    let note = {
        let opened = open_vault_with_index(dir.path(), None).unwrap();
        let note = opened
            .index
            .list_all_paths()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        opened
            .index
            .record_archival_snapshot(&note, &snapshot())
            .unwrap();
        note
    };
    damage_last_page(&index_path(dir.path()));

    let open = |root: std::path::PathBuf| {
        std::thread::spawn(move || open_vault_checked(&root, None).expect("each server starts"))
    };
    let a = open(dir.path().to_path_buf());
    let b = open(dir.path().to_path_buf());
    let (a, b) = (a.join().unwrap(), b.join().unwrap());

    assert!(a.damage_at_open.is_some() || b.damage_at_open.is_some());
    assert!(index_path(dir.path()).exists(), "not set aside");
    for opened in [&a, &b] {
        assert!(opened.sqlite.quick_check().unwrap().is_empty());
        assert!(indexed_notes(opened) >= NOTES, "not emptied");
        assert_eq!(
            opened.index.find_archival_snapshot(&note).unwrap(),
            Some(snapshot()),
            "the carried snapshot survives the other server's start"
        );
    }
}

#[test]
fn an_open_whose_rebuild_cannot_get_the_lock_starts_on_the_index_as_it_is() {
    // The failure the review traced: the vault lock held past its timeout
    // (another server repairing the same file). The open must start on
    // the index as it is, never empty it or set it aside. A vault of one
    // large note keeps the per-note lock waits few; the lock is held for
    // the whole open, so every wait in it times out.
    let dir = TempDir::new().unwrap();
    cdno_cli::commands::init::run(dir.path()).expect("cdno init");
    let folder = dir.path().join("journal/2025/daily");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("2025-01-01.md"),
        format!(
            "---\ntype: daily\n---\n# 2025-01-01\n\n## Logs\n\n- **09:00**: {}\n",
            "Surrogate model notes. ".repeat(4000)
        ),
    )
    .unwrap();
    damage_index(dir.path());
    let before = std::fs::metadata(index_path(dir.path())).unwrap().len();
    let store = cdno_core::store::FsVaultStore::new(dir.path());
    let held = cdno_core::store::VaultStore::acquire_write_lock(&store).expect("the other server");

    let opened = open_vault_checked(dir.path(), None).expect("the open still starts");
    drop(held);

    assert!(opened.damage_at_open.is_some(), "it saw the damage");
    assert!(
        !opened.sqlite.quick_check().unwrap().is_empty(),
        "the file is as it was: not emptied in place, not replaced"
    );
    assert!(index_path(dir.path()).exists(), "not set aside");
    assert!(std::fs::metadata(index_path(dir.path())).unwrap().len() >= before);
}

fn snapshot() -> cdno_core::index::ArchivalSnapshot {
    cdno_core::index::ArchivalSnapshot {
        frozen_size: 42,
        frozen_hash: "abc".into(),
        archived_at_ns: 7,
        masked_hash: None,
        mask_version: None,
        masked_size: None,
    }
}

/// A zero spacing, so a test reaches the recurrence limit without waiting.
fn eager() -> RepairPolicy {
    RepairPolicy {
        min_rebuild_interval: std::time::Duration::ZERO,
        ..RepairPolicy::default()
    }
}

/// Damage the live index through its own connection, as another writer on
/// the same file would: a schema-less file fails the check.
fn damage_live(opened: &cdno_mcp::bootstrap::OpenedVault) {
    opened
        .sqlite
        .with_connection(|c| c.execute_batch("DROP TABLE schema_migrations"))
        .unwrap();
}

#[test]
fn recurring_damage_stops_the_automatic_repair() {
    let dir = vault_with_notes();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let health = IndexHealth::from_opened(&opened).with_policy(eager());
    let previous = beside(&index_path(dir.path()), cdno_core::index::PREVIOUS_SUFFIX);

    let mut first_copy = None;
    for round in 1..=3 {
        damage_live(&opened);
        health.sweep();
        let status = health.status();
        assert!(
            status.last_rebuild.as_ref().is_some_and(|r| r.rebuilt),
            "round {round} is repaired"
        );
        assert_eq!(status.damage_rebuilds_recent, round);
        assert_eq!(status.repair, RepairState::Automatic);
        // The first round's copy of the damaged file is the one kept.
        let copy = std::fs::read(&previous).expect("a copy is kept");
        match &first_copy {
            None => first_copy = Some(copy),
            Some(first) => assert_eq!(&copy, first, "round {round} overwrote the evidence"),
        }
    }

    damage_live(&opened);
    let rebuilt_before = health.status().last_rebuild.unwrap().at;
    health.sweep();
    let status = health.status();
    assert_eq!(status.repair, RepairState::DamageRecurring);
    assert!(!status.healthy);
    assert_eq!(status.damage_rebuilds_recent, 3);
    assert_eq!(
        status.last_rebuild.unwrap().at,
        rebuilt_before,
        "no fourth automatic rebuild"
    );
    let advice = status.advice.expect("advice");
    assert!(advice.contains("CUADERNO_INDEX_PATH"), "{advice}");
    assert!(advice.contains("another machine"), "{advice}");
    assert!(
        !opened.sqlite.quick_check().unwrap().is_empty(),
        "still damaged"
    );

    // An explicit `rebuild_index` still runs, and ends the stop, but keeps
    // the history: it buys one more automatic repair, not a fresh count.
    let record = health.rebuild("rebuild_index was called").unwrap();
    assert!(record.rebuilt, "{:?}", record.failure);
    let status = health.status();
    assert!(status.healthy);
    assert_eq!(status.repair, RepairState::Automatic);
    assert_eq!(status.damage_rebuilds_recent, 3);

    damage_live(&opened);
    health.sweep();
    let status = health.status();
    assert!(
        status.last_rebuild.as_ref().is_some_and(|r| r.rebuilt),
        "the one repair the requested rebuild allowed"
    );
    assert_eq!(status.repair, RepairState::Automatic);

    damage_live(&opened);
    health.sweep();
    assert_eq!(
        health.status().repair,
        RepairState::DamageRecurring,
        "and then it stops again"
    );
}

#[test]
fn failed_repairs_do_not_count_as_recurring_damage() {
    // A read-only index directory (or a full disk, or a peer holding the
    // lock) fails every attempt; that is not damage coming back, and the
    // advice to move the index would be wrong for it.
    use std::os::unix::fs::PermissionsExt;
    let dir = vault_with_notes();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let health = IndexHealth::from_opened(&opened).with_policy(eager());
    damage_live(&opened);
    let meta_dir = dir.path().join(paths::CUADERNO_DIR);
    std::fs::set_permissions(&meta_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    for _ in 0..5 {
        health.sweep();
    }
    let status = health.status();
    std::fs::set_permissions(&meta_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    let rebuild = status.last_rebuild.expect("attempts were made");
    assert!(!rebuild.rebuilt, "the directory refused the fresh file");
    assert_eq!(status.damage_rebuilds_recent, 0);
    assert_eq!(status.repair, RepairState::Automatic, "{:?}", status.advice);

    health.sweep();
    assert!(
        health.status().last_rebuild.is_some_and(|r| r.rebuilt),
        "once the cause is gone the next sweep repairs"
    );
}

#[test]
fn a_stop_ends_by_itself_when_the_index_checks_sound_again() {
    // Another process repaired the file, or the writer sharing it went
    // away: no person needs to call `rebuild_index`.
    let dir = vault_with_notes();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let health = IndexHealth::from_opened(&opened).with_policy(RepairPolicy {
        recurrence_limit: 1,
        ..eager()
    });
    damage_live(&opened);
    health.sweep();
    damage_live(&opened);
    health.sweep();
    assert_eq!(health.status().repair, RepairState::DamageRecurring);

    // The repair another process makes, through the same file.
    cdno_core::reconcile::rebuild(&opened.store, &opened.sqlite, &opened.scope, false).unwrap();
    health.sweep();

    let status = health.status();
    assert_eq!(status.repair, RepairState::Automatic);
    assert!(status.healthy);
}

#[test]
fn damage_found_at_open_counts_as_the_first_repair_of_a_streak() {
    let dir = vault_with_notes();
    damage_index(dir.path());
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let previous = beside(&index_path(dir.path()), cdno_core::index::PREVIOUS_SUFFIX);
    let at_open = std::fs::read(&previous).expect("the open kept a copy");
    let health = IndexHealth::from_opened(&opened).with_policy(eager());
    assert_eq!(health.status().damage_rebuilds_recent, 1);

    damage_live(&opened);
    health.sweep();

    assert!(health.status().last_rebuild.is_some_and(|r| r.rebuilt));
    assert_eq!(
        std::fs::read(&previous).unwrap(),
        at_open,
        "the copy from the open is the one kept"
    );
}
// ---- An overridden index (#998) -------------------------------------------

/// A vault plus a directory outside it holding its overridden index, built
/// by a first open and closed again.
fn vault_with_overridden_index() -> (TempDir, TempDir, std::path::PathBuf) {
    let dir = vault_with_notes();
    let elsewhere = TempDir::new().expect("tempdir");
    let index = elsewhere.path().join("surrogate-model.db");
    drop(
        open_vault_with_index(dir.path(), Some(index.as_os_str()))
            .expect("first open builds the overridden index"),
    );
    (dir, elsewhere, index)
}

/// `path` with `suffix` appended to its file name.
fn beside(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

/// No index file of any kind under `.cuaderno/`: an overridden index's
/// repair writes only beside the override.
fn assert_nothing_of_the_index_under_cuaderno(root: &Path) {
    let names: Vec<String> = std::fs::read_dir(root.join(paths::CUADERNO_DIR))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !names.iter().any(|n| n.starts_with("index.db")),
        "an index file was created under .cuaderno/: {names:?}"
    );
}

fn canonical(root: &Path) -> String {
    cdno_core::index::canonical_vault_root(root)
}

#[test]
fn a_checked_open_repairs_an_overridden_index_in_place_and_keeps_its_binding() {
    let (dir, _elsewhere, index) = vault_with_overridden_index();
    damage_last_page(&index);

    let opened = open_vault_checked(dir.path(), Some(index.as_os_str()))
        .expect("the checked open repairs the overridden index");

    assert!(opened.damage_at_open.is_some(), "the damage is reported");
    assert_eq!(opened.index_path, index);
    assert_eq!(opened.sqlite.path(), index.as_path());
    assert!(opened.sqlite.quick_check().unwrap().is_empty());
    assert!(indexed_notes(&opened) >= NOTES);
    assert!(
        beside(&index, cdno_core::index::PREVIOUS_SUFFIX).exists(),
        "the copy of the damaged file sits beside the override"
    );
    assert_nothing_of_the_index_under_cuaderno(dir.path());
    drop(opened);

    // The repaired file is still cuaderno's, still bound to this vault, so
    // the next open, checked or not, takes it as it is.
    cdno_core::index::SqliteIndex::ensure_discardable(&index).expect("still cuaderno's index");
    assert_eq!(
        cdno_core::index::SqliteIndex::bound_vault_root(&index).unwrap(),
        Some(canonical(dir.path()))
    );
    let again = open_vault_checked(dir.path(), Some(index.as_os_str())).unwrap();
    assert!(again.damage_at_open.is_none());
    assert_nothing_of_the_index_under_cuaderno(dir.path());
}

#[test]
fn a_sweep_repairs_an_overridden_index_behind_the_live_handle() {
    let (dir, _elsewhere, index) = vault_with_overridden_index();
    damage_last_page(&index);
    let opened = open_vault_with_index(dir.path(), Some(index.as_os_str())).unwrap();
    let health = IndexHealth::from_opened(&opened);

    health.sweep();

    let status = health.status();
    assert!(status.check.ok, "{:?}", status.check.problems);
    assert_eq!(status.index_path, index.display().to_string());
    let rebuild = status.last_rebuild.expect("the sweep rebuilt the index");
    assert!(rebuild.rebuilt, "{:?}", rebuild.failure);
    assert!(indexed_notes(&opened) >= NOTES);
    assert_eq!(
        cdno_core::index::SqliteIndex::bound_vault_root(&index).unwrap(),
        Some(canonical(dir.path())),
        "the rebuilt index is bound to this vault"
    );
    assert_nothing_of_the_index_under_cuaderno(dir.path());
}

#[test]
fn index_status_names_the_default_index_file() {
    let dir = vault_with_notes();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let status = IndexHealth::from_opened(&opened).status();
    assert_eq!(
        status.index_path,
        index_path(dir.path()).display().to_string()
    );
}

#[test]
fn a_checked_open_refuses_a_file_that_is_not_an_index_and_leaves_it_alone() {
    let dir = vault_with_notes();
    let elsewhere = TempDir::new().unwrap();
    let foreign = elsewhere.path().join("other.db");
    {
        let conn = rusqlite::Connection::open(&foreign).unwrap();
        conn.execute_batch(
            "CREATE TABLE orders (id INTEGER PRIMARY KEY); INSERT INTO orders VALUES (1);",
        )
        .unwrap();
    }
    let before = std::fs::read(&foreign).unwrap();

    let err = open_vault_checked(dir.path(), Some(foreign.as_os_str()))
        .err()
        .expect("refused");

    assert!(
        format!("{err:#}").contains("not a cuaderno index"),
        "{err:#}"
    );
    assert_eq!(
        std::fs::read(&foreign).unwrap(),
        before,
        "not one byte changed"
    );
    let names: Vec<_> = std::fs::read_dir(elsewhere.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        names,
        vec![std::ffi::OsString::from("other.db")],
        "nothing created beside it"
    );
}

#[test]
fn a_checked_open_refuses_a_damaged_index_bound_to_another_vault() {
    let (owner, _elsewhere, index) = vault_with_overridden_index();
    damage_last_page(&index);
    assert_eq!(
        cdno_core::index::SqliteIndex::bound_vault_root(&index).unwrap(),
        Some(canonical(owner.path())),
        "precondition: the damage spares the binding"
    );
    let before = std::fs::read(&index).unwrap();
    let other = vault_with_notes();

    let err = open_vault_checked(other.path(), Some(index.as_os_str()))
        .err()
        .expect("refused");

    assert!(
        format!("{err:#}").contains("belongs to the vault at"),
        "{err:#}"
    );
    assert_eq!(std::fs::read(&index).unwrap(), before, "not repaired");
    assert!(
        !beside(&index, cdno_core::index::PREVIOUS_SUFFIX).exists(),
        "not copied or set aside"
    );
}

#[test]
fn a_rebuild_never_takes_back_an_index_another_vault_has_bound_since() {
    // Something rebound the file behind this server's back (`cdno reindex
    // --rebind` from another vault, say): a rebuild must not take it back.
    let (dir, _elsewhere, index) = vault_with_overridden_index();
    let opened = open_vault_with_index(dir.path(), Some(index.as_os_str())).unwrap();
    {
        let conn = rusqlite::Connection::open(&index).unwrap();
        conn.execute(
            "UPDATE index_meta SET value = '/elsewhere/vault' WHERE key = 'vault_root'",
            [],
        )
        .unwrap();
    }

    let record = IndexHealth::from_opened(&opened).rebuild("test").unwrap();

    assert!(!record.rebuilt);
    assert!(
        record
            .failure
            .as_deref()
            .unwrap_or("")
            .contains("belongs to the vault at"),
        "{:?}",
        record.failure
    );
    assert_eq!(
        cdno_core::index::SqliteIndex::bound_vault_root(&index).unwrap(),
        Some("/elsewhere/vault".to_owned())
    );
}

/// The stdio server's reading of the sweep interval (#1002): unset, empty
/// or blank is the HTTP server's default, a number (spaces around it
/// ignored) is taken as seconds, `0` included, and anything else is
/// refused, naming the variable.
#[test]
fn the_stdio_sweep_interval_variable_parses() {
    use cdno_mcp::index_health::{
        DEFAULT_SWEEP_INTERVAL_SECS, SWEEP_INTERVAL_ENV, parse_sweep_interval,
    };
    use std::ffi::OsStr;

    assert_eq!(DEFAULT_SWEEP_INTERVAL_SECS, 300);
    assert_eq!(SWEEP_INTERVAL_ENV, "CDNO_MCP_RECONCILE_INTERVAL_SECS");
    assert_eq!(parse_sweep_interval(None), Ok(300));
    assert_eq!(parse_sweep_interval(Some(OsStr::new(""))), Ok(300));
    assert_eq!(parse_sweep_interval(Some(OsStr::new("   "))), Ok(300));
    assert_eq!(parse_sweep_interval(Some(OsStr::new("0"))), Ok(0));
    assert_eq!(parse_sweep_interval(Some(OsStr::new(" 60 "))), Ok(60));
    for bad in ["5m", "-1", "1.5", "sixty"] {
        let err = parse_sweep_interval(Some(OsStr::new(bad))).unwrap_err();
        assert!(err.contains(SWEEP_INTERVAL_ENV), "{err}");
    }
}

/// The first sweep's jitter stays between half an interval and one, so a
/// server never sweeps later than it did before the jitter.
#[test]
fn the_first_sweep_comes_between_half_an_interval_and_one() {
    use cdno_mcp::index_health::first_sweep_delay;
    use std::time::Duration;

    let every = Duration::from_secs(300);
    for random in [0, 1, 149_999, 150_000, u64::MAX] {
        let delay = first_sweep_delay(every, random);
        assert!(delay >= every / 2 && delay < every, "{delay:?}");
    }
    assert_eq!(first_sweep_delay(every, 0), every / 2);
    assert_eq!(
        first_sweep_delay(Duration::from_millis(1), 7),
        Duration::from_millis(1)
    );
}

/// An interval of `u64::MAX` seconds, which a configuration may give to
/// mean "never", neither panics in the jitter nor ends the loop task (and
/// the corruption listener with it): the deadlines are capped, not added
/// past what an `Instant` can hold.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_huge_sweep_interval_keeps_the_loop_alive() {
    use cdno_mcp::index_health::{MAX_SWEEP_INTERVAL, first_sweep_delay};
    use std::time::Duration;

    let huge = Duration::from_secs(u64::MAX);
    for random in [0, 7, u64::MAX] {
        assert!(first_sweep_delay(huge, random) >= huge / 2);
        let capped = first_sweep_delay(MAX_SWEEP_INTERVAL, random);
        assert!(capped >= MAX_SWEEP_INTERVAL / 2 && capped < MAX_SWEEP_INTERVAL);
    }

    let dir = TempDir::new().expect("tempdir");
    cdno_cli::commands::init::run(dir.path()).expect("cdno init");
    let opened = open_vault_checked(dir.path(), None).expect("open");
    let health = Arc::new(IndexHealth::from_opened(&opened));
    let started = health.start(u64::MAX);
    let spawned = health.spawn_loop(Some(huge));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!started.is_finished(), "the started loop ended");
    assert!(!spawned.is_finished(), "the spawned loop ended");
    started.abort();
    spawned.abort();
}

/// The tool-call count the stdio server drains at shutdown (#1002): a call
/// is counted while its closure runs, still counted when its caller stops
/// awaiting it, and `idle` returns once every closure has returned. The
/// limit holds the others back, counted, until a turn frees.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_calls_are_counted_until_their_work_returns() {
    use cdno_mcp::tool_calls::ToolCalls;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    let calls = Arc::new(ToolCalls::with_limit(1));
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
    let done = Arc::new(AtomicUsize::new(0));

    let first = {
        let (calls, done) = (Arc::clone(&calls), Arc::clone(&done));
        tokio::spawn(async move {
            calls
                .run(move || {
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    done.fetch_add(1, Ordering::SeqCst);
                })
                .await
        })
    };
    tokio::task::spawn_blocking(move || started_rx.recv_timeout(Duration::from_secs(5)))
        .await
        .unwrap()
        .expect("the first call's work started");
    let second = {
        let (calls, done) = (Arc::clone(&calls), Arc::clone(&done));
        tokio::spawn(async move {
            calls
                .run(move || {
                    done.fetch_add(1, Ordering::SeqCst);
                })
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while calls.in_flight() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both calls counted");
    // The caller of the running one gives up; its work is still counted.
    first.abort();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        done.load(Ordering::SeqCst),
        0,
        "the limit held the second back"
    );
    assert!(calls.in_flight() >= 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(200), calls.idle())
            .await
            .is_err(),
        "idle while work runs"
    );

    release_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), calls.idle())
        .await
        .expect("idle once the work has returned");
    assert_eq!(done.load(Ordering::SeqCst), 2);
    assert_eq!(calls.in_flight(), 0);
    second.await.unwrap().unwrap();
}
