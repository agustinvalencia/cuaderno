//! The exit statuses `cdno` gives besides the generic 1.
//!
//! The binary's `main` exits with [`cdno_cli::exit::special_status`] when
//! it is `Some`; these tests pin what it answers. A concurrent change
//! cannot be provoked deterministically in a subprocess, so the wiring in
//! `main` is not exercised here.

use anyhow::Context;
use cdno_cli::exit::{CONCURRENT_CHANGE, special_status};
use cdno_core::error::{StoreError, TransactionError};
use cdno_core::path::VaultPath;
use cdno_domain::error::DomainError;

fn conflict() -> DomainError {
    DomainError::Transaction(TransactionError::Conflict {
        paths: vec![VaultPath::new("journal/2026/daily/2026-06-13.md").unwrap()],
    })
}

#[test]
fn a_concurrent_change_exits_with_tempfail_even_under_context() {
    assert_eq!(
        CONCURRENT_CHANGE, 75,
        "EX_TEMPFAIL, a script's cue to retry"
    );
    let bare = anyhow::Error::from(conflict());
    assert_eq!(special_status(&bare), Some(75));

    // Handlers wrap the domain error in context, as `cdno log` does.
    let wrapped = Err::<(), _>(conflict())
        .context("appending log entry to daily note")
        .unwrap_err();
    assert_eq!(special_status(&wrapped), Some(75));
    let report = format!("{wrapped:?}");
    assert!(report.contains("appending log entry"), "{report}");
    assert!(report.contains("nothing was written"), "{report}");
    assert!(report.contains("run the operation again"), "{report}");
}

#[test]
fn every_other_failure_keeps_the_generic_status() {
    let others = [
        anyhow::Error::from(DomainError::Store(StoreError::Changed(cdno_core::path::VaultPath::new("a.md").unwrap()))),
        anyhow::Error::from(DomainError::Transaction(TransactionError::FileWrite {
            source: StoreError::Changed(cdno_core::path::VaultPath::new("a.md").unwrap()),
            rollback_failures: vec![StoreError::Changed(cdno_core::path::VaultPath::new("b.md").unwrap())],
        })),
        anyhow::Error::from(DomainError::Transaction(TransactionError::IndexStale(
            vec![],
        ))),
        anyhow::anyhow!("not inside a Cuaderno vault"),
    ];
    for err in &others {
        assert_eq!(special_status(err), None, "{err:?}");
    }
}
