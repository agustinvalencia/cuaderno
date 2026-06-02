//! In-process tests for `bootstrap::{discover_vault_root, open_vault}`.
//!
//! These tests construct a *minimal* vault on disk (just an empty
//! `.cuaderno/` directory) rather than calling `init::run`. The point
//! is to test bootstrap behaviour in isolation — coupling these
//! tests to init's side effects (template dump, full directory tree)
//! would mean any change to init's defaults silently breaks bootstrap
//! tests.

use std::fs;
use std::path::Path;

use cdno_cli::bootstrap;
use cdno_core::paths;
use tempfile::tempdir;

/// Lay down only what `bootstrap::open_vault` needs to succeed:
/// the `.cuaderno/` marker directory. The SQLite index file is
/// created on demand by `SqliteIndex::open`.
fn make_minimal_vault(root: &Path) {
    fs::create_dir(root.join(paths::CUADERNO_DIR)).expect("create .cuaderno");
}

#[test]
fn discover_vault_root_finds_marker_at_start_path() {
    let dir = tempdir().unwrap();
    make_minimal_vault(dir.path());

    let found = bootstrap::discover_vault_root(dir.path());

    assert_eq!(found.as_deref(), Some(dir.path()));
}

#[test]
fn discover_vault_root_walks_ancestors_to_find_marker() {
    let dir = tempdir().unwrap();
    make_minimal_vault(dir.path());
    let nested = dir.path().join("a").join("b").join("c");
    fs::create_dir_all(&nested).unwrap();

    let found = bootstrap::discover_vault_root(&nested);

    assert_eq!(found.as_deref(), Some(dir.path()));
}

#[test]
fn discover_vault_root_returns_none_when_no_marker_in_ancestors() {
    let dir = tempdir().unwrap();

    let found = bootstrap::discover_vault_root(dir.path());

    assert!(found.is_none());
}

#[test]
fn resolve_vault_root_prefers_explicit_flag_over_everything() {
    // CWD is itself a vault and the env var points at a third place;
    // the explicit flag must still win.
    let flagged = tempdir().unwrap();
    let cwd = tempdir().unwrap();
    make_minimal_vault(cwd.path());

    let resolved =
        bootstrap::resolve_vault_root(Some(flagged.path()), cwd.path(), Some("/some/env/vault"));

    assert_eq!(resolved.as_deref(), Some(flagged.path()));
}

#[test]
fn resolve_vault_root_prefers_cwd_discovery_over_env() {
    // Inside a vault with the env var pointing elsewhere: discovery
    // must win so a stray CUADERNO_VAULT_PATH can't misroute writes.
    let cwd = tempdir().unwrap();
    make_minimal_vault(cwd.path());
    let nested = cwd.path().join("projects");
    fs::create_dir_all(&nested).unwrap();

    let resolved = bootstrap::resolve_vault_root(None, &nested, Some("/some/env/vault"));

    assert_eq!(resolved.as_deref(), Some(cwd.path()));
}

#[test]
fn resolve_vault_root_falls_back_to_env_outside_any_vault() {
    // CWD is not inside a vault: the env var is the fallback.
    let cwd = tempdir().unwrap();

    let resolved = bootstrap::resolve_vault_root(None, cwd.path(), Some("/env/vault"));

    assert_eq!(resolved, Some(Path::new("/env/vault").to_path_buf()));
}

#[test]
fn resolve_vault_root_treats_blank_env_as_unset() {
    let cwd = tempdir().unwrap();

    assert!(bootstrap::resolve_vault_root(None, cwd.path(), Some("   ")).is_none());
    assert!(bootstrap::resolve_vault_root(None, cwd.path(), Some("")).is_none());
}

#[test]
fn resolve_vault_root_returns_none_when_nothing_resolves() {
    let cwd = tempdir().unwrap();

    assert!(bootstrap::resolve_vault_root(None, cwd.path(), None).is_none());
}

#[test]
fn open_vault_errors_with_helpful_message_when_cuaderno_dir_missing() {
    let dir = tempdir().unwrap();

    // `.err()` avoids the `Debug` bound that `expect_err` would
    // require on the `Ok` variant — `Vault` doesn't implement `Debug`.
    let err = bootstrap::open_vault(dir.path())
        .err()
        .expect("open_vault should refuse without .cuaderno/");
    let msg = format!("{err}");
    assert!(msg.contains("no Cuaderno vault"), "unexpected error: {msg}");
    assert!(msg.contains("cdno init"), "unexpected error: {msg}");
}

#[test]
fn open_vault_returns_empty_reconciliation_report_for_minimal_vault() {
    let dir = tempdir().unwrap();
    make_minimal_vault(dir.path());

    let (_vault, report) = bootstrap::open_vault(dir.path()).expect("open succeeds");

    assert_eq!(report.scanned, 0);
    assert_eq!(report.added, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(report.removed, 0);
    assert!(report.errors.is_empty());
}
