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
use cdno_mcp::bootstrap::{open_vault, open_vault_checked};
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
    drop(open_vault(root).expect("first open indexes the vault"));
    let path = index_path(root);
    let page_size: u64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("PRAGMA page_size", [], |r| r.get(0))
        .unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let start = bytes.len() - page_size as usize;
    for b in &mut bytes[start..] {
        *b = 0xA5;
    }
    std::fs::write(&path, bytes).unwrap();
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
fn a_checked_open_sets_a_damaged_index_aside_and_rebuilds_it() {
    let dir = vault_with_notes();
    damage_index(dir.path());

    let opened = open_vault_checked(dir.path()).expect("the checked open recovers");

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
    drop(open_vault(dir.path()).unwrap());

    let opened = open_vault_checked(dir.path()).unwrap();

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
    let opened = open_vault(dir.path()).unwrap();
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
    let opened = open_vault_checked(dir.path()).unwrap();
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
    let opened = open_vault_checked(dir.path()).unwrap();
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
    let opened = open_vault_checked(dir.path()).unwrap();
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
    let opened = open_vault(dir.path()).unwrap();
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
    let opened = open_vault_checked(dir.path()).unwrap();
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
