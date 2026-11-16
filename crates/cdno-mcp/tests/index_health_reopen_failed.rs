//! A reopen that keeps failing for a reason other than damage (#1000): an
//! overridden index file that another vault has taken (`cdno reindex
//! --rebind` from there). It is said once per cause, retried at the
//! spacing, kept in `index_status`, and it refuses `rebuild_index`.
//!
//! A target of its own, with one test, because the log is captured with a
//! thread-local subscriber, which other tests' threads in the same process
//! can keep from seeing a callsite.

use std::sync::{Arc, Mutex};

use cdno_core::index::SqliteIndex;
use cdno_core::paths::IndexLocation;
use cdno_mcp::bootstrap::open_vault_checked;
use cdno_mcp::index_health::{IndexHealth, RepairPolicy};
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
fn a_reopen_that_keeps_failing_is_said_once_retried_and_refuses_a_rebuild() {
    let vault = TempDir::new().unwrap();
    cdno_cli::commands::init::run(vault.path()).expect("cdno init");
    let elsewhere = TempDir::new().unwrap();
    let index = elsewhere.path().join("surrogate-model.db");
    let opened = open_vault_checked(vault.path(), Some(index.as_os_str())).unwrap();
    let other_vault = TempDir::new().unwrap();
    cdno_cli::commands::init::run(other_vault.path()).expect("cdno init");
    SqliteIndex::remove(&index);
    drop(
        SqliteIndex::open_location(
            &IndexLocation {
                path: index.clone(),
                overridden: true,
            },
            other_vault.path(),
        )
        .unwrap(),
    );
    let health = IndexHealth::from_opened(&opened).with_policy(RepairPolicy {
        min_reopen_interval: std::time::Duration::ZERO,
        ..RepairPolicy::default()
    });

    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        for _ in 0..3 {
            health.sweep();
        }
    });

    let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    let errors = log
        .lines()
        .filter(|l| l.contains("reopening them failed"))
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1, "one ERROR per cause:\n{log}");
    assert!(errors[0].contains("ERROR"), "{}", errors[0]);
    let status = health.status();
    let reopen = status.last_reopen.clone().unwrap();
    assert!(!reopen.reopened);
    assert!(
        reopen
            .failure
            .as_deref()
            .unwrap_or("")
            .contains("belongs to the vault"),
        "{reopen:?}"
    );
    assert!(!status.files.held);
    assert!(!status.healthy);
    assert_eq!(status.reopens_recent, 0, "failed reopens are not counted");
    let sweep = status.last_sweep.unwrap();
    assert!(
        sweep
            .failure
            .as_deref()
            .unwrap_or("")
            .starts_with("skipped"),
        "nothing reconciles into the copy: {sweep:?}"
    );

    let refusal = health.rebuild("rebuild_index was called").unwrap_err();
    assert_eq!(refusal.refused.code(), "files_not_held", "{refusal}");
    assert!(
        refusal.to_string().contains("reopening it failed"),
        "{refusal}"
    );
}
