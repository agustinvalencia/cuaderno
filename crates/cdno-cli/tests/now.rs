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
use cdno_cli::commands::now::{build_now, elapsed_since, now_json};
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
    assert_eq!(elapsed_since(t(9, 0), t(9, 0)).as_deref(), Some("just now"));
    assert_eq!(elapsed_since(t(9, 0), t(9, 30)).as_deref(), Some("30m"));
    assert_eq!(elapsed_since(t(9, 0), t(11, 0)).as_deref(), Some("2h"));
    assert_eq!(elapsed_since(t(9, 0), t(11, 25)).as_deref(), Some("2h 25m"));
}

#[test]
fn a_start_in_the_future_says_nothing_rather_than_a_negative() {
    // Any stamp ahead of render time: a clock moving backwards within
    // the day (a timezone change, an NTP correction), or a line typed
    // into the log with a later stamp. (Not a midnight crossing: the
    // focus is read from one date's note against that same moment's
    // clock.) A negative duration would be worse than silence.
    assert_eq!(elapsed_since(t(23, 0), t(1, 0)), None);
}

#[test]
fn with_nothing_started_it_says_so_rather_than_printing_an_empty_frame() {
    let dir = vault_with_action();
    let out = build_now(dir.path(), day(), t(10, 0)).expect("builds");
    assert!(out.contains("Nothing started yet"), "empty state:\n{out}");
}

#[test]
fn a_start_shows_the_project_the_action_and_how_long() {
    let dir = vault_with_action();
    start(dir.path(), moment(9, 30), "Draft methods");

    let out = build_now(dir.path(), day(), t(11, 0)).expect("builds");
    assert!(out.contains("alpha"), "project:\n{out}");
    // The RESOLVED bullet text, energy suffix and all — not the query.
    assert!(out.contains("Draft methods (deep)"), "action:\n{out}");
    assert!(out.contains("since 09:30"), "start stamp:\n{out}");
    assert!(out.contains("1h 30m"), "elapsed:\n{out}");
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

    let out = build_now(dir.path(), day(), t(11, 30)).expect("builds");
    assert!(out.contains("Nothing started yet"), "cleared:\n{out}");
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

    let out = build_now(dir.path(), day(), t(9, 0)).expect("builds");
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
    assert_eq!(elapsed_since(t(9, 0), t(9, 0)).as_deref(), Some("just now"));
    assert_eq!(elapsed_since(t(9, 0), t(9, 1)).as_deref(), Some("1m"));
    assert_eq!(elapsed_since(t(9, 0), t(9, 59)).as_deref(), Some("59m"));
    assert_eq!(elapsed_since(t(9, 0), t(10, 0)).as_deref(), Some("1h"));
    assert_eq!(elapsed_since(t(9, 0), t(10, 1)).as_deref(), Some("1h 1m"));
}

#[test]
fn json_is_all_null_when_nothing_is_open() {
    // The documented contract: a caller tests one field without first
    // branching on the shape of the document. A bare `null` would break
    // `.project == null` for every consumer.
    let v = now_json(None);
    assert!(v.is_object(), "an object, not a bare null: {v}");
    for field in ["project", "action", "started"] {
        assert!(v[field].is_null(), "{field} must be null: {v}");
    }
}

#[test]
fn json_carries_the_logged_text_and_a_hh_mm_stamp() {
    let focus = CurrentFocus {
        project: "alpha".to_owned(),
        action: "Draft methods (deep)".to_owned(),
        started: t(9, 5),
    };
    let v = now_json(Some(&focus));
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

    let out = build_now(dir.path(), day(), t(9, 0)).expect("builds");
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

    let out = build_now(dir.path(), day(), t(9, 0)).expect("builds");
    assert!(out.contains("since 14:00"), "the stamp still shows:\n{out}");
    assert!(!out.contains('·'), "but no elapsed clause:\n{out}");
    assert!(!out.contains('-'), "and certainly no negative:\n{out}");
}

#[test]
fn a_start_seconds_in_the_future_is_also_silent() {
    // The truncation trap: `now` carries seconds, the log stamp does
    // not, so a start 1..59s ahead divides to 0 minutes. Guarding on
    // minutes would let the whole first minute read "just now" while
    // the doc promises None for any future start.
    let started = t(9, 1);
    for secs_before in [1u32, 30, 59] {
        let now = NaiveTime::from_hms_opt(9, 0, 60 - secs_before).unwrap();
        assert_eq!(
            elapsed_since(started, now),
            None,
            "{secs_before}s in the future must be silent"
        );
    }
    // And the boundary the other way still counts.
    assert_eq!(
        elapsed_since(t(9, 0), NaiveTime::from_hms_opt(9, 0, 59).unwrap()).as_deref(),
        Some("just now")
    );
}
