//! The exit status and `--json` object of a write refused because its note
//! changed underneath it (#1001).
//!
//! The binary's `main` exits with [`cdno_cli::exit::special_status`] when
//! it is `Some`, printing [`cdno_cli::exit::rejection`] under `--json`;
//! these tests pin what they answer. A concurrent change cannot be
//! provoked deterministically in a subprocess, so the wiring in `main` is
//! not exercised here.

use anyhow::Context;
use cdno_cli::exit::{CONCURRENT_CHANGE, rejection, special_status};
use cdno_core::error::{StoreError, TransactionError};
use cdno_core::path::VaultPath;
use cdno_domain::error::DomainError;
use serde_json::json;

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

fn conflict() -> DomainError {
    DomainError::Transaction(TransactionError::Conflict {
        paths: vec![vp("journal/2026/daily/2026-06-13.md")],
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
fn under_json_a_concurrent_change_is_the_rejection_object_mcp_returns() {
    let wrapped = Err::<(), _>(conflict())
        .context("appending log entry to daily note")
        .unwrap_err();
    let object = rejection(&wrapped).expect("a concurrent change has an object");
    assert_eq!(object["code"], "concurrent_change");
    assert_eq!(
        object["details"],
        json!({ "paths": ["journal/2026/daily/2026-06-13.md"] })
    );
    assert!(
        object["message"]
            .as_str()
            .unwrap()
            .contains("nothing was written"),
        "{object}"
    );
}

#[test]
fn every_other_failure_keeps_the_generic_status_and_no_object() {
    let others = [
        anyhow::Error::from(DomainError::Store(StoreError::Changed(vp("a.md")))),
        anyhow::Error::from(DomainError::Transaction(TransactionError::FileWrite {
            source: StoreError::Changed(vp("a.md")),
            rollback_failures: vec![StoreError::Changed(vp("b.md"))],
        })),
        anyhow::Error::from(DomainError::Transaction(TransactionError::IndexStale(
            vec![],
        ))),
        anyhow::anyhow!("not inside a Cuaderno vault"),
    ];
    for err in &others {
        assert_eq!(special_status(err), None, "{err:?}");
        assert!(rejection(err).is_none(), "{err:?}");
    }
}
