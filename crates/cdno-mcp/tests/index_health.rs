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
use cdno_mcp::index_health::{IndexHealth, RebuildRefused};
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
    // The final pass runs under the vault write lock, so nothing can land
    // between it and the copy (second review round on #997: 119 of 984
    // captures were missing without it).
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = vault_with_notes();
    let opened = Arc::new(open_vault_checked(dir.path(), None).unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let (opened, stop) = (Arc::clone(&opened), Arc::clone(&stop));
        std::thread::spawn(move || {
            let at = NaiveDate::from_ymd_opt(2026, 3, 1)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap();
            let mut written = 0usize;
            while !stop.load(Ordering::Relaxed) {
                opened
                    .vault
                    .capture_to_inbox(at, &format!("surrogate model idea {written}"))
                    .expect("capture");
                written += 1;
            }
            written
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(100));
    let record = IndexHealth::from_opened(&opened).rebuild("test").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    stop.store(true, Ordering::Relaxed);
    let written = writer.join().unwrap();

    assert!(record.rebuilt, "{:?}", record.failure);
    assert!(written > 0);
    let on_disk = std::fs::read_dir(dir.path().join("inbox")).unwrap().count();
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
fn damage_to_the_file_header_asks_for_a_restart() {
    // The in-place copy cannot write over a header SQLite no longer
    // recognises; a running server must say so rather than retry forever.
    let dir = vault_with_notes();
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let path = index_path(dir.path());
    let mut bytes = std::fs::read(&path).unwrap();
    for b in &mut bytes[..100] {
        *b = 0xA5;
    }
    std::fs::write(&path, bytes).unwrap();
    let health = IndexHealth::from_opened(&opened);

    let record = health.rebuild("test").unwrap();

    if record.rebuilt {
        // SQLite may still hold the header in its cache and write it back;
        // then the repair simply worked.
        assert!(opened.sqlite.quick_check().unwrap().is_empty());
    } else {
        assert!(record.restart_required, "{:?}", record.failure);
        assert!(health.status().restart_required);
    }
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
