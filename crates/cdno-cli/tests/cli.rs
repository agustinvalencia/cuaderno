//! End-to-end tests of the `cdno` binary itself: argument parsing,
//! exit codes, stderr formatting, CWD-as-default. Anything that
//! exercises `main.rs` plumbing rather than command logic.
//!
//! These tests run the built binary as a subprocess via `assert_cmd`,
//! so they exercise clap dispatch and the path-resolution code in
//! `main.rs`. Coverage of those lines is invisible to tarpaulin
//! (subprocess instrumentation isn't tracked); `main.rs` is excluded
//! from the codecov patch report for that reason. The tests still run
//! in CI as a smoke gate.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn cdno() -> Command {
    Command::cargo_bin("cdno").expect("cdno binary built")
}

#[test]
fn init_exits_nonzero_and_emits_stderr_when_cuaderno_dir_already_exists() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    cdno()
        .arg("init")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn init_with_no_path_arg_uses_current_working_directory() {
    let dir = tempdir().unwrap();

    cdno()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success();

    assert!(dir.path().join(".cuaderno/config.toml").is_file());
}

#[test]
fn log_discovers_the_vault_root_when_run_from_a_subdirectory() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    cdno()
        .current_dir(dir.path().join("inbox"))
        .args(["log", "ran from a subdir", "--at", "2026-04-25T09:00:00"])
        .assert()
        .success();

    assert!(
        dir.path()
            .join("journal/2026/daily/2026-04-25.md")
            .is_file()
    );
}

#[test]
fn lint_exits_nonzero_with_summary_on_stderr_when_issues_found() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    std::fs::write(
        dir.path().join("bogus.md"),
        "---\ntype: nonsense\n---\n# Body\n",
    )
    .unwrap();

    cdno()
        .current_dir(dir.path())
        .arg("lint")
        .assert()
        .failure()
        .stdout(predicate::str::contains("[error]"))
        .stderr(predicate::str::contains("1 error(s)"));
}

#[test]
fn lint_warnings_are_non_fatal_by_default() {
    // A note with only a broken wikilink (a warning) must not fail the
    // command by default — clippy-style warn-don't-fail (#217).
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    std::fs::write(
        dir.path().join("note.md"),
        "---\ntype: daily\ntitle: D\n---\n# D\n\nSee [[projects/ghost]].\n",
    )
    .unwrap();

    cdno()
        .current_dir(dir.path())
        .arg("lint")
        .assert()
        .success()
        .stdout(predicate::str::contains("[warning]"))
        .stdout(predicate::str::contains("non-fatal"));
}

#[test]
fn lint_fails_on_errors_even_alongside_warnings() {
    // A warning must never suppress an error failure: a vault with both
    // an unknown-type note (error) and a broken-link note (warning)
    // fails by default.
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    std::fs::write(
        dir.path().join("bogus.md"),
        "---\ntype: nonsense\n---\n# x\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("note.md"),
        "---\ntype: daily\ntitle: D\n---\n# D\n\nSee [[projects/ghost]].\n",
    )
    .unwrap();

    cdno()
        .current_dir(dir.path())
        .arg("lint")
        .assert()
        .failure()
        .stderr(predicate::str::contains("1 error(s), 1 warning(s)"));
}

#[test]
fn lint_strict_fails_on_warnings() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    std::fs::write(
        dir.path().join("note.md"),
        "---\ntype: daily\ntitle: D\n---\n# D\n\nSee [[projects/ghost]].\n",
    )
    .unwrap();

    cdno()
        .current_dir(dir.path())
        .args(["lint", "--strict"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("0 error(s), 1 warning(s)"));
}

#[test]
fn log_errors_clearly_when_run_outside_any_vault() {
    let dir = tempdir().unwrap();

    cdno()
        .current_dir(dir.path())
        .args(["log", "anything"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not inside a Cuaderno vault"));
}

#[test]
fn vault_flag_targets_a_vault_from_outside_any_vault() {
    // The reported workflow: jot something down without cd-ing into
    // the vault. `--vault <path>` from an unrelated directory writes
    // to that vault.
    let vault = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    cdno().arg("init").arg(vault.path()).assert().success();

    cdno()
        .current_dir(elsewhere.path())
        .args(["--vault", vault.path().to_str().unwrap()])
        .args(["log", "from a flag", "--at", "2026-04-25T09:00:00"])
        .assert()
        .success();

    assert!(
        vault
            .path()
            .join("journal/2026/daily/2026-04-25.md")
            .is_file()
    );
}

#[test]
fn env_var_targets_a_vault_from_outside_any_vault() {
    let vault = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    cdno().arg("init").arg(vault.path()).assert().success();

    cdno()
        .current_dir(elsewhere.path())
        .env("CUADERNO_VAULT_PATH", vault.path())
        .args(["log", "from the env var", "--at", "2026-04-25T09:00:00"])
        .assert()
        .success();

    assert!(
        vault
            .path()
            .join("journal/2026/daily/2026-04-25.md")
            .is_file()
    );
}

#[test]
fn cwd_discovery_wins_over_env_var() {
    // Standing inside vault A while CUADERNO_VAULT_PATH points at
    // vault B: the write must land in A. Guards against a stray env
    // var silently misrouting writes in a multi-vault setup.
    let inside = tempdir().unwrap();
    let env_vault = tempdir().unwrap();
    cdno().arg("init").arg(inside.path()).assert().success();
    cdno().arg("init").arg(env_vault.path()).assert().success();

    cdno()
        .current_dir(inside.path())
        .env("CUADERNO_VAULT_PATH", env_vault.path())
        .args(["log", "lands in cwd vault", "--at", "2026-04-25T09:00:00"])
        .assert()
        .success();

    let daily = "journal/2026/daily/2026-04-25.md";
    assert!(inside.path().join(daily).is_file(), "cwd vault written");
    assert!(
        !env_vault.path().join(daily).is_file(),
        "env vault must be untouched"
    );
}

#[test]
fn capture_creates_an_inbox_file_when_run_from_inside_a_vault() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    cdno()
        .current_dir(dir.path())
        .args(["capture", "small thought from the cli"])
        .assert()
        .success();

    let inbox = std::fs::read_dir(dir.path().join("inbox")).unwrap();
    let captured: Vec<_> = inbox
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
        .collect();
    assert_eq!(captured.len(), 1, "expected exactly one inbox note");
    let name = captured[0].file_name().into_string().unwrap();
    assert!(
        name.contains("small-thought-from-the-cli"),
        "filename: {name}"
    );
}

#[test]
fn triage_lists_pending_inbox_items_when_not_a_tty() {
    // assert_cmd pipes stdout, so `is_interactive` is false and triage
    // takes the listing path without prompting (#208).
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["capture", "buy milk"])
        .assert()
        .success();

    cdno()
        .current_dir(dir.path())
        .arg("triage")
        .assert()
        .success()
        .stdout(predicate::str::contains("1 inbox item(s) pending"))
        .stdout(predicate::str::contains("buy milk"));
}

#[test]
fn triage_reports_an_empty_inbox() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .arg("triage")
        .assert()
        .success()
        .stdout(predicate::str::contains("Inbox empty"));
}

// ---------------------------------------------------------------------
// project subcommands
// ---------------------------------------------------------------------

#[test]
fn project_full_lifecycle() {
    // The acceptance criterion from #30: full lifecycle from create
    // through state, action add/done, milestone add, park, activate.
    // Each step asserts an artefact (file existence, content) so a
    // regression in any subcommand surfaces here.
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    // create
    cdno()
        .current_dir(dir.path())
        .args([
            "project",
            "create",
            "--title",
            "ICML paper",
            "--context",
            "work",
        ])
        .assert()
        .success();
    let project_path = dir.path().join("projects/icml-paper.md");
    assert!(project_path.is_file(), "project file created");

    // state
    cdno()
        .current_dir(dir.path())
        .args([
            "project",
            "state",
            "--slug",
            "icml-paper",
            "--text",
            "Started feature B.",
        ])
        .assert()
        .success();
    let body = std::fs::read_to_string(&project_path).unwrap();
    assert!(
        body.contains("Started feature B."),
        "state present:\n{body}"
    );

    // action add
    cdno()
        .current_dir(dir.path())
        .args([
            "action",
            "add",
            "--project",
            "icml-paper",
            "--title",
            "Run ablation",
            "--energy",
            "deep",
        ])
        .assert()
        .success();
    let body = std::fs::read_to_string(&project_path).unwrap();
    assert!(
        body.contains("- [ ] Run ablation (deep)"),
        "action present:\n{body}"
    );

    // action complete
    cdno()
        .current_dir(dir.path())
        .args([
            "action",
            "complete",
            "--project",
            "icml-paper",
            "--query",
            "ablation",
        ])
        .assert()
        .success();
    let body = std::fs::read_to_string(&project_path).unwrap();
    assert!(
        !body.contains("- [ ] Run ablation"),
        "open action gone:\n{body}"
    );

    // milestone add (hard)
    cdno()
        .current_dir(dir.path())
        .args([
            "project",
            "milestone",
            "add",
            "--slug",
            "icml-paper",
            "--title",
            "Submit camera-ready",
            "--date",
            "2026-05-22",
            "--hard",
        ])
        .assert()
        .success();
    let body = std::fs::read_to_string(&project_path).unwrap();
    assert!(
        body.contains("hard: 2026-05-22"),
        "hard milestone wire format:\n{body}"
    );

    // park
    cdno()
        .current_dir(dir.path())
        .args(["project", "park", "--slug", "icml-paper"])
        .assert()
        .success();
    assert!(!project_path.is_file(), "active path empty after park");
    let parked_path = dir.path().join("projects/_parked/icml-paper.md");
    assert!(parked_path.is_file(), "parked file present");

    // activate
    cdno()
        .current_dir(dir.path())
        .args(["project", "activate", "--slug", "icml-paper"])
        .assert()
        .success();
    assert!(project_path.is_file(), "active path back");
    assert!(!parked_path.is_file(), "parked path empty after activate");
}

#[test]
fn project_list_prints_each_active_project_with_state() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "Alpha", "--context", "work"])
        .assert()
        .success();
    cdno()
        .current_dir(dir.path())
        .args([
            "project",
            "create",
            "--title",
            "Beta",
            "--context",
            "personal",
        ])
        .assert()
        .success();
    cdno()
        .current_dir(dir.path())
        .args([
            "project",
            "state",
            "--slug",
            "alpha",
            "--text",
            "Just started.",
        ])
        .assert()
        .success();

    cdno()
        .current_dir(dir.path())
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha [work]"))
        .stdout(predicate::str::contains("beta [personal]"))
        .stdout(predicate::str::contains("Just started."));
}

#[test]
fn project_show_renders_summary_block() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
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
        .current_dir(dir.path())
        .args([
            "project",
            "state",
            "--slug",
            "surrogate-model",
            "--text",
            "Initial exploration.",
        ])
        .assert()
        .success();
    // Drop the template-default placeholder action so we control
    // what's at the top.
    cdno()
        .current_dir(dir.path())
        .args([
            "action",
            "complete",
            "--project",
            "surrogate-model",
            "--query",
            "first concrete",
        ])
        .assert()
        .success();
    cdno()
        .current_dir(dir.path())
        .args([
            "action",
            "add",
            "--project",
            "surrogate-model",
            "--title",
            "Email supervisor",
            "--energy",
            "light",
        ])
        .assert()
        .success();

    cdno()
        .current_dir(dir.path())
        .args(["project", "show", "surrogate-model"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[surrogate-model] (active)"))
        .stdout(predicate::str::contains("Initial exploration."))
        .stdout(predicate::str::contains("Top: Email supervisor (light)"));
}

#[test]
fn project_create_rejects_unknown_context() {
    // Clap's typed parsing should reject the value before any vault
    // work happens. The error message should name the bad input.
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "X", "--context", "studies"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("studies"));
}

#[test]
fn project_done_errors_when_action_not_found() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "X", "--context", "work"])
        .assert()
        .success();

    cdno()
        .current_dir(dir.path())
        .args([
            "action",
            "complete",
            "--project",
            "x",
            "--query",
            "ghost-action",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("ghost-action"));
}

#[test]
fn project_waiting_add_and_resolve() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "X", "--context", "work"])
        .assert()
        .success();

    cdno()
        .current_dir(dir.path())
        .args([
            "project",
            "waiting",
            "add",
            "--slug",
            "x",
            "--description",
            "Compute allocation - 500 GPU-hours",
        ])
        .assert()
        .success();
    let body = std::fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(
        body.contains("- Compute allocation - 500 GPU-hours"),
        "waiting added:\n{body}"
    );

    cdno()
        .current_dir(dir.path())
        .args([
            "project", "waiting", "resolve", "--slug", "x", "--query", "Compute",
        ])
        .assert()
        .success();
    let body = std::fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(
        !body.contains("Compute allocation"),
        "waiting resolved:\n{body}"
    );
}

#[test]
fn project_milestone_add_and_done_round_trip() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "X", "--context", "work"])
        .assert()
        .success();
    cdno()
        .current_dir(dir.path())
        .args([
            "project",
            "milestone",
            "add",
            "--slug",
            "x",
            "--title",
            "Submit camera-ready",
            "--date",
            "2026-05-22",
            "--hard",
        ])
        .assert()
        .success();
    cdno()
        .current_dir(dir.path())
        .args([
            "project",
            "milestone",
            "done",
            "--slug",
            "x",
            "--query",
            "camera-ready",
        ])
        .assert()
        .success();

    let body = std::fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(
        body.contains("- [x] Submit camera-ready"),
        "milestone marked done:\n{body}"
    );
    assert!(
        !body.contains("- [ ] Submit camera-ready"),
        "open form gone:\n{body}"
    );
}

#[test]
fn project_list_says_no_active_projects_when_vault_is_empty() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    cdno()
        .current_dir(dir.path())
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No active projects"));
}

#[test]
fn project_show_renders_parked_status() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "X", "--context", "work"])
        .assert()
        .success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "park", "--slug", "x"])
        .assert()
        .success();

    cdno()
        .current_dir(dir.path())
        .args(["project", "show", "x"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[x] (parked)"));
}

#[test]
fn project_show_says_no_open_actions_after_completing_default() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "X", "--context", "work"])
        .assert()
        .success();
    // The template seeds one default action; complete it so the
    // `Top: (no open actions)` branch fires.
    cdno()
        .current_dir(dir.path())
        .args([
            "action",
            "complete",
            "--project",
            "x",
            "--query",
            "first concrete",
        ])
        .assert()
        .success();

    cdno()
        .current_dir(dir.path())
        .args(["project", "show", "x"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Top: (no open actions)"));
}

#[test]
fn project_show_renders_completed_status() {
    // No CLI op flips a project to completed yet (#28 only handles
    // park/activate). Writing the file directly is the only way to
    // exercise the Completed arm of print_summary.
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let body = "---\ntype: project\ncontext: work\nstatus: completed\ncreated: 2026-04-01\n---\n\n# Done\n\n## Current State\nShipped 2026-04-15.\n\n## Next Actions\n\n## Waiting On\n(nothing yet)\n\n## Milestones\n";
    std::fs::write(dir.path().join("projects/done.md"), body).unwrap();

    cdno()
        .current_dir(dir.path())
        .args(["project", "show", "done"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[done] (completed)"));
}

#[test]
fn project_show_renders_state_none_when_section_empty() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "X", "--context", "work"])
        .assert()
        .success();
    // Drive the state to whitespace-only; project_summary collapses
    // it to an empty snippet, and show prints `State: (none)`.
    cdno()
        .current_dir(dir.path())
        .args(["project", "state", "--slug", "x", "--text", "  "])
        .assert()
        .success();

    cdno()
        .current_dir(dir.path())
        .args(["project", "show", "x"])
        .assert()
        .success()
        .stdout(predicate::str::contains("State: (none)"));
}

#[test]
fn project_show_renders_top_action_without_energy_suffix() {
    // Manually-edited project with a bare `- [ ]` bullet (no
    // `(deep|medium|light)` suffix). show prints the action without
    // the trailing energy parenthetical.
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let body = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# X\n\n## Current State\nFoo.\n\n## Next Actions\n- [ ] Bare action\n\n## Waiting On\n(nothing yet)\n\n## Milestones\n";
    std::fs::write(dir.path().join("projects/x.md"), body).unwrap();

    let assert = cdno()
        .current_dir(dir.path())
        .args(["project", "show", "x"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let top_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("Top:"))
        .expect("output has a Top line");
    assert_eq!(top_line.trim(), "Top: Bare action");
}

#[test]
fn project_milestone_date_must_be_iso_format() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "X", "--context", "work"])
        .assert()
        .success();

    cdno()
        .current_dir(dir.path())
        .args([
            "project",
            "milestone",
            "add",
            "--slug",
            "x",
            "--title",
            "First",
            "--date",
            "May 22 2026",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("YYYY-MM-DD"));
}

#[test]
fn search_runs_and_reports_no_matches_on_an_empty_vault() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    cdno()
        .current_dir(dir.path())
        .args(["search", "anything"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(no matches)"));
}

#[test]
fn search_rejects_an_unknown_note_type() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    cdno()
        .current_dir(dir.path())
        .args(["search", "anything", "--type", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid --type"));
}

#[test]
fn search_rejects_a_non_iso_date() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    cdno()
        .current_dir(dir.path())
        .args(["search", "anything", "--from", "yesterday"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("YYYY-MM-DD"));
}

#[test]
fn open_vault_surfaces_reconciliation_errors_on_stderr() {
    // A note that fails to index (malformed frontmatter) is otherwise
    // invisible; opening the vault for any command must warn about it
    // on stderr (#207). `commitments` is a read-only command that
    // succeeds despite the corrupt note.
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    std::fs::write(
        dir.path().join("broken.md"),
        "---\ntype: [unterminated\n---\n# Broken\n",
    )
    .unwrap();

    cdno()
        .current_dir(dir.path())
        .arg("commitments")
        .assert()
        .success()
        .stderr(predicate::str::contains("could not be indexed"))
        .stderr(predicate::str::contains("broken.md"));
}

#[test]
fn open_vault_reports_the_count_of_unindexable_notes() {
    // Two corrupt notes pin the `N note(s)` count line and exercise the
    // loop with more than one entry.
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    std::fs::write(dir.path().join("a.md"), "---\ntype: [bad\n---\n# A\n").unwrap();
    std::fs::write(dir.path().join("b.md"), "---\ntype: [bad\n---\n# B\n").unwrap();

    cdno()
        .current_dir(dir.path())
        .arg("commitments")
        .assert()
        .success()
        .stderr(predicate::str::contains("2 note(s) could not be indexed"))
        .stderr(predicate::str::contains("a.md"))
        .stderr(predicate::str::contains("b.md"));
}

#[test]
fn reindex_rebuilds_the_index_from_markdown() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    // A valid note on disk that reconciliation will index.
    std::fs::write(
        dir.path().join("zettel.md"),
        "---\ntype: zettel\ntitle: A note\n---\n# A note\n",
    )
    .unwrap();

    // `--vault` pins the target so the destructive rebuild can't touch
    // an ambient `CUADERNO_VAULT_PATH` vault. `init` seeds no indexable
    // notes, so the lone zettel is the whole count -- assert it exactly,
    // proving the rebuild actually indexed it (not just that it ran).
    cdno()
        .args([
            "--vault".as_ref(),
            dir.path().as_os_str(),
            "reindex".as_ref(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Reindexed 1 note(s)"));
}

#[test]
fn reindex_errors_outside_a_vault() {
    let dir = tempdir().unwrap();
    cdno()
        .current_dir(dir.path())
        .env_remove("CUADERNO_VAULT_PATH")
        .arg("reindex")
        .assert()
        .failure()
        .stderr(predicate::str::contains("is not inside a Cuaderno vault"));
}

#[test]
fn corrupt_index_self_heals_on_next_command() {
    // A truncated index db must not break the CLI: opening the vault
    // discards the corrupt cache and rebuilds it (#206).
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    std::fs::write(dir.path().join(".cuaderno/index.db"), b"corrupt garbage").unwrap();

    cdno()
        .args([
            "--vault".as_ref(),
            dir.path().as_os_str(),
            "commitments".as_ref(),
        ])
        .assert()
        .success();
}

#[test]
fn open_vault_is_quiet_on_a_clean_vault() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    cdno()
        .current_dir(dir.path())
        .arg("commitments")
        .assert()
        .success()
        .stderr(predicate::str::contains("could not be indexed").not());
}

// ---------------------------------------------------------------------
// --json output mode (#210)
// ---------------------------------------------------------------------

/// Run `cdno <args>` in `dir`, assert success, and parse stdout as JSON
/// — so a malformed-but-bracket-containing output can't pass.
fn json_stdout(dir: &std::path::Path, args: &[&str]) -> serde_json::Value {
    let out = cdno().current_dir(dir).args(args).assert().success();
    let bytes = &out.get_output().stdout;
    serde_json::from_slice(bytes).expect("stdout is valid JSON")
}

#[test]
fn commitments_json_emits_an_array() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    // Empty vault -> empty JSON array (not the table header).
    let v = json_stdout(dir.path(), &["commitments", "--json"]);
    assert_eq!(v, serde_json::json!([]));
}

#[test]
fn questions_json_emits_the_serialized_summaries() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args([
            "question",
            "create",
            "--domain",
            "research",
            "--text",
            "Does X beat Y?",
        ])
        .assert()
        .success();

    let v = json_stdout(dir.path(), &["questions", "--json"]);
    let arr = v.as_array().expect("JSON array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["domain"], "research");
    assert!(arr[0]["slug"].is_string());
}

#[test]
fn status_json_emits_an_object_with_context_keys() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let v = json_stdout(dir.path(), &["status", "--json"]);
    for key in ["projects", "commitments", "lapsed_habits"] {
        assert!(v[key].is_array(), "missing array key {key}: {v}");
    }
}

#[test]
fn orient_json_emits_the_orientation_context() {
    // orient shares status's `orientation_context`, but pin its own
    // wiring so a refactor can't silently break it.
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let v = json_stdout(dir.path(), &["orient", "--json"]);
    assert!(v["projects"].is_array(), "orient JSON shape: {v}");
    assert!(v["commitments"].is_array(), "orient JSON shape: {v}");
}

#[test]
fn search_json_emits_an_array_of_hits() {
    // End-to-end through the real `--json` flag wiring (#227): two notes
    // match the query, so the array has two structured hits in best-first
    // order. Exercises clap -> main -> run -> serialize -> stdout.
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["capture", "sparse attention kernel benchmark"])
        .assert()
        .success();
    cdno()
        .current_dir(dir.path())
        .args(["capture", "more notes on sparse attention today"])
        .assert()
        .success();

    let v = json_stdout(dir.path(), &["search", "sparse attention", "--json"]);
    let arr = v.as_array().expect("JSON array of hits");
    assert_eq!(arr.len(), 2, "both captures should match: {v}");

    let first = &arr[0];
    for key in ["path", "note_type", "snippet", "score"] {
        assert!(first.get(key).is_some(), "hit missing `{key}`: {first}");
    }
    assert!(
        first.as_object().unwrap().contains_key("title"),
        "hit carries a title key (may be null): {first}"
    );
    assert!(
        first["path"].as_str().unwrap().starts_with("inbox/"),
        "captured notes live in inbox/: {first}"
    );
    // Best-first: lower bm25 score ranks earlier (the documented sort key).
    let s0 = arr[0]["score"].as_f64().expect("numeric score");
    let s1 = arr[1]["score"].as_f64().expect("numeric score");
    assert!(
        s0 <= s1,
        "results must be best-first by score: {s0} then {s1}"
    );
}

#[test]
fn search_json_emits_empty_array_on_no_matches() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let v = json_stdout(dir.path(), &["search", "nonexistent-term-xyz", "--json"]);
    assert_eq!(v, serde_json::json!([]));
}

#[test]
fn project_list_json_emits_summaries() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "Alpha", "--context", "work"])
        .assert()
        .success();

    let v = json_stdout(dir.path(), &["project", "list", "--json"]);
    let arr = v.as_array().expect("JSON array");
    assert_eq!(arr.len(), 1, "{v}");
    assert_eq!(arr[0]["slug"], "alpha");
    // `state_snippet` exists only on the summary, not on ProjectFrontmatter
    // — its presence pins that we serialise summaries, not the raw
    // (path, frontmatter) tuples.
    assert!(
        arr[0].get("state_snippet").is_some(),
        "summary carries derived state_snippet: {}",
        arr[0]
    );
}

#[test]
fn project_list_json_is_empty_array_with_no_active_projects() {
    // The empty-Vec path, shared by all three list verbs.
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    assert_eq!(
        json_stdout(dir.path(), &["project", "list", "--json"]),
        serde_json::json!([])
    );
}

#[test]
fn portfolio_list_json_emits_summaries() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["portfolio", "create", "--question", "Sparse vs dense OOD"])
        .assert()
        .success();

    let v = json_stdout(dir.path(), &["portfolio", "list", "--json"]);
    let arr = v.as_array().expect("JSON array");
    assert_eq!(arr.len(), 1, "{v}");
    assert_eq!(arr[0]["slug"], "sparse-vs-dense-ood");
    assert_eq!(arr[0]["question"], "Sparse vs dense OOD");
    assert!(
        arr[0].get("evidence_count").is_some(),
        "summary carries evidence_count: {}",
        arr[0]
    );
}

#[test]
fn stewardship_list_json_emits_summaries() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args([
            "stewardship",
            "create",
            "--name",
            "Finances",
            "--context",
            "household",
        ])
        .assert()
        .success();

    let v = json_stdout(dir.path(), &["stewardship", "list", "--json"]);
    let arr = v.as_array().expect("JSON array");
    assert_eq!(arr.len(), 1, "{v}");
    assert_eq!(arr[0]["slug"], "finances");
    // Lowercase, matching the MCP DTO (`flat`/`expanded`), not PascalCase.
    assert_eq!(
        arr[0]["variant"], "flat",
        "variant casing matches the MCP DTO: {}",
        arr[0]
    );
}

#[test]
fn action_list_json_emits_entries() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "Alpha", "--context", "work"])
        .assert()
        .success();
    cdno()
        .current_dir(dir.path())
        .args([
            "action",
            "add",
            "--project",
            "alpha",
            "--title",
            "Run ablation",
            "--energy",
            "deep",
        ])
        .assert()
        .success();

    let v = json_stdout(
        dir.path(),
        &["action", "list", "--project", "alpha", "--json"],
    );
    let arr = v.as_array().expect("JSON array");
    // The project scaffold seeds a default "Define first concrete step"
    // bullet, so find ours rather than assuming the count.
    let ablation = arr
        .iter()
        .find(|e| e["text"].as_str().unwrap_or("").contains("Run ablation"))
        .unwrap_or_else(|| panic!("Run ablation entry present: {v}"));
    // energy serialises kebab-case ("deep"), matching the MCP DTO.
    assert_eq!(ablation["energy"], "deep", "energy casing: {ablation}");
}

// ---------------------------------------------------------------------
// --json on write verbs (#227): {path, message}
// ---------------------------------------------------------------------

#[test]
fn project_create_json_emits_a_write_result() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let v = json_stdout(
        dir.path(),
        &[
            "project",
            "create",
            "--title",
            "Alpha",
            "--context",
            "work",
            "--json",
        ],
    );
    assert!(
        v["path"].as_str().unwrap().ends_with("projects/alpha.md"),
        "write result path: {v}"
    );
    assert!(
        v["message"].as_str().unwrap().contains("Created"),
        "write result message: {v}"
    );
}

#[test]
fn action_add_json_emits_a_write_result() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "Alpha", "--context", "work"])
        .assert()
        .success();
    let v = json_stdout(
        dir.path(),
        &[
            "action",
            "add",
            "--project",
            "alpha",
            "--title",
            "Run ablation",
            "--energy",
            "deep",
            "--json",
        ],
    );
    // No-note branch reports the project map as the written file.
    assert!(
        v["path"].as_str().unwrap().ends_with("projects/alpha.md"),
        "write result path: {v}"
    );
    assert!(
        v["message"].as_str().unwrap().contains("Action added"),
        "write result message: {v}"
    );
}

#[test]
fn action_add_with_note_json_emits_a_write_result() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["project", "create", "--title", "Alpha", "--context", "work"])
        .assert()
        .success();
    let v = json_stdout(
        dir.path(),
        &[
            "action",
            "add",
            "--project",
            "alpha",
            "--title",
            "Run ablation",
            "--energy",
            "deep",
            "--note",
            "--json",
        ],
    );
    // Distinct branch: `path` is the action NOTE, not the project map.
    assert!(
        v["path"]
            .as_str()
            .unwrap()
            .ends_with("actions/run-ablation.md"),
        "write result path: {v}"
    );
    assert!(
        v["message"]
            .as_str()
            .unwrap()
            .contains("Action added to projects/alpha.md with note"),
        "write result message: {v}"
    );
}

#[test]
fn portfolio_link_json_emits_a_write_result() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args([
            "project",
            "create",
            "--title",
            "Surrogate",
            "--context",
            "work",
        ])
        .assert()
        .success();
    cdno()
        .current_dir(dir.path())
        .args(["portfolio", "create", "--question", "Sparse vs dense OOD"])
        .assert()
        .success();
    let v = json_stdout(
        dir.path(),
        &[
            "portfolio",
            "link",
            "--portfolio",
            "sparse-vs-dense-ood",
            "--project",
            "projects/surrogate",
            "--json",
        ],
    );
    let msg = v["message"].as_str().unwrap();
    assert!(
        msg.contains("Linked portfolio") && msg.contains("surrogate"),
        "write result message: {v}"
    );
    // The link verb's message embeds the path it returns.
    assert!(
        msg.contains(v["path"].as_str().unwrap()),
        "path embedded in message: {v}"
    );
}

#[test]
fn log_json_emits_a_write_result() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let v = json_stdout(dir.path(), &["log", "a quick entry", "--json"]);
    assert!(
        v["path"].as_str().unwrap().contains("daily/"),
        "write result path: {v}"
    );
    assert!(v["message"].as_str().unwrap().contains("Logged"), "{v}");
}

#[test]
fn capture_json_emits_a_write_result() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let v = json_stdout(dir.path(), &["capture", "a stray thought", "--json"]);
    assert!(
        v["path"].as_str().unwrap().starts_with("inbox/"),
        "write result path: {v}"
    );
    assert!(v["message"].as_str().unwrap().contains("Captured"), "{v}");
}

#[test]
fn question_create_json_emits_a_write_result() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let v = json_stdout(
        dir.path(),
        &[
            "question",
            "create",
            "--domain",
            "research",
            "--text",
            "Does sparse beat dense?",
            "--json",
        ],
    );
    assert!(
        v["path"]
            .as_str()
            .unwrap()
            .starts_with("questions/research/"),
        "write result path: {v}"
    );
    assert!(v["message"].as_str().unwrap().contains("Created"), "{v}");
}

#[test]
fn question_transition_json_emits_a_write_result() {
    // The transition verbs (park/answer/retire/activate) build a dynamic
    // message via capitalise_first(past_tense(verb)) — a distinct path
    // from `create`, so exercise it under --json too.
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args([
            "question",
            "create",
            "--domain",
            "research",
            "--text",
            "Does sparse beat dense?",
        ])
        .assert()
        .success();
    let v = json_stdout(
        dir.path(),
        &[
            "question",
            "park",
            "--slug",
            "does-sparse-beat-dense",
            "--json",
        ],
    );
    // Park keeps the note at questions/research/<slug>.md (no folder move).
    assert!(
        v["path"]
            .as_str()
            .unwrap()
            .starts_with("questions/research/"),
        "write result path: {v}"
    );
    assert!(
        v["message"].as_str().unwrap().starts_with("Parked"),
        "transition message: {v}"
    );
}

#[test]
fn commit_create_json_emits_a_write_result() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let v = json_stdout(
        dir.path(),
        &[
            "commit",
            "create",
            "--title",
            "Pay rent",
            "--due",
            "2026-08-01",
            "--context",
            "personal",
            "--json",
        ],
    );
    assert!(
        v["path"].as_str().unwrap().contains("commitments/"),
        "write result path: {v}"
    );
    assert!(v["message"].as_str().unwrap().contains("Created"), "{v}");
}

#[test]
fn file_json_emits_a_write_result() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["portfolio", "create", "--question", "Sparse vs dense OOD"])
        .assert()
        .success();
    let v = json_stdout(
        dir.path(),
        &[
            "file",
            "--portfolio",
            "sparse-vs-dense-ood",
            "--source",
            "Chen 2025",
            "--origin",
            "projects/foo",
            "--content",
            "They show a 4x speedup.",
            "--json",
        ],
    );
    assert!(
        v["path"]
            .as_str()
            .unwrap()
            .starts_with("portfolios/sparse-vs-dense-ood/"),
        "write result path: {v}"
    );
    assert!(v["message"].as_str().unwrap().contains("Filed"), "{v}");
}

#[test]
fn track_json_emits_a_write_result() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
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
    let v = json_stdout(
        dir.path(),
        &[
            "track",
            "gym",
            "--stewardship",
            "health",
            "--content",
            "Upper body, good energy.",
            "--json",
        ],
    );
    assert!(
        v["path"].as_str().unwrap().contains("stewardships/health/"),
        "write result path: {v}"
    );
    assert!(v["message"].as_str().unwrap().contains("Tracked"), "{v}");
}

#[test]
fn portfolio_create_json_emits_a_write_result() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let v = json_stdout(
        dir.path(),
        &[
            "portfolio",
            "create",
            "--question",
            "Sparse vs dense OOD",
            "--json",
        ],
    );
    assert!(
        v["path"]
            .as_str()
            .unwrap()
            .ends_with("portfolios/sparse-vs-dense-ood/_index.md"),
        "write result path: {v}"
    );
    assert!(v["message"].as_str().unwrap().contains("Created"), "{v}");
}

#[test]
fn stewardship_create_json_emits_a_write_result() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    let v = json_stdout(
        dir.path(),
        &[
            "stewardship",
            "create",
            "--name",
            "Finances",
            "--context",
            "household",
            "--json",
        ],
    );
    assert!(
        v["path"]
            .as_str()
            .unwrap()
            .ends_with("stewardships/finances.md"),
        "write result path: {v}"
    );
    assert!(v["message"].as_str().unwrap().contains("Created"), "{v}");
}

// ---------------------------------------------------------------------
// review weekly (#209)
// ---------------------------------------------------------------------

#[test]
fn review_weekly_non_interactive_reports_no_note() {
    // Piped stdout -> non-interactive: it reads rather than prompts,
    // reusing `cdno weekly`'s placeholder.
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();
    cdno()
        .current_dir(dir.path())
        .args(["review", "weekly"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No weekly note for"));
}

#[test]
fn review_weekly_non_interactive_prints_the_current_weeks_note() {
    use chrono::{Datelike, Local};
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    // `review weekly` defaults to today's ISO week; seed that note.
    let iso = Local::now().date_naive().iso_week();
    let rel = format!(
        "journal/{}/weekly/{}-W{:02}.md",
        iso.year(),
        iso.year(),
        iso.week()
    );
    let path = dir.path().join(&rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "---\ntype: weekly\n---\n\n# Week\n\n## Wins\nshipped the parser\n",
    )
    .unwrap();

    cdno()
        .current_dir(dir.path())
        .args(["review", "weekly"])
        .assert()
        .success()
        .stdout(predicate::str::contains("shipped the parser"))
        // frontmatter is stripped by the shared weekly renderer
        .stdout(predicate::str::contains("type: weekly").not());
}

// ---------------------------------------------------------------------
// normalise (#233)
// ---------------------------------------------------------------------

#[test]
fn normalise_check_flags_then_normalise_reorders_frontmatter() {
    let dir = tempdir().unwrap();
    cdno().arg("init").arg(dir.path()).assert().success();

    // A project note with scrambled frontmatter.
    let p = dir.path().join("projects/foo.md");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(
        &p,
        "---\nstatus: active\ntype: project\ncontext: work\ncreated: 2026-04-01\n---\n# Foo\n\n## Current State\n",
    )
    .unwrap();

    // --check reports it and exits non-zero, writing nothing.
    cdno()
        .current_dir(dir.path())
        .args(["normalise", "--check"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("projects/foo.md"));

    // The default pass rewrites it.
    cdno()
        .current_dir(dir.path())
        .args(["normalise"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Normalised frontmatter in 1"));

    let out = std::fs::read_to_string(&p).unwrap();
    assert!(
        out.starts_with(
            "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---"
        ),
        "canonical order expected, got:\n{out}"
    );

    // Now canonical: --check passes (exit 0).
    cdno()
        .current_dir(dir.path())
        .args(["normalise", "--check"])
        .assert()
        .success();
}
