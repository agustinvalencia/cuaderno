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

// ---------------------------------------------------------------------
// The method-version skew notice (RFC 0006 plan T9): printed by `main`
// after a command has opened its vault, never by `open_vault`.
// ---------------------------------------------------------------------

fn cdno() -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd
}

/// A fresh vault whose `[vault] method_version` line is replaced by `line`
/// (`None` removes it, leaving the vault unstamped).
fn inited_vault_with_method_line(line: Option<&str>) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let config_path = dir.path().join(paths::CONFIG_FILE);
    let config = fs::read_to_string(&config_path).unwrap();
    let stamped = format!("method_version = \"{}\"", cdno_domain::METHOD_VERSION);
    assert!(
        config.contains(&stamped),
        "init stamps the vault:\n{config}"
    );
    let replaced = match line {
        Some(line) => config.replace(&stamped, line),
        None => config.replace(&format!("{stamped}\n"), ""),
    };
    fs::write(&config_path, replaced).unwrap();
    dir
}

/// Run `cdno orient --json` in `dir`: its stdout must parse as JSON whatever
/// the notice does. Returns stderr.
fn orient_json_stderr(dir: &Path) -> String {
    let output = cdno()
        .current_dir(dir)
        .args(["orient", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .expect("orient --json's stdout is JSON");
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn an_unstamped_vault_at_binary_1_0_prints_no_notice() {
    // Between T9 and T23 the binary is 1.0, and an unstamped vault reads as
    // 1.0: not below the binary, so nothing to say.
    assert_eq!(
        cdno_domain::METHOD_VERSION,
        cdno_core::config::MethodVersion { major: 1, minor: 0 }
    );
    let dir = inited_vault_with_method_line(None);

    assert_eq!(orient_json_stderr(dir.path()), "");
    let output = cdno()
        .current_dir(dir.path())
        .arg("questions")
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
}

#[test]
fn a_newer_minor_vault_prints_the_notice_once_on_stderr_only() {
    let binary = cdno_domain::METHOD_VERSION;
    let newer = format!("{}.{}", binary.major, binary.minor + 1);
    let dir = inited_vault_with_method_line(Some(&format!("method_version = \"{newer}\"")));

    let stderr = orient_json_stderr(dir.path());

    assert_eq!(
        stderr,
        format!(
            "notice: this vault follows RLM {newer}, a newer minor version than the RLM {binary} \
             this cdno implements \u{2014} writes proceed; upgrade cdno for the practices it adds\n"
        )
    );
    // No count of maps or dates: the only digits are the two versions'.
    let without_versions = stderr.replace(&newer, "").replace(&binary.to_string(), "");
    assert!(
        !without_versions.chars().any(|c| c.is_ascii_digit()),
        "the notice carries a count: {stderr}"
    );
}

#[test]
fn an_older_vault_prints_the_migrate_notice_and_migrate_itself_does_not() {
    // A vault recording an older major than the binary: effective version
    // below the binary's, so the notice points at `cdno migrate --dry-run`.
    let dir = inited_vault_with_method_line(Some("method_version = \"0.9\""));
    let binary = cdno_domain::METHOD_VERSION;

    assert_eq!(
        orient_json_stderr(dir.path()),
        format!(
            "notice: this vault records RLM 0.9; this cdno implements RLM {binary} \u{2014} \
             `cdno migrate --dry-run` shows what changes\n"
        )
    );

    // `cdno migrate` is the remedy: it says the versions itself.
    let output = cdno()
        .current_dir(dir.path())
        .args(["migrate", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
}
