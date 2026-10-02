//! `cdno now` — what you are in the middle of.
//!
//! In-process dispatch, like the other per-subcommand suites: subprocess
//! runs are invisible to tarpaulin on Linux, and `tests/cli.rs` covers
//! the `main.rs` plumbing separately.

use std::fs;
use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use tempfile::TempDir;

use cdno_cli::commands::action::ActionCommands;
use cdno_cli::commands::now::{build_line, build_now, elapsed_since, now_json};
use cdno_cli::commands::project::ProjectCommands;
use cdno_cli::commands::{action, init, project};
use cdno_domain::CurrentFocus;
use cdno_domain::frontmatter::{Context, EnergyLevel};

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 5, 26).unwrap()
}

fn moment(h: u32, min: u32) -> NaiveDateTime {
    day().and_hms_opt(h, min, 0).unwrap()
}

fn t(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

/// A vault with one active project carrying one open bullet.
fn vault_with_action() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    init::run(dir.path()).expect("init");
    project::run(
        dir.path(),
        moment(9, 0),
        ProjectCommands::Create {
            title: Some("Alpha".into()),
            context: Some(Context::Work),
            question: None,
            var: vec![],
        },
        true,
        false,
    )
    .expect("create project");
    action::run(
        dir.path(),
        moment(9, 0),
        ActionCommands::Add {
            project: Some("alpha".into()),
            title: Some("Draft methods".into()),
            energy: Some(EnergyLevel::Deep),
            note: false,
            var: vec![],
        },
        true,
        false,
    )
    .expect("add action");
    dir
}

fn start(root: &Path, at: NaiveDateTime, query: &str) {
    action::run(
        root,
        at,
        ActionCommands::Start {
            project: Some("alpha".into()),
            query: Some(query.into()),
            unplanned: false,
            title: None,
            energy: None,
        },
        true,
        false,
    )
    .expect("start action");
}

#[test]
fn elapsed_reads_in_words() {
    assert_eq!(
        elapsed_since(moment(9, 0), moment(9, 0)).as_deref(),
        Some("just now")
    );
    assert_eq!(
        elapsed_since(moment(9, 0), moment(9, 30)).as_deref(),
        Some("30m")
    );
    assert_eq!(
        elapsed_since(moment(9, 0), moment(11, 0)).as_deref(),
        Some("2h")
    );
    assert_eq!(
        elapsed_since(moment(9, 0), moment(11, 25)).as_deref(),
        Some("2h 25m")
    );
}

#[test]
fn a_start_in_the_future_says_nothing_rather_than_a_negative() {
    // Any stamp ahead of render time: a clock moving backwards within
    // the day (a timezone change, an NTP correction), or a line typed
    // into the log with a later stamp. A negative duration would be worse
    // than silence.
    assert_eq!(elapsed_since(moment(23, 0), moment(1, 0)), None);
}

#[test]
fn with_nothing_started_it_says_so_rather_than_printing_an_empty_frame() {
    let dir = vault_with_action();
    let out = build_now(dir.path(), moment(10, 0)).expect("builds");
    assert!(out.contains("Nothing started."), "empty state:\n{out}");
}

#[test]
fn a_start_shows_the_project_the_action_and_how_long() {
    let dir = vault_with_action();
    start(dir.path(), moment(9, 30), "Draft methods");

    let out = build_now(dir.path(), moment(11, 0)).expect("builds");
    assert!(out.contains("alpha"), "project:\n{out}");
    // The RESOLVED bullet text, energy suffix and all — not the query.
    assert!(out.contains("Draft methods (deep)"), "action:\n{out}");
    assert!(
        out.contains("since 09:30 (1h 30m)"),
        "start stamp and elapsed:\n{out}"
    );
}

#[test]
fn completing_the_action_clears_the_focus() {
    // The whole #568 invariant, end to end through the CLI: a start and
    // its close pair by exact text, so `now` empties on completion.
    let dir = vault_with_action();
    start(dir.path(), moment(9, 30), "Draft methods");
    action::run(
        dir.path(),
        moment(11, 0),
        ActionCommands::Complete {
            project: Some("alpha".into()),
            query: Some("Draft methods".into()),
        },
        true,
        false,
    )
    .expect("complete");

    let out = build_now(dir.path(), moment(11, 30)).expect("builds");
    assert!(out.contains("Nothing started."), "cleared:\n{out}");
}

#[test]
fn the_focus_is_read_back_from_the_log_not_from_cli_state() {
    // `current_focus` replays today's `## Logs`, so a start written by
    // anything at all counts. Hand-write the log line an agent or an
    // editor would leave and assert `now` sees it.
    let dir = vault_with_action();
    let daily = dir.path().join("journal/2026/daily/2026-05-26.md");
    fs::create_dir_all(daily.parent().unwrap()).unwrap();
    fs::write(
        &daily,
        "---\ntype: daily\ndate: 2026-05-26\n---\n\n# Tuesday\n\n## Logs\n\
         - **08:15**: started [[alpha]] — Draft methods (deep)\n",
    )
    .unwrap();

    let out = build_now(dir.path(), moment(9, 0)).expect("builds");
    assert!(
        out.contains("Draft methods (deep)"),
        "external start:\n{out}"
    );
    assert!(out.contains("since 08:15"), "its stamp:\n{out}");
}

#[test]
fn elapsed_is_exact_at_the_minute_and_hour_boundaries() {
    // The three thresholds the wording turns on: below a minute reads
    // "just now", exactly a minute is the first counted minute, and
    // exactly an hour switches to hours with no stray "0m".
    assert_eq!(
        elapsed_since(moment(9, 0), moment(9, 0)).as_deref(),
        Some("just now")
    );
    assert_eq!(
        elapsed_since(moment(9, 0), moment(9, 1)).as_deref(),
        Some("1m")
    );
    assert_eq!(
        elapsed_since(moment(9, 0), moment(9, 59)).as_deref(),
        Some("59m")
    );
    assert_eq!(
        elapsed_since(moment(9, 0), moment(10, 0)).as_deref(),
        Some("1h")
    );
    assert_eq!(
        elapsed_since(moment(9, 0), moment(10, 1)).as_deref(),
        Some("1h 1m")
    );
}

const ALL_KEYS: [&str; 12] = [
    "project",
    "action",
    "title",
    "note",
    "energy",
    "started",
    "started_at",
    "date",
    "carried",
    "origin",
    "elapsed_minutes",
    "last_paused",
];

#[test]
fn json_is_all_null_when_nothing_is_open() {
    // The documented contract: a caller tests one field without first
    // branching on the shape of the document. A bare `null` would break
    // `.project == null` for every consumer.
    let v = now_json(None, None, moment(10, 0));
    assert!(v.is_object(), "an object, not a bare null: {v}");
    for field in ALL_KEYS {
        assert!(v[field].is_null(), "{field} must be null: {v}");
    }
}

#[test]
fn json_carries_the_logged_text_and_a_hh_mm_stamp() {
    let focus = CurrentFocus {
        project: "alpha".to_owned(),
        action: "Draft methods (deep)".to_owned(),
        started: t(9, 5),
        date: day(),
        origin: None,
    };
    let v = now_json(Some(&focus), None, moment(10, 0));
    assert_eq!(v["project"], "alpha");
    // Verbatim, energy suffix and all — the string `complete` matches.
    assert_eq!(v["action"], "Draft methods (deep)");
    assert_eq!(v["started"], "09:05", "zero-padded HH:MM, not H:MM");
}

#[test]
fn hostile_text_from_the_log_is_sanitised_before_it_reaches_the_terminal() {
    // Both rendered strings come straight out of the daily log, which a
    // hand edit or an agent can fill with anything. Without sanitise the
    // escapes below would repaint and clear the user's terminal. Same
    // shape as the render_list case in tests/action.rs.
    //
    // The slug half is poisoned as well as the action half: `render`
    // calls sanitise twice, and a case that only dirties the action text
    // leaves the slug call free to be deleted. The wikilink target is
    // not checked against an existing project here -- the focus is
    // replayed from the log line, whatever it says.
    let dir = vault_with_action();
    let daily = dir.path().join("journal/2026/daily/2026-05-26.md");
    fs::create_dir_all(daily.parent().unwrap()).unwrap();
    fs::write(
        &daily,
        "---\ntype: daily\ndate: 2026-05-26\n---\n\n# Tuesday\n\n## Logs\n\
         - **08:15**: started [[al\u{1b}[42mpha]] — safe\tesc\u{1b}[41mRED\u{1b}[2J tail\n",
    )
    .unwrap();

    let out = build_now(dir.path(), moment(9, 0)).expect("builds");
    assert!(
        !out.contains('\u{1b}'),
        "no ESC reaches the terminal, from either half:\n{out:?}"
    );
    assert!(!out.contains('\t'), "no raw tab either:\n{out:?}");
    assert!(
        out.contains("safe"),
        "the readable action text survives:\n{out}"
    );
    assert!(out.contains("pha"), "and so does the readable slug:\n{out}");
}

#[test]
fn a_start_stamped_in_the_future_renders_without_an_elapsed_clause() {
    // A stamp ahead of render time, whatever put it there. `render`
    // must drop the "· {ago}" half rather than print a negative
    // duration — the branch elapsed_since's None exists to drive,
    // exercised here through the renderer.
    let dir = vault_with_action();
    start(dir.path(), moment(14, 0), "Draft methods");

    let out = build_now(dir.path(), moment(9, 0)).expect("builds");
    assert!(out.contains("since 14:00"), "the stamp still shows:\n{out}");
    assert!(
        !out.contains("since 14:00 ("),
        "but no elapsed clause:\n{out}"
    );
    assert!(!out.contains('-'), "and certainly no negative:\n{out}");
}

#[test]
fn a_start_seconds_in_the_future_is_also_silent() {
    // The truncation trap: `now` carries seconds, the log stamp does
    // not, so a start 1..59s ahead divides to 0 minutes. Guarding on
    // minutes would let the whole first minute read "just now" while
    // the doc promises None for any future start.
    let started = moment(9, 1);
    for secs_before in [1u32, 30, 59] {
        let now = day().and_hms_opt(9, 0, 60 - secs_before).unwrap();
        assert_eq!(
            elapsed_since(started, now),
            None,
            "{secs_before}s in the future must be silent"
        );
    }
    // And the boundary the other way still counts.
    assert_eq!(
        elapsed_since(moment(9, 0), day().and_hms_opt(9, 0, 59).unwrap()).as_deref(),
        Some("just now")
    );
}

fn yesterday() -> NaiveDate {
    day().pred_opt().unwrap()
}

/// Write the daily note for `date` with the given `## Logs` lines, each
/// one a whole entry (continuations included, newline-joined by the caller).
fn write_daily(root: &Path, date: NaiveDate, logs: &[&str]) {
    let path = root.join(format!("journal/{}/daily/{date}.md", date.format("%Y")));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        format!(
            "---\ntype: daily\ndate: {date}\n---\n\n# Day\n\n## Logs\n{}\n",
            logs.join("\n")
        ),
    )
    .unwrap();
}

fn at(date: NaiveDate, h: u32, m: u32) -> NaiveDateTime {
    date.and_hms_opt(h, m, 0).unwrap()
}

#[test]
fn elapsed_spans_midnight() {
    // 08:00 yesterday to 10:00 today is 26 hours, not the 2 a
    // time-of-day difference would say.
    assert_eq!(
        elapsed_since(at(yesterday(), 8, 0), at(day(), 10, 0)).as_deref(),
        Some("26h")
    );
    assert_eq!(
        elapsed_since(at(yesterday(), 14, 5), at(day(), 9, 0)).as_deref(),
        Some("18h 55m")
    );
    // And through the renderer: a start left open last night reads as such.
    let dir = vault_with_action();
    write_daily(
        dir.path(),
        yesterday(),
        &["- **14:05**: started [[alpha]] \u{2014} Draft methods (deep)"],
    );
    let out = build_now(dir.path(), at(day(), 9, 0)).expect("builds");
    assert!(
        out.contains("since Monday 14:05 (18h 55m)"),
        "a carried start names its day:\n{out}"
    );
}

#[test]
fn json_carries_date_carried_and_origin() {
    let dir = vault_with_action();
    // Started Monday afternoon, resumed this morning: carried is false (the
    // focus is anchored today), origin is the Monday start.
    write_daily(
        dir.path(),
        yesterday(),
        &["- **14:05**: started [[alpha]] \u{2014} [[actions/draft-methods]] (deep)"],
    );
    write_daily(
        dir.path(),
        day(),
        &["- **08:50**: resumed [[alpha]] \u{2014} [[actions/draft-methods]] (deep)"],
    );
    let (vault, _) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let focus = vault.current_focus(day()).unwrap().expect("a focus");
    let v = now_json(Some(&focus), None, at(day(), 11, 5));
    assert_eq!(v["date"], "2026-05-26");
    assert_eq!(v["carried"], false);
    assert_eq!(v["started"], "08:50");
    assert_eq!(v["started_at"], "2026-05-26T08:50");
    assert_eq!(v["origin"]["started_at"], "2026-05-25T14:05");
    assert_eq!(v["elapsed_minutes"], 135);
    assert_eq!(v["title"], "draft-methods");
    assert_eq!(v["note"], "actions/draft-methods");
    assert_eq!(v["energy"], "deep");
    assert_eq!(v["action"], "[[actions/draft-methods]] (deep)");
    for k in ALL_KEYS {
        assert!(v.get(k).is_some(), "{k} present: {v}");
    }
    let human = build_now(dir.path(), at(day(), 11, 5)).unwrap();
    assert!(
        human.contains("picked up 08:50 today (started Monday 14:05)"),
        "the resumed rendering:\n{human}"
    );

    // A start carried over midnight, never resumed: carried, no origin.
    fs::remove_file(dir.path().join("journal/2026/daily/2026-05-26.md")).unwrap();
    let (vault, _) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let focus = vault.current_focus(day()).unwrap().expect("a focus");
    let v = now_json(Some(&focus), None, at(day(), 9, 0));
    assert_eq!(v["date"], "2026-05-25");
    assert_eq!(v["carried"], true);
    assert!(v["origin"].is_null());
    // 14:05 yesterday to 09:00 today.
    assert_eq!(v["elapsed_minutes"], 18 * 60 + 55);
}

#[test]
fn line_is_sanitised_and_capped() {
    // Every piece of note-derived text in the line must go through
    // `sanitise`: the project and the started title (the focus form), and
    // the paused project, title and `next:` (the none form). Each carries
    // an ESC sequence and a C1 control (U+0085), so removing the call
    // from any one of them leaves a control character in the line.
    let dir = vault_with_action();
    write_daily(
        dir.path(),
        day(),
        &[
            "- **08:00**: started [[al\u{1b}[42m\u{85}pha]] \u{2014} Draft \u{1b}[31m\u{85}red (deep)",
        ],
    );
    let focus_line = build_line(dir.path(), at(day(), 10, 0)).expect("builds");
    assert!(
        !focus_line.chars().any(char::is_control),
        "no control character survives in the focus form: {focus_line:?}"
    );
    assert!(focus_line.starts_with("Focus: al"), "{focus_line}");
    assert!(
        focus_line.contains("pha") && focus_line.contains("red"),
        "the readable halves survive: {focus_line}"
    );

    // 300 multi-byte characters in `next:`, so a byte-index cut would
    // split one.
    let long = "\u{e9}\u{1f642}".repeat(150);
    assert_eq!(long.chars().count(), 300);
    write_daily(
        dir.path(),
        day(),
        &[&format!(
            "- **09:00**: action paused on [[al\u{1b}[42m\u{85}pha]] \u{2014} Draft \u{1b}[31m\u{85}red (deep)\n  next: \u{1b}[31m\u{85}{long}"
        )],
    );
    let none_line = build_line(dir.path(), at(day(), 10, 0)).expect("builds");
    assert!(
        !none_line.chars().any(char::is_control),
        "no control character survives in the none form: {none_line:?}"
    );
    assert!(
        none_line.starts_with("Focus: none (last paused: al"),
        "{none_line}"
    );
    assert!(none_line.contains("pha"), "{none_line}");
    assert_eq!(none_line.chars().count(), 160, "capped: {none_line:?}");
    assert!(
        none_line.ends_with('\u{2026}'),
        "with an ellipsis: {none_line:?}"
    );
    assert!(
        none_line.contains('\u{1f642}'),
        "multi-byte text is cut on a character boundary, not mangled: {none_line:?}"
    );

    // A short one is untouched and has the RFC's shape.
    write_daily(
        dir.path(),
        day(),
        &["- **08:50**: started [[alpha]] \u{2014} Draft methods (deep)"],
    );
    let line = build_line(dir.path(), at(day(), 10, 0)).unwrap();
    assert_eq!(line, "Focus: alpha \u{2014} Draft methods (since 08:50)");
}

#[test]
fn line_prints_nothing_and_exits_zero_outside_a_vault() {
    let outside = tempfile::tempdir().unwrap();
    assert_cmd::Command::cargo_bin("cdno")
        .unwrap()
        .env_remove("CUADERNO_VAULT_PATH")
        .current_dir(outside.path())
        .args(["now", "--line"])
        .assert()
        .success()
        .stdout("");
    // The ordinary form still fails there, so the silence is --line's own.
    assert_cmd::Command::cargo_bin("cdno")
        .unwrap()
        .env_remove("CUADERNO_VAULT_PATH")
        .current_dir(outside.path())
        .arg("now")
        .assert()
        .failure();
}

#[test]
fn nothing_started_shows_the_last_pause_with_its_next() {
    let dir = vault_with_action();
    write_daily(
        dir.path(),
        day(),
        &[
            "- **08:00**: started [[alpha]] \u{2014} Draft methods (deep)",
            "- **10:40**: action paused on [[alpha]] \u{2014} Draft methods (deep)\n  next: pick up at \"Prior approaches\"",
        ],
    );
    let out = build_now(dir.path(), at(day(), 11, 0)).unwrap();
    assert!(out.contains("Nothing started."), "{out}");
    assert!(
        out.contains(
            "Last paused: alpha \u{2014} Draft methods (10:40), next: pick up at \"Prior approaches\""
        ),
        "{out}"
    );
    let line = build_line(dir.path(), at(day(), 11, 0)).unwrap();
    assert_eq!(
        line,
        "Focus: none (last paused: alpha \u{2014} Draft methods, next: pick up at \"Prior approaches\")"
    );
}

#[test]
fn line_prints_nothing_and_exits_zero_on_a_broken_vault() {
    // A vault that is found but whose daily note is unreadable: the
    // failure comes after vault resolution, from the read itself.
    let dir = vault_with_action();
    let today = chrono::Local::now().date_naive();
    let path = dir
        .path()
        .join(format!("journal/{}/daily/{today}.md", today.format("%Y")));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "---\n: [unclosed\n  - {\n---\n\n## Logs\n").unwrap();
    assert_cmd::Command::cargo_bin("cdno")
        .unwrap()
        .env_remove("CUADERNO_VAULT_PATH")
        .args(["--vault", dir.path().to_str().unwrap(), "now", "--line"])
        .assert()
        .success()
        .stdout("");
}

#[cfg(unix)]
#[test]
fn line_does_not_panic_when_stdout_is_closed() {
    use std::process::{Command, Stdio};
    let dir = vault_with_action();
    let bin = assert_cmd::cargo::cargo_bin("cdno");
    let mut child = Command::new(bin)
        .env_remove("CUADERNO_VAULT_PATH")
        .args(["--vault", dir.path().to_str().unwrap(), "now", "--line"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "exit 0, not a panic's 101");
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("panicked"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
