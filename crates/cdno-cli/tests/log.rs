//! Tests for `cdno log`: in process for `commands::log::run` and the
//! exact shape `cdno log note` writes, and through the built binary (via
//! `assert_cmd`) where clap dispatch, flags and output are the point.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use cdno_cli::commands::log::LogCommands;
use cdno_cli::commands::{init, log};
use chrono::{NaiveDate, NaiveTime};
use predicates::prelude::*;
use tempfile::tempdir;

fn moment() -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 4, 25)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(14, 30, 0).unwrap())
}

#[test]
fn log_appends_a_line_to_the_daily_note_for_the_given_moment() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    log::run(dir.path(), moment(), "first entry", false).expect("log");

    let daily = dir.path().join("journal/2026/daily/2026-04-25.md");
    let content = fs::read_to_string(&daily).expect("daily note exists");
    assert!(content.contains("type: daily"));
    assert!(content.contains("- **14:30**: first entry"));
}

#[test]
fn log_stacks_multiple_entries_under_the_logs_section() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    log::run(dir.path(), moment(), "first entry", false).expect("log");
    let later = moment() + chrono::Duration::minutes(15);
    log::run(dir.path(), later, "second entry", false).expect("log");

    let daily = dir.path().join("journal/2026/daily/2026-04-25.md");
    let content = fs::read_to_string(&daily).expect("daily note exists");
    assert!(content.contains("- **14:30**: first entry"));
    assert!(content.contains("- **14:45**: second entry"));
    assert!(
        content.find("first entry").unwrap() < content.find("second entry").unwrap(),
        "entries must appear in chronological order"
    );
}

#[test]
fn log_errors_when_target_is_not_a_vault() {
    let dir = tempdir().unwrap();
    // No init: `.cuaderno/` is missing.

    let err = log::run(dir.path(), moment(), "x", false)
        .expect_err("log without an inited vault should fail");
    let msg = format!("{err}");
    assert!(msg.contains("no Cuaderno vault"), "unexpected error: {msg}");
}

// ---- `cdno log note` (T12, RFC 0002 §5.4) ----

const DAILY: &str = "journal/2026/daily/2026-04-25.md";

fn cdno() -> Command {
    let mut cmd = Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd
}

fn vault_arg(dir: &Path) -> String {
    dir.to_str().unwrap().to_owned()
}

fn write_body(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

/// Today's daily note, where a `log note` without `--date` lands. The CLI
/// stamps the local date, so the test does too.
fn today_daily(dir: &Path) -> PathBuf {
    let today = chrono::Local::now().date_naive();
    dir.join(format!(
        "journal/{}/daily/{}.md",
        today.format("%Y"),
        today.format("%Y-%m-%d")
    ))
}

/// `cdno log note --heading <heading> --body-file <body_file>` in process,
/// non-interactive, at [`moment`].
fn log_note(dir: &Path, heading: &str, body_file: &Path) -> anyhow::Result<()> {
    log::run_command(
        dir,
        moment(),
        LogCommands::Note {
            heading: Some(heading.to_owned()),
            body_file: Some(body_file.to_path_buf()),
            date: None,
        },
        true,
        false,
    )
}

/// The text of the `## <name>` section, heading excluded, up to the next
/// level-2 heading.
fn section<'a>(content: &'a str, name: &str) -> &'a str {
    let marker = format!("\n## {name}\n");
    let start = content
        .find(&marker)
        .unwrap_or_else(|| panic!("no `## {name}` in:\n{content}"))
        + marker.len();
    let rest = &content[start..];
    rest.find("\n## ").map_or(rest, |end| &rest[..end])
}

#[test]
fn log_note_writes_the_entry_under_notes_and_the_pointer_under_logs() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    let body = write_body(
        dir.path(),
        "body.md",
        "\n(A + UCV)^-1 = A^-1 - A^-1 U (C^-1 + V A^-1 U)^-1 V A^-1\n\
         used for the k=3 refit on [[projects/surrogate-model]], see [[woodbury|the note]] \
         and [[projects/surrogate-model]] again.\n\n",
    );

    log_note(dir.path(), "Woodbury identity", &body).expect("log note");

    let content = fs::read_to_string(dir.path().join(DAILY)).unwrap();
    assert_eq!(
        section(&content, "Notes").trim_end(),
        "### Woodbury identity\n\
         (A + UCV)^-1 = A^-1 - A^-1 U (C^-1 + V A^-1 U)^-1 V A^-1\n\
         used for the k=3 refit on [[projects/surrogate-model]], see [[woodbury|the note]] \
         and [[projects/surrogate-model]] again.",
        "{content}"
    );
    assert_eq!(
        section(&content, "Logs").trim_end(),
        "- **14:30**: noted [[journal/2026/daily/2026-04-25#Woodbury identity]] \
         ([[projects/surrogate-model]] [[woodbury|the note]])",
        "{content}"
    );
    let last_h2 = content.rfind("\n## ").unwrap();
    assert!(
        content[last_h2..].starts_with("\n## Logs\n"),
        "`## Logs` must stay last:\n{content}"
    );
    assert!(content.find("\n## Notes\n").unwrap() < last_h2);
}

#[test]
fn log_note_pointer_has_no_link_list_when_the_body_links_nothing() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    let body = write_body(dir.path(), "body.md", "A page of reasoning.\n");

    log_note(dir.path(), "Plain entry", &body).expect("log note");

    let content = fs::read_to_string(dir.path().join(DAILY)).unwrap();
    assert!(
        content.ends_with(
            "## Logs\n- **14:30**: noted [[journal/2026/daily/2026-04-25#Plain entry]]\n"
        ),
        "{content}"
    );
}

#[test]
fn log_note_keeps_the_indentation_of_a_body_opening_with_a_code_block() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    let body = write_body(
        dir.path(),
        "body.md",
        "\n\n    let x = solve(a, b);\n    x.norm()\n\nThe residual stays below 1e-9.\n",
    );

    log_note(dir.path(), "Solver check", &body).expect("log note");

    let content = fs::read_to_string(dir.path().join(DAILY)).unwrap();
    assert!(
        content.contains(
            "### Solver check\n    let x = solve(a, b);\n    x.norm()\n\nThe residual stays below 1e-9.\n"
        ),
        "leading indentation must survive:\n{content}"
    );
}

#[test]
fn log_note_refuses_a_duplicate_heading_with_the_domain_message_and_writes_nothing() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    let body = write_body(dir.path(), "body.md", "First derivation.\n");
    log_note(dir.path(), "Woodbury identity", &body).expect("first log note");
    let before = fs::read_to_string(dir.path().join(DAILY)).unwrap();

    let other = write_body(dir.path(), "other.md", "Second derivation.\n");
    let err = log_note(dir.path(), "Woodbury identity", &other)
        .expect_err("a duplicate heading must be refused");

    assert_eq!(
        format!("{err}"),
        "heading `Woodbury identity` is not allowed in `## Notes`: \
         a heading with that text already exists in the note"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join(DAILY)).unwrap(),
        before,
        "a refused entry must leave the daily note untouched"
    );
}

#[test]
fn log_note_refuses_a_daily_section_name_as_heading() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    let body = write_body(dir.path(), "body.md", "Substance.\n");

    let err = log_note(dir.path(), "Logs", &body).expect_err("`Logs` must be refused");

    assert_eq!(
        format!("{err}"),
        "heading `Logs` is not allowed in `## Notes`: it is the name of a daily section"
    );
    assert!(!dir.path().join(DAILY).exists(), "nothing may be written");
}

#[test]
fn log_note_refuses_a_blank_body_file_naming_the_flag() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    let body = write_body(dir.path(), "blank.md", "\n   \n\n");

    let err = log_note(dir.path(), "Empty", &body).expect_err("a blank body must be refused");

    assert!(format!("{err}").contains("--body-file is blank"), "{err}");
    assert!(!dir.path().join(DAILY).exists(), "nothing may be written");
}

#[test]
fn log_note_without_heading_fails_non_interactively_with_missing_flag() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    let body = write_body(dir.path(), "body.md", "Substance.\n");

    cdno()
        .args(["--vault", &vault_arg(dir.path()), "log", "note", "--no-interactive"])
        .arg("--body-file")
        .arg(&body)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "missing required flag: --heading (provide it explicitly or run interactively in a TTY)",
        ));
    assert!(!today_daily(dir.path()).exists(), "nothing may be written");
}

#[test]
fn log_note_without_body_file_fails_non_interactively_with_missing_flag() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    cdno()
        .args([
            "--vault",
            &vault_arg(dir.path()),
            "log",
            "note",
            "--no-interactive",
            "--heading",
            "Woodbury identity",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "missing required flag: --body-file (provide it explicitly or run interactively in a TTY)",
        ));
    assert!(!today_daily(dir.path()).exists(), "nothing may be written");
}

#[test]
fn log_note_json_emits_the_four_fields_and_date_picks_the_day() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    let body = write_body(dir.path(), "body.md", "See [[projects/surrogate-model]].\n");

    let out = cdno()
        .args([
            "--vault",
            &vault_arg(dir.path()),
            "log",
            "note",
            "--heading",
            "Woodbury identity",
            "--date",
            "2026-04-25",
            "--json",
        ])
        .arg("--body-file")
        .arg(&body)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("stdout is JSON");

    let target = "journal/2026/daily/2026-04-25#Woodbury identity";
    assert_eq!(v["path"], DAILY, "{v}");
    assert_eq!(v["target"], target, "{v}");
    assert_eq!(v["message"], format!("Noted {target}"), "{v}");
    assert_eq!(
        v["log_line"],
        format!("noted [[{target}]] ([[projects/surrogate-model]])"),
        "{v}"
    );
    assert_eq!(v.as_object().unwrap().len(), 4, "{v}");
    let content = fs::read_to_string(dir.path().join(DAILY)).unwrap();
    assert!(content.contains("### Woodbury identity\nSee [[projects/surrogate-model]]."));
    assert!(content.contains(&format!(
        ": noted [[{target}]] ([[projects/surrogate-model]])"
    )));
}

#[test]
fn log_note_prints_the_noted_target() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    let body = write_body(dir.path(), "body.md", "Substance.\n");

    cdno()
        .args([
            "--vault",
            &vault_arg(dir.path()),
            "log",
            "note",
            "--heading",
            "Woodbury identity",
            "--date",
            "2026-04-25",
        ])
        .arg("--body-file")
        .arg(&body)
        .assert()
        .success()
        .stdout("Noted journal/2026/daily/2026-04-25#Woodbury identity\n");
}

#[test]
fn log_note_with_an_unreadable_body_file_fails_naming_the_path_and_writes_nothing() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");
    let missing = dir.path().join("no-such-body.md");

    cdno()
        .args([
            "--vault",
            &vault_arg(dir.path()),
            "log",
            "note",
            "--heading",
            "Woodbury identity",
            "--date",
            "2026-04-25",
        ])
        .arg("--body-file")
        .arg(&missing)
        .assert()
        .failure()
        .stderr(predicate::str::contains(format!(
            "reading --body-file {}",
            missing.display()
        )));
    assert!(!dir.path().join(DAILY).exists(), "nothing may be written");
}

/// Runs `cdno --vault <dir> log <args…>` with nothing between `log` and
/// `args`, and returns the daily note the command reports writing. Any
/// option before the first word would already stop clap reading it as a
/// subcommand (`args_conflicts_with_subcommands`), so these tests pass no
/// `--at` and read the path back from stdout rather than guess the date.
fn log_bare(dir: &Path, args: &[&str]) -> String {
    let output = cdno()
        .args(["--vault", &vault_arg(dir), "log"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rel = stdout
        .strip_prefix("Logged to ")
        .and_then(|s| s.strip_suffix('\n'))
        .unwrap_or_else(|| panic!("unexpected stdout: {stdout}"));
    fs::read_to_string(dir.join(rel)).unwrap()
}

#[test]
fn log_dash_dash_note_logs_the_word_note() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    let content = log_bare(dir.path(), &["--", "note"]);
    let last = content.lines().last().unwrap();
    assert!(
        last.starts_with("- **") && last.ends_with("**: note"),
        "{content}"
    );
}

#[test]
fn log_help_logs_the_word_help() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    let content = log_bare(dir.path(), &["help"]);
    let last = content.lines().last().unwrap();
    assert!(
        last.starts_with("- **") && last.ends_with("**: help"),
        "{content}"
    );
}

#[test]
fn log_with_an_option_before_note_logs_the_word_note() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    // Only the first argument, before any option, can name a subcommand.
    cdno()
        .args(["--vault", &vault_arg(dir.path())])
        .args(["log", "--at", "2026-04-25T09:15", "note"])
        .assert()
        .success()
        .stdout(format!("Logged to {DAILY}\n"));
    let content = fs::read_to_string(dir.path().join(DAILY)).unwrap();
    assert!(
        content.ends_with("## Logs\n- **09:15**: note\n"),
        "{content}"
    );
}

#[test]
fn bare_log_note_runs_the_subcommand_and_fails_on_the_missing_heading() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    cdno()
        .args(["--vault", &vault_arg(dir.path()), "log", "note"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "missing required flag: --heading (provide it explicitly or run interactively in a TTY)",
        ));
    assert!(!today_daily(dir.path()).exists(), "nothing may be written");
}

#[test]
fn log_with_a_plain_message_still_appends_a_log_line() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    cdno()
        .args([
            "--vault",
            &vault_arg(dir.path()),
            "log",
            "plain message",
            "--at",
            "2026-04-25T09:15",
        ])
        .assert()
        .success()
        .stdout(format!("Logged to {DAILY}\n"));
    let content = fs::read_to_string(dir.path().join(DAILY)).unwrap();
    assert!(
        content.ends_with("## Logs\n- **09:15**: plain message\n"),
        "{content}"
    );
    assert!(!content.contains("## Notes"), "{content}");
}

#[test]
fn log_note_help_lists_no_required_flag_and_states_the_convention() {
    let out = cdno()
        .args(["log", "note", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8(out).unwrap();

    // A required flag would appear in the usage line; there is none.
    assert!(
        help.contains("\nUsage: cdno log note [OPTIONS]\n"),
        "{help}"
    );
    for flag in [
        "--heading <STRING>",
        "--body-file <PATH>",
        "--date <YYYY-MM-DD>",
    ] {
        assert!(help.contains(flag), "missing {flag}:\n{help}");
    }
    assert!(help.contains("under `## Notes`"), "{help}");
    assert!(help.contains("written to `## Logs` for you"), "{help}");
    assert!(help.contains("`#concept`"), "{help}");
}
