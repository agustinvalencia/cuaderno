//! Recurring reopens (#1000): files that keep being deleted under the
//! server mean something else manages the index file. Each reopen is a
//! WARN; past the limit the server stops reopening, says so once, and
//! reports it on every client error until a `rebuild_index`.
//!
//! A target of its own, with one test, because the hint a client error
//! carries is process-wide.

use std::sync::{Arc, Mutex};

use cdno_core::paths;
use cdno_mcp::bootstrap::open_vault_checked;
use cdno_mcp::index_health::{IndexHealth, RepairPolicy, RepairState, damage_hint};
use tempfile::TempDir;

/// A `tracing` writer that keeps what it is given.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn repeated_reopens_are_bounded_said_once_and_not_counted_as_damage() {
    let dir = TempDir::new().unwrap();
    cdno_cli::commands::init::run(dir.path()).expect("cdno init");
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let health = IndexHealth::from_opened(&opened).with_policy(RepairPolicy {
        min_rebuild_interval: std::time::Duration::ZERO,
        recurrence_limit: 1,
        reopen_limit: 2,
        ..RepairPolicy::default()
    });
    let path = dir.path().join(paths::INDEX_DB);
    let unlink_sidecars = || {
        for suffix in ["-wal", "-shm"] {
            let mut name = path.clone().into_os_string();
            name.push(suffix);
            std::fs::remove_file(name).unwrap();
        }
    };
    assert!(damage_hint().contains("rebuilds it if it is"));

    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        // Two reopens, then the round that stops, then one more that is
        // only reported.
        for round in 0..4 {
            if round < 3 {
                unlink_sidecars();
            }
            health.sweep();
        }
    });

    let status = health.status();
    assert_eq!(status.repair, RepairState::ReopenRecurring);
    assert!(!status.healthy);
    assert!(!status.files.held);
    assert_eq!(status.reopens_recent, 2);
    assert_eq!(
        status.damage_rebuilds_recent, 0,
        "reopens are not damage, even with a recurrence limit of one"
    );
    assert!(status.last_rebuild.is_none());
    assert!(
        status
            .advice
            .as_deref()
            .unwrap_or("")
            .contains("CUADERNO_INDEX_PATH"),
        "{:?}",
        status.advice
    );
    let logged = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let reopened = logged
        .lines()
        .filter(|l| l.contains("reopened the index"))
        .collect::<Vec<_>>();
    assert_eq!(reopened.len(), 2, "one WARN per reopen:\n{logged}");
    assert!(reopened.iter().all(|l| l.contains("WARN")), "{reopened:?}");
    assert!(reopened[0].contains("-wal"), "it says what it found");
    let stops = logged
        .lines()
        .filter(|l| l.contains("automatic index reopening stopped"))
        .collect::<Vec<_>>();
    assert_eq!(stops.len(), 1, "one error, not one per round:\n{logged}");
    assert!(stops[0].contains("ERROR"), "{}", stops[0]);
    assert!(stops[0].contains("CUADERNO_INDEX_PATH"), "{}", stops[0]);

    let hint = damage_hint();
    assert!(hint.contains("stopped reopening"), "{hint}");
    assert!(hint.contains("its own index"), "{hint}");

    // The person's `rebuild_index` reopens, rebuilds and ends the stop.
    let record = health.rebuild("rebuild_index was called").unwrap();
    assert!(record.rebuilt, "{:?}", record.failure);
    let status = health.status();
    assert_eq!(status.repair, RepairState::Automatic);
    assert!(status.files.held);
    assert!(status.healthy, "{status:?}");
    assert!(damage_hint().contains("rebuilds it if it is"));
}
