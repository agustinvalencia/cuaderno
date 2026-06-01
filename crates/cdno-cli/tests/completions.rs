//! End-to-end tests for `cdno completions <shell>` and the runtime
//! completion intercept.
//!
//! Two layers exercised:
//!
//! 1. **Script emission** — `cdno completions zsh|bash|fish|elvish|powershell`
//!    must print a non-empty registration shim with the shell-specific
//!    marker (`#compdef cdno` for zsh, `_clap_complete_cdno()` for
//!    bash, etc.). Sourcing this script is what wires TAB.
//!
//! 2. **Runtime intercept** — when invoked with `COMPLETE=<shell>` and
//!    `_CLAP_COMPLETE_INDEX=<n>`, the binary returns vault-aware
//!    completion candidates (project slugs, portfolio slugs, etc.)
//!    rather than executing the requested subcommand. This is the
//!    `CompleteEnv::with_factory(...).complete()` short-circuit at
//!    the top of `main`.
//!
//! Subprocess tests rather than unit tests on the completer functions
//! because the production CWD-discovery path is exactly what we want
//! to exercise — and rigging an in-process `current_dir` flips
//! global state.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn cdno() -> Command {
    Command::cargo_bin("cdno").expect("cdno binary built")
}

// ---------------------------------------------------------------------
// Script emission per shell.
// ---------------------------------------------------------------------

#[test]
fn completions_zsh_emits_compdef_header() {
    cdno()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#compdef cdno"))
        .stdout(predicate::str::contains("COMPLETE=\"zsh\""));
}

#[test]
fn completions_bash_emits_function_and_complete_directive() {
    cdno()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_clap_complete_cdno"))
        .stdout(predicate::str::contains("complete "))
        .stdout(predicate::str::contains("COMPLETE=\"bash\""));
}

#[test]
fn completions_fish_emits_complete_directive() {
    cdno()
        .args(["completions", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete"))
        .stdout(predicate::str::contains("--command cdno"))
        .stdout(predicate::str::contains("COMPLETE=fish"));
}

#[test]
fn completions_elvish_emits_registration() {
    cdno()
        .args(["completions", "elvish"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("edit:completion:arg-completer[cdno]")
                .and(predicate::str::contains(r#"COMPLETE="elvish""#)),
        );
}

#[test]
fn completions_powershell_emits_registration() {
    cdno()
        .args(["completions", "powershell"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Register-ArgumentCompleter"));
}

#[test]
fn completions_unknown_shell_errors_cleanly() {
    // clap rejects at parse time with `error: invalid value for shell`.
    cdno()
        .args(["completions", "tcsh"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

// ---------------------------------------------------------------------
// Runtime intercept — vault-aware candidates.
//
// The shell-emitted shim sets two env vars and re-invokes the binary
// with `cdno -- <words>`. We reproduce that contract directly here.
//
// `_CLAP_COMPLETE_INDEX` is the 0-based offset into the words array
// pointing at the cursor position. For `cdno action add --project <CURSOR>`
// the words array is `[cdno, action, add, --project, ""]` so the
// cursor sits at index 4.
// ---------------------------------------------------------------------

fn init_vault() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    dir
}

#[test]
fn intercept_returns_active_project_slugs_for_action_project_flag() {
    let vault = init_vault();
    cdno()
        .current_dir(vault.path())
        .args([
            "project",
            "create",
            "--title",
            "Surrogate Model",
            "--context",
            "work",
        ])
        .assert()
        .success();
    cdno()
        .current_dir(vault.path())
        .args([
            "project",
            "create",
            "--title",
            "Tax Filing",
            "--context",
            "personal",
        ])
        .assert()
        .success();

    let output = cdno()
        .current_dir(vault.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "4")
        .args(["--", "cdno", "action", "add", "--project", ""])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();
    assert!(
        stdout.contains("surrogate-model"),
        "expected surrogate-model in completions:\n{stdout}"
    );
    assert!(
        stdout.contains("tax-filing"),
        "expected tax-filing in completions:\n{stdout}"
    );
}

#[test]
fn intercept_returns_parked_slugs_for_project_activate() {
    let vault = init_vault();
    cdno()
        .current_dir(vault.path())
        .args([
            "project",
            "create",
            "--title",
            "Old Initiative",
            "--context",
            "work",
        ])
        .assert()
        .success();
    cdno()
        .current_dir(vault.path())
        .args(["project", "park", "--slug", "old-initiative"])
        .assert()
        .success();

    // `cdno project activate --slug <CURSOR>` → index 4.
    let output = cdno()
        .current_dir(vault.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "4")
        .args(["--", "cdno", "project", "activate", "--slug", ""])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();
    assert!(
        stdout.contains("old-initiative"),
        "expected old-initiative in parked completions:\n{stdout}"
    );
}

#[test]
fn intercept_returns_portfolio_slugs() {
    let vault = init_vault();
    cdno()
        .current_dir(vault.path())
        .args([
            "portfolio",
            "create",
            "--question",
            "Why does the surrogate plateau?",
        ])
        .assert()
        .success();

    // `cdno portfolio show --portfolio <CURSOR>` → index 4.
    let output = cdno()
        .current_dir(vault.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "4")
        .args(["--", "cdno", "portfolio", "show", "--portfolio", ""])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();
    assert!(
        stdout.contains("why-does-the-surrogate-plateau"),
        "expected the portfolio slug in completions:\n{stdout}"
    );
}

#[test]
fn intercept_returns_stewardship_slugs() {
    let vault = init_vault();
    cdno()
        .current_dir(vault.path())
        .args([
            "stewardship",
            "create",
            "--name",
            "Health",
            "--context",
            "personal",
            "--tracking",
        ])
        .assert()
        .success();

    // `cdno track gym --stewardship <CURSOR>` → index 4.
    let output = cdno()
        .current_dir(vault.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "4")
        .args(["--", "cdno", "track", "gym", "--stewardship", ""])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();
    assert!(
        stdout.contains("health"),
        "expected health in stewardship completions:\n{stdout}"
    );
}

#[test]
fn intercept_returns_question_slugs() {
    let vault = init_vault();
    cdno()
        .current_dir(vault.path())
        .args([
            "question",
            "create",
            "--domain",
            "research",
            "--text",
            "How accurate can a surrogate get?",
        ])
        .assert()
        .success();

    // `cdno question park --slug <CURSOR>` → index 4.
    let output = cdno()
        .current_dir(vault.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "4")
        .args(["--", "cdno", "question", "park", "--slug", ""])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();
    assert!(
        stdout.contains("how-accurate"),
        "expected the question slug in completions:\n{stdout}"
    );
}

#[test]
fn intercept_emits_no_candidates_outside_a_vault() {
    // No vault at the parent directory. The completer for `--project`
    // must silently return nothing rather than panicking, so the
    // overall completion request still succeeds (with empty output
    // beyond what clap would add for the flag name itself).
    let outside = tempdir().unwrap();
    let output = cdno()
        .current_dir(outside.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "4")
        .args(["--", "cdno", "action", "add", "--project", ""])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();
    // The dynamic engine may still emit nothing or a trailing newline;
    // what matters is no slug accidentally surfaced and the binary
    // exited cleanly. Slug-shaped tokens contain hyphens; assert the
    // absence of any.
    let saw_slug = stdout
        .lines()
        .any(|l| !l.is_empty() && l.contains('-') && !l.starts_with("--"));
    assert!(
        !saw_slug,
        "expected no slug candidates outside a vault, got:\n{stdout}"
    );
}
