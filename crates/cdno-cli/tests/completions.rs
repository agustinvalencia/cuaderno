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
    let mut cmd = Command::cargo_bin("cdno").expect("cdno binary built");
    // Hermetic: don't inherit the developer's ambient CUADERNO_VAULT_PATH
    // so a stray shell export can't route a test at the real vault.
    cmd.env_remove("CUADERNO_VAULT_PATH");
    // Likewise the index location (#998): the tests below assert on
    // `.cuaderno/index.db` and must find it there.
    cmd.env_remove("CUADERNO_INDEX_PATH");
    cmd
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
fn intercept_offers_parked_slugs_for_action_add_but_not_action_start() {
    // RFC 0006 §8.1: `action add` accepts a parked map, `action start`
    // does not, and each one's `--project` completes accordingly.
    let vault = init_vault();
    for title in ["Live Work", "Shelved Work"] {
        cdno()
            .current_dir(vault.path())
            .args(["project", "create", "--title", title, "--context", "work"])
            .assert()
            .success();
    }
    cdno()
        .current_dir(vault.path())
        .args(["project", "park", "--slug", "shelved-work"])
        .assert()
        .success();

    let complete = |verb: &str| {
        let output = cdno()
            .current_dir(vault.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", "4")
            .args(["--", "cdno", "action", verb, "--project", ""])
            .assert()
            .success();
        String::from_utf8(output.get_output().stdout.clone()).unwrap()
    };
    let add = complete("add");
    assert!(add.contains("live-work"), "active offered to add:\n{add}");
    assert!(
        add.contains("shelved-work"),
        "parked offered to add:\n{add}"
    );
    let start = complete("start");
    assert!(
        start.contains("live-work"),
        "active offered to start:\n{start}"
    );
    assert!(
        !start.contains("shelved-work"),
        "parked not offered to start:\n{start}"
    );
}

#[test]
fn intercept_offers_parked_slugs_for_milestone_add() {
    let vault = init_vault();
    cdno()
        .current_dir(vault.path())
        .args([
            "project",
            "create",
            "--title",
            "Shelved Work",
            "--context",
            "work",
        ])
        .assert()
        .success();
    cdno()
        .current_dir(vault.path())
        .args(["project", "park", "--slug", "shelved-work"])
        .assert()
        .success();

    // `cdno project milestone add --slug <CURSOR>` → index 5.
    let output = cdno()
        .current_dir(vault.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "5")
        .args(["--", "cdno", "project", "milestone", "add", "--slug", ""])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();
    assert!(
        stdout.contains("shelved-work"),
        "parked offered to milestone add:\n{stdout}"
    );
}

/// `hand-off` and `take-back` act on open maps, so `--slug` offers a
/// parked one as well as an active one (RFC 0006 §8.2).
#[test]
fn intercept_offers_parked_slugs_for_hand_off_and_take_back() {
    let vault = init_vault();
    cdno()
        .current_dir(vault.path())
        .args([
            "project",
            "create",
            "--title",
            "Shelved Work",
            "--context",
            "work",
        ])
        .assert()
        .success();
    cdno()
        .current_dir(vault.path())
        .args(["project", "park", "--slug", "shelved-work"])
        .assert()
        .success();

    for verb in ["hand-off", "take-back"] {
        // `cdno project <verb> --slug <CURSOR>` → index 4.
        let output = cdno()
            .current_dir(vault.path())
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", "4")
            .args(["--", "cdno", "project", verb, "--slug", ""])
            .assert()
            .success();
        let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();
        assert!(
            stdout.contains("shelved-work"),
            "parked offered to {verb}:\n{stdout}"
        );
    }
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

#[test]
fn intercept_note_type_completion_includes_custom_types() {
    // `complete_note_type` (used by `templates vars <type>` and `search --type`)
    // is vault-aware: it offers the built-ins plus any config-defined custom
    // types.
    let vault = init_vault();
    let cfg = vault.path().join(".cuaderno/config.toml");
    let mut content = std::fs::read_to_string(&cfg).unwrap_or_default();
    content.push_str("\n[note_types.person]\nfolder = \"people\"\n");
    std::fs::write(&cfg, content).unwrap();

    // Words: [cdno, templates, vars, ""] — cursor on the note_type positional.
    let output = cdno()
        .current_dir(vault.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "3")
        .args(["--", "cdno", "templates", "vars", ""])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();
    assert!(
        stdout.contains("project"),
        "built-in type in completions:\n{stdout}"
    );
    assert!(
        stdout.contains("person"),
        "custom type in completions:\n{stdout}"
    );
}

#[test]
fn intercept_returns_closed_and_parked_slugs_for_project_activate() {
    let vault = init_vault();
    for title in ["Old Initiative", "Finished Thing"] {
        cdno()
            .current_dir(vault.path())
            .args(["project", "create", "--title", title, "--context", "work"])
            .assert()
            .success();
    }
    cdno()
        .current_dir(vault.path())
        .args(["project", "park", "--slug", "old-initiative"])
        .assert()
        .success();
    cdno()
        .current_dir(vault.path())
        .args([
            "--no-interactive",
            "project",
            "drop",
            "--slug",
            "finished-thing",
            "--drop-open",
        ])
        .assert()
        .success();

    let output = cdno()
        .current_dir(vault.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "4")
        .args(["--", "cdno", "project", "activate", "--slug", ""])
        .assert()
        .success();
    let stdout = std::str::from_utf8(&output.get_output().stdout).unwrap();
    assert!(stdout.contains("old-initiative"), "{stdout}");
    assert!(stdout.contains("finished-thing"), "{stdout}");
    assert!(
        stdout.contains("dropped"),
        "labelled with its outcome:\n{stdout}"
    );
}

#[test]
fn completion_in_a_skewed_vault_prints_nothing_on_stderr() {
    // A vault on a newer minor method version gets a notice after a command
    // runs, but never at Tab time: a line on stderr there would smear across
    // the user's prompt on every keypress (RFC 0006 plan T9).
    let vault = init_vault();
    let cfg = vault.path().join(".cuaderno/config.toml");
    let content = std::fs::read_to_string(&cfg).unwrap();
    let binary = cdno_domain::METHOD_VERSION;
    let stamped = format!("method_version = \"{binary}\"");
    assert!(
        content.contains(&stamped),
        "init stamps the vault:\n{content}"
    );
    let newer = format!("method_version = \"{}.{}\"", binary.major, binary.minor + 1);
    std::fs::write(&cfg, content.replace(&stamped, &newer)).unwrap();

    // The same vault does print the notice after a real command, so the
    // silence below is the completion path's, not a vault without skew.
    let ran = cdno()
        .current_dir(vault.path())
        .args(["orient", "--json"])
        .assert()
        .success();
    assert!(
        !ran.get_output().stderr.is_empty(),
        "the skewed vault prints its notice after a command"
    );

    let output = cdno()
        .current_dir(vault.path())
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", "4")
        .args(["--", "cdno", "action", "add", "--project", ""])
        .assert()
        .success();
    let stderr = std::str::from_utf8(&output.get_output().stderr).unwrap();
    assert_eq!(stderr, "", "completion printed on stderr");
}

// ---------------------------------------------------------------------
// Completion never writes (#1007).
//
// A Tab press re-invokes the binary. Opening the index from there would
// migrate and reconcile it, and SQLite would checkpoint and remove the
// `-wal`/`-shm` files on exit, under any server holding them. The
// completers read the notes on disk instead.
// ---------------------------------------------------------------------

/// The cursor index and words of one invocation per completer.
const COMPLETERS: &[(&str, &[&str])] = &[
    // complete_active_project
    ("4", &["cdno", "action", "start", "--project", ""]),
    // complete_any_project
    ("4", &["cdno", "action", "add", "--project", ""]),
    // complete_reactivatable_project
    ("4", &["cdno", "project", "activate", "--slug", ""]),
    // complete_note_type
    ("3", &["cdno", "templates", "vars", ""]),
    // complete_portfolio
    ("4", &["cdno", "portfolio", "show", "--portfolio", ""]),
    // complete_stewardship
    ("4", &["cdno", "track", "gym", "--stewardship", ""]),
    // complete_question
    ("4", &["cdno", "question", "park", "--slug", ""]),
    // complete_note_ref
    ("2", &["cdno", "open", ""]),
];

/// A bound on every completion subprocess, so a regression that blocks (on
/// the write lock, say) fails the test instead of hanging it.
const COMPLETION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Run one completion request from `dir` and return its stdout.
fn complete(dir: &std::path::Path, index: &str, words: &[&str]) -> String {
    let output = cdno()
        .current_dir(dir)
        .env("COMPLETE", "zsh")
        .env("_CLAP_COMPLETE_INDEX", index)
        .arg("--")
        .args(words)
        .timeout(COMPLETION_TIMEOUT)
        .assert()
        .success();
    String::from_utf8(output.get_output().stdout.clone()).unwrap()
}

/// The candidates a zsh completion offers, in the order it offers them, as
/// `(value, help)`: each line is `value` or `value:help`, with a literal `:`
/// in the value escaped. The flags clap offers beside a positional's values
/// are left out.
fn candidates(stdout: &str) -> Vec<(String, Option<String>)> {
    stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut value = String::new();
            let mut help = None;
            let mut chars = l.chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => value.extend(chars.next()),
                    ':' => {
                        help = Some(chars.by_ref().collect());
                        break;
                    }
                    c => value.push(c),
                }
            }
            (value, help)
        })
        .filter(|(v, _)| !v.starts_with('-'))
        .collect()
}

/// The values of [`candidates`], as a set.
fn values(stdout: &str) -> std::collections::BTreeSet<String> {
    candidates(stdout).into_iter().map(|(v, _)| v).collect()
}

/// A vault with something for every completer: active, parked and closed
/// projects, a portfolio, an expanded stewardship, a question, a custom note
/// type, and concept notes in Markdown and in Typst. Two hand-made maps,
/// `projects/a-b.md` and `projects/a/c.md`, are a pair the path's text and
/// its components order differently, so the active order is pinned.
fn vault_for_every_completer() -> tempfile::TempDir {
    let vault = init_vault();
    let run = |args: &[&str]| {
        cdno()
            .current_dir(vault.path())
            .arg("--no-interactive")
            .args(args)
            .assert()
            .success();
    };
    for title in ["Surrogate Model", "Old Initiative", "Finished Thing"] {
        run(&["project", "create", "--title", title, "--context", "work"]);
    }
    run(&["project", "park", "--slug", "old-initiative"]);
    run(&["project", "drop", "--slug", "finished-thing", "--drop-open"]);
    run(&[
        "portfolio",
        "create",
        "--question",
        "Why does the surrogate plateau?",
    ]);
    run(&[
        "stewardship",
        "create",
        "--name",
        "Health",
        "--context",
        "personal",
        "--tracking",
    ]);
    run(&[
        "question",
        "create",
        "--domain",
        "research",
        "--text",
        "How accurate can a surrogate get?",
    ]);
    let cfg = vault.path().join(".cuaderno/config.toml");
    let mut content = std::fs::read_to_string(&cfg).unwrap();
    content.push_str("\n[note_types.person]\nfolder = \"people\"\n");
    std::fs::write(&cfg, content).unwrap();

    let root = vault.path();
    let map = std::fs::read_to_string(root.join("projects/surrogate-model.md")).unwrap();
    std::fs::create_dir_all(root.join("projects/a")).unwrap();
    std::fs::write(root.join("projects/a-b.md"), &map).unwrap();
    std::fs::write(root.join("projects/a/c.md"), &map).unwrap();
    std::fs::create_dir_all(root.join("concepts")).unwrap();
    std::fs::write(
        root.join("concepts/surrogate-trick.md"),
        "---\ntype: concept\ncreated: 2026-07-13\n---\n# Surrogate trick\n",
    )
    .unwrap();
    std::fs::write(
        root.join("concepts/bias-variance-split.typ"),
        "#metadata((type: \"concept\", created: \"2026-07-13\")) <cdno>\n\n= Bias variance split\n",
    )
    .unwrap();
    vault
}

/// Every file under `.cuaderno/`, relative to the vault root, sorted.
fn cuaderno_files(root: &std::path::Path) -> Vec<String> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join(".cuaderno"), root, &mut out);
    out.sort();
    out
}

/// The bytes and mtime of a file, or `None` when it does not exist.
fn file_state(path: &std::path::Path) -> Option<(Vec<u8>, std::time::SystemTime)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((std::fs::read(path).unwrap(), meta.modified().unwrap()))
}

#[test]
fn completion_in_a_vault_without_an_index_creates_nothing() {
    let vault = vault_for_every_completer();
    let cuaderno = vault.path().join(".cuaderno");
    for name in ["index.db", "index.db-wal", "index.db-shm"] {
        let _ = std::fs::remove_file(cuaderno.join(name));
    }
    let before = cuaderno_files(vault.path());
    assert!(!before.iter().any(|p| p.contains("index.db")), "{before:?}");

    let mut offered = std::collections::BTreeSet::new();
    for (index, words) in COMPLETERS {
        offered.extend(values(&complete(vault.path(), index, words)));
        assert_eq!(
            cuaderno_files(vault.path()),
            before,
            "completing {words:?} created or removed a file under .cuaderno/"
        );
    }
    // Answered from the files: no index was there to answer from.
    for slug in [
        "surrogate-model",
        "old-initiative",
        "finished-thing",
        "why-does-the-surrogate-plateau",
        "health",
        "how-accurate-can-a-surrogate-get",
        "person",
        "surrogate-trick",
        "bias-variance-split",
    ] {
        assert!(offered.contains(slug), "{slug} not offered: {offered:?}");
    }
}

#[test]
fn completion_in_a_vault_without_a_config_creates_nothing() {
    // A `.cuaderno/` marker and no `config.toml`: the default config is
    // used, and none is written.
    let vault = tempdir().unwrap();
    std::fs::create_dir(vault.path().join(".cuaderno")).unwrap();
    std::fs::create_dir(vault.path().join("projects")).unwrap();
    std::fs::write(
        vault.path().join("projects/surrogate-model.md"),
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-07-13\n---\n\
         # Surrogate model\n",
    )
    .unwrap();

    let mut offered = std::collections::BTreeSet::new();
    for (index, words) in COMPLETERS {
        offered.extend(values(&complete(vault.path(), index, words)));
    }
    assert!(
        cuaderno_files(vault.path()).is_empty(),
        "a completion wrote under .cuaderno/: {:?}",
        cuaderno_files(vault.path())
    );
    assert!(offered.contains("surrogate-model"), "{offered:?}");
}

#[test]
fn completion_never_resolves_an_overridden_index() {
    // `CUADERNO_INDEX_PATH` moves the index out of the vault (#998); a
    // completion opens no index, so it creates nothing there either.
    let vault = vault_for_every_completer();
    let elsewhere = tempdir().unwrap();
    let index = elsewhere.path().join("index.db");
    let before = cuaderno_files(vault.path());
    for (cursor, words) in COMPLETERS {
        cdno()
            .current_dir(vault.path())
            .env("CUADERNO_INDEX_PATH", &index)
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", cursor)
            .arg("--")
            .args(*words)
            .timeout(COMPLETION_TIMEOUT)
            .assert()
            .success();
    }
    assert_eq!(
        std::fs::read_dir(elsewhere.path()).unwrap().count(),
        0,
        "a completion created a file at the overridden index location"
    );
    assert_eq!(cuaderno_files(vault.path()), before);
}

#[test]
fn completion_leaves_a_live_index_its_sidecars_and_the_lock_untouched() {
    let vault = vault_for_every_completer();
    // A verb that opens the index leaves it, and the lock, behind.
    cdno()
        .current_dir(vault.path())
        .args(["search", "surrogate"])
        .assert()
        .success();
    // A map written by hand since that verb ran: the old completers would
    // have reconciled it into the index, a write.
    let map = std::fs::read_to_string(vault.path().join("projects/surrogate-model.md")).unwrap();
    std::fs::write(vault.path().join("projects/hand-written.md"), map).unwrap();

    // A live connection, as a long-lived server holds one: WAL mode, no
    // automatic checkpoint, and a committed write that is in the `-wal`
    // only. So `-wal` and `-shm` exist while the completions run, and a
    // process that checkpointed, or removed them as the last connection it
    // could see, would change them.
    let cuaderno = vault.path().join(".cuaderno");
    let conn = rusqlite::Connection::open(cuaderno.join("index.db")).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    conn.execute_batch(
        "CREATE TABLE completion_probe (x INTEGER); INSERT INTO completion_probe VALUES (7);",
    )
    .unwrap();

    let watched = ["index.db", "index.db-wal", "index.db-shm", ".lock"];
    let states: Vec<_> = watched
        .iter()
        .map(|name| file_state(&cuaderno.join(name)))
        .collect();
    for (name, state) in watched.iter().zip(&states) {
        assert!(
            state.is_some(),
            "{name} exists while the connection is live"
        );
    }
    assert!(
        !states[1].as_ref().unwrap().0.is_empty(),
        "the write is in the -wal"
    );
    let before = cuaderno_files(vault.path());
    // A file created and removed in `.cuaderno/` (the write lock file,
    // #1012, is gone again on release) still changes the folder's mtime.
    let folder_mtime = std::fs::metadata(&cuaderno).unwrap().modified().unwrap();

    let mut offered = std::collections::BTreeSet::new();
    for (index, words) in COMPLETERS {
        offered.extend(values(&complete(vault.path(), index, words)));
    }

    for (name, state) in watched.iter().zip(&states) {
        assert!(
            &file_state(&cuaderno.join(name)) == state,
            "{name} was removed, or its bytes or mtime changed"
        );
    }
    assert_eq!(cuaderno_files(vault.path()), before);
    assert_eq!(
        std::fs::metadata(&cuaderno).unwrap().modified().unwrap(),
        folder_mtime,
        "a file was created or removed in .cuaderno/ (the write lock file?)"
    );
    // The live connection still reads its own uncheckpointed write.
    let x: i64 = conn
        .query_row("SELECT x FROM completion_probe", [], |row| row.get(0))
        .unwrap();
    assert_eq!(x, 7);
    drop(conn);
    assert!(
        offered.contains("hand-written"),
        "a map on disk is offered without reconciling it: {offered:?}"
    );
}

/// The vault the completers' answers are compared against: opened,
/// migrated and reconciled the ordinary way, as the completers opened it
/// before #1007. The domain's `open_vault` opens the index at its default
/// location and never reads `CUADERNO_INDEX_PATH`, so the reference does not
/// depend on the test process's environment (which `.cargo/config.toml`
/// also blanks).
fn reference_vault(root: &std::path::Path) -> cdno_domain::Vault {
    cdno_domain::bootstrap::open_vault(root)
        .expect("open vault")
        .vault
}

/// What each of [`COMPLETERS`] offered before #1007, as sets, computed as it
/// was, from [`reference_vault`].
fn reference_values(root: &std::path::Path) -> Vec<std::collections::BTreeSet<String>> {
    use std::collections::BTreeSet;
    let vault = reference_vault(root);
    let today = chrono::Local::now().date_naive();
    let stem = |p: &cdno_core::path::VaultPath| {
        p.as_path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };
    let active: BTreeSet<String> = vault
        .active_projects()
        .unwrap()
        .iter()
        .map(|(p, _)| stem(p))
        .collect();
    let parked: BTreeSet<String> = vault
        .parked_projects()
        .unwrap()
        .iter()
        .map(|(p, _)| stem(p))
        .collect();
    let closed: BTreeSet<String> = vault
        .closed_projects()
        .unwrap()
        .iter()
        .map(|(p, _)| stem(p))
        .collect();
    let mut notes: BTreeSet<String> = ["today", "yesterday", "tomorrow"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    notes.extend(
        vault
            .list_note_candidates()
            .unwrap()
            .iter()
            .map(|c| cdno_cli::commands::open::slug_of(&c.path)),
    );
    vec![
        active.clone(),
        active.union(&parked).cloned().collect(),
        parked.union(&closed).cloned().collect(),
        vault
            .type_registry()
            .all_names()
            .into_iter()
            .map(str::to_owned)
            .collect(),
        vault
            .list_portfolios(today)
            .unwrap()
            .into_iter()
            .map(|s| s.slug)
            .collect(),
        vault
            .list_stewardships(today)
            .unwrap()
            .into_iter()
            .map(|s| s.slug)
            .collect(),
        vault
            .list_questions()
            .unwrap()
            .into_iter()
            .map(|q| q.slug)
            .collect(),
        notes,
    ]
}

#[test]
fn each_completer_offers_what_a_reconciled_vault_lists() {
    let vault = vault_for_every_completer();
    let offered: Vec<_> = COMPLETERS
        .iter()
        .map(|(index, words)| values(&complete(vault.path(), index, words)))
        .collect();
    let expected = reference_values(vault.path());
    for ((_, words), (got, want)) in COMPLETERS.iter().zip(offered.iter().zip(&expected)) {
        assert!(!want.is_empty(), "the fixture gives {words:?} something");
        assert_eq!(got, want, "completing {words:?}");
    }
}

#[test]
fn project_completers_keep_the_order_and_labels_of_a_reconciled_vault() {
    let vault = vault_for_every_completer();
    let stem = |p: &cdno_core::path::VaultPath| {
        p.as_path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };

    // Active maps keep the index's order: `SqliteIndex` orders `path` by
    // its bytes, so `projects/a-b.md` comes before `projects/a/c.md`.
    let (index, words) = COMPLETERS[0];
    let got: Vec<String> = candidates(&complete(vault.path(), index, words))
        .into_iter()
        .map(|(v, _)| v)
        .collect();
    let reference = reference_vault(vault.path());
    let want: Vec<String> = reference
        .active_projects()
        .unwrap()
        .iter()
        .map(|(p, _)| stem(p))
        .collect();
    assert_eq!(got, want, "active project order");
    let a_b = got.iter().position(|s| s == "a-b").unwrap();
    let c = got.iter().position(|s| s == "c").unwrap();
    assert!(a_b < c, "{got:?}");

    // `project activate`: parked then closed, each with its label.
    let (index, words) = COMPLETERS[2];
    let got = candidates(&complete(vault.path(), index, words));
    let mut want: Vec<(String, Option<String>)> = reference
        .parked_projects()
        .unwrap()
        .iter()
        .map(|(p, _)| (stem(p), Some("parked".to_owned())))
        .collect();
    want.extend(
        reference
            .closed_projects()
            .unwrap()
            .iter()
            .map(|(p, fm)| (stem(p), Some(fm.status.as_str().to_owned()))),
    );
    assert_eq!(got, want, "project activate candidates");
}
