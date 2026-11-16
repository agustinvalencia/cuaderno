//! Recurring index damage (#526 review): once the server stops repairing
//! by itself, it says so once in the log and on every client error.
//!
//! A target of its own, with one test, because the hint a client error
//! carries is process-wide (errors are converted with no server at hand),
//! so another test's repairs in the same process would change it under
//! this one.

use std::sync::{Arc, Mutex};

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
fn recurring_damage_is_said_once_in_the_log_and_on_every_client_error() {
    let dir = TempDir::new().unwrap();
    cdno_cli::commands::init::run(dir.path()).expect("cdno init");
    let opened = open_vault_checked(dir.path(), None).unwrap();
    let health = IndexHealth::from_opened(&opened).with_policy(RepairPolicy {
        min_rebuild_interval: std::time::Duration::ZERO,
        recurrence_limit: 2,
        ..RepairPolicy::default()
    });
    let damage = || {
        opened
            .sqlite
            .with_connection(|c| c.execute_batch("DROP TABLE IF EXISTS schema_migrations"))
            .unwrap();
    };
    assert!(damage_hint().contains("rebuilds it if it is"));

    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        // Two repairs, then three more rounds of damage that are reported.
        for _ in 0..5 {
            damage();
            health.sweep();
        }
    });

    let status = health.status();
    assert_eq!(status.repair, RepairState::DamageRecurring);
    let logged = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let stops = logged
        .lines()
        .filter(|l| l.contains("automatic index repair stopped"))
        .collect::<Vec<_>>();
    assert_eq!(stops.len(), 1, "one error, not one per round:\n{logged}");
    assert!(stops[0].contains("ERROR"), "{}", stops[0]);
    assert!(stops[0].contains("CUADERNO_INDEX_PATH"), "{}", stops[0]);
    // Two repairs, then the round that decided the stop; the two rounds
    // after it say nothing about rebuilding.
    let rebuilding = logged
        .lines()
        .filter(|l| l.contains("rebuilding it from the notes"))
        .count();
    assert_eq!(
        rebuilding, 3,
        "no rebuild announced while stopped:\n{logged}"
    );

    let hint = damage_hint();
    assert!(hint.contains("stopped repairing it by itself"), "{hint}");
    assert!(hint.contains("its own index"), "{hint}");
    assert!(!hint.contains("rebuilds it if it is"), "{hint}");

    // While the server does not hold its files, `index_status` runs no
    // check on its own copy, so a pass there cannot end the stop.
    cdno_core::index::SqliteIndex::set_aside(&cdno_core::paths::IndexLocation {
        path: opened.index_path.clone(),
        overridden: false,
    })
    .unwrap();
    let unheld = health.status();
    assert!(!unheld.files.held);
    assert!(
        unheld.check.problems[0].starts_with("not checked"),
        "{unheld:?}"
    );
    assert_eq!(unheld.repair, RepairState::DamageRecurring);
    assert!(unheld.damage_repair_stopped);

    // The person's `rebuild_index` reopens, ends the stop, and the hint
    // follows.
    assert!(health.rebuild("rebuild_index was called").unwrap().rebuilt);
    assert!(damage_hint().contains("rebuilds it if it is"));
}
