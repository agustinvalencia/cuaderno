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
    lint::run(dir.path(), false).expect("lint should succeed on empty vault");
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

    let err = lint::run(dir.path(), false).expect_err("lint should fail");
    let msg = format!("{err}");
    assert!(msg.contains("1 error(s)"), "unexpected error: {msg}");
}

#[test]
fn lint_warns_on_frontmatter_order_drift_and_strict_makes_it_fatal() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    // Canonical daily order is `type` then `date`; this note reverses
    // them, so it drifts. (Frontmatter is valid, so it's a warning.)
    let daily = dir.path().join("journal/2026/daily/2026-04-19.md");
    fs::create_dir_all(daily.parent().unwrap()).unwrap();
    fs::write(&daily, "---\ndate: 2026-04-19\ntype: daily\n---\n# Note\n").unwrap();

    // Non-strict: a warning is non-fatal, so lint still succeeds.
    lint::run(dir.path(), false).expect("order drift is non-fatal without --strict");

    // --strict: the warning becomes a failure.
    let err = lint::run(dir.path(), true).expect_err("strict lint should fail on the drift");
    assert!(
        format!("{err}").contains("1 warning(s)"),
        "unexpected error: {err}"
    );
}

#[test]
fn lint_errors_when_target_is_not_a_vault() {
    let dir = tempdir().unwrap();

    let err = lint::run(dir.path(), false).expect_err("lint without vault must fail");
    let msg = format!("{err}");
    assert!(msg.contains("no Cuaderno vault"), "unexpected error: {msg}");
}

#[test]
fn lint_warns_on_malformed_stewardship_dashboard_line_and_strict_makes_it_fatal() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    // A stewardship whose Periodic Commitments bullet omits the `next:`
    // marker. The canonical parser rejects it, so the new dashboard rule
    // warns — proving it flows through the CLI surface with no per-rule
    // wiring (the frontmatter is in canonical order and there are no other
    // sections, so this is the only issue in the report).
    let steward = dir.path().join("stewardships/health.md");
    fs::create_dir_all(steward.parent().unwrap()).unwrap();
    fs::write(
        &steward,
        "---\ntype: stewardship\ncontext: personal\n---\n\n# Health\n\n## Periodic Commitments\n- Dental check-up \u{2014} every 6 months \u{2014} next 2026-04-01\n",
    )
    .unwrap();

    // Non-strict: a warning is non-fatal, so lint still succeeds.
    lint::run(dir.path(), false).expect("dashboard warning is non-fatal without --strict");

    // --strict: the warning becomes a failure.
    let err =
        lint::run(dir.path(), true).expect_err("strict lint should fail on the dashboard warning");
    assert!(
        format!("{err}").contains("1 warning(s)"),
        "unexpected error: {err}"
    );
}
