//! In-process tests for `commands::lint::run`.

use std::fs;

use cdno_cli::commands::{init, lint};
use tempfile::tempdir;

#[test]
fn lint_succeeds_silently_on_a_freshly_inited_vault() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    // Post-#87 the index is empty after init (templates under
    // `.cuaderno/` are excluded from the scan), so lint finds nothing.
    lint::run(dir.path()).expect("lint should succeed on empty vault");
}

#[test]
fn lint_returns_err_when_a_note_has_an_unknown_type() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    fs::write(
        dir.path().join("strange.md"),
        "---\ntype: bogus\ntitle: Mystery\n---\n# Body\n",
    )
    .unwrap();

    let err = lint::run(dir.path()).expect_err("lint should fail");
    let msg = format!("{err}");
    assert!(msg.contains("1 lint issue"), "unexpected error: {msg}");
}

#[test]
fn lint_errors_when_target_is_not_a_vault() {
    let dir = tempdir().unwrap();

    let err = lint::run(dir.path()).expect_err("lint without vault must fail");
    let msg = format!("{err}");
    assert!(msg.contains("no Cuaderno vault"), "unexpected error: {msg}");
}
