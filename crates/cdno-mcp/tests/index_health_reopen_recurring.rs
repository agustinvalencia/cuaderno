//! Recurring reopens (#1000): files that keep being deleted under the
//! server mean something else is using the index file. Each reopen is a
//! WARN; past the limit the server says so once and reports
//! `reopen_recurring`, but keeps reopening every time, and the report ends
//! by itself once the reopens stop recurring over the window.
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
fn recurring_reopens_are_reported_once_never_stop_reopening_and_end_by_themselves() {
    let dir = TempDir::new().unwrap();
    cdno_cli::commands::init::run(dir.path()).expect("cdno init");
    let opened = open_vault_checked(dir.path(), None).unwrap();
    // Room for slow, instrumented runs (coverage): four sweeps must land
    // inside it.
    let window = std::time::Duration::from_secs(8);
    let health = IndexHealth::from_opened(&opened).with_policy(RepairPolicy {
        min_rebuild_interval: std::time::Duration::ZERO,
        min_reopen_interval: std::time::Duration::ZERO,
        recurrence_window: window,
        recurrence_limit: 1,
        reopen_limit: 2,
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
    let (during, after) = tracing::subscriber::with_default(subscriber, || {
        for _ in 0..4 {
            unlink_sidecars();
            health.sweep();
        }
        let during = health.status();
        std::thread::sleep(window + std::time::Duration::from_millis(200));
        (during, health.status())
    });

    // Every round reopened: the server never settles on the orphan.
    assert!(during.files.held, "{:?}", during.files);
    assert_eq!(during.reopens_recent, 4);
    assert_eq!(during.repair, RepairState::ReopenRecurring);
    assert!(!during.healthy);
    assert!(
        during
            .advice
            .as_deref()
            .unwrap_or("")
            .contains("CUADERNO_INDEX_PATH"),
        "vanished sidecars are the cross-kernel signature: {:?}",
        during.advice
    );
    assert_eq!(
        during.damage_rebuilds_recent, 0,
        "reopens are not damage, even with a recurrence limit of one"
    );
    let logged = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let reopened = logged
        .lines()
        .filter(|l| l.contains("reopened the index on the file"))
        .collect::<Vec<_>>();
    assert_eq!(reopened.len(), 4, "one WARN per reopen:\n{logged}");
    assert!(reopened.iter().all(|l| l.contains("WARN")), "{reopened:?}");
    assert!(reopened[0].contains("-wal"), "it says what it found");
    assert!(reopened[0].contains("done="), "it says what it did");
    let recurring = logged
        .lines()
        .filter(|l| l.contains("keep changing under this server"))
        .collect::<Vec<_>>();
    assert_eq!(
        recurring.len(),
        1,
        "one error, not one per round:\n{logged}"
    );
    assert!(recurring[0].contains("ERROR"), "{}", recurring[0]);
    assert!(
        recurring[0].contains("CUADERNO_INDEX_PATH"),
        "{}",
        recurring[0]
    );

    // The window drained: the report ends by itself, and is said to.
    assert_eq!(after.repair, RepairState::Automatic, "{after:?}");
    assert!(after.healthy, "{after:?}");
    assert!(
        logged.contains("stopped changing under this server"),
        "{logged}"
    );
    assert!(damage_hint().contains("rebuilds it if it is"));
}
