//! In-process tests for `commands::project::run`. Calls the run
//! dispatcher directly with explicitly constructed `ProjectCommands`
//! values, rather than spawning the binary — Linux tarpaulin can't
//! instrument subprocess code, so subprocess-only tests would leave
//! the entire dispatcher unmeasured.
//!
//! Subprocess smoke tests for clap parsing and the full lifecycle
//! still live in `tests/cli.rs`; this file owns the per-subcommand
//! coverage.

use std::fs;
use std::path::Path;

use cdno_cli::commands::action::ActionCommands;
use cdno_cli::commands::project::{
    self, MilestoneCommands, ProjectCommands, WaitingCommands, parse_iso_date,
};
use cdno_cli::commands::{action, init};
use cdno_domain::frontmatter::Context;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use tempfile::TempDir;

fn moment(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

fn vault() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    init::run(dir.path()).expect("init");
    dir
}

fn create_project(root: &Path, at: NaiveDateTime, title: &str, context: Context) {
    project::run(
        root,
        at,
        ProjectCommands::Create {
            title: title.to_owned(),
            context,
            question: None,
        },
    )
    .expect("create");
}

#[test]
fn create_writes_active_project_to_disk() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "Alpha", Context::Work);

    let path = dir.path().join("projects/alpha.md");
    assert!(path.is_file(), "project file present");
    let body = fs::read_to_string(&path).unwrap();
    assert!(body.contains("status: active"));
    assert!(body.contains("context: work"));
}

#[test]
fn create_with_question_wraps_target_in_wikilink() {
    let dir = vault();
    project::run(
        dir.path(),
        moment(2026, 5, 2, 9, 0),
        ProjectCommands::Create {
            title: "Surrogate".to_owned(),
            context: Context::Work,
            question: Some("questions/research/surrogate-cost".to_owned()),
        },
    )
    .expect("create");

    let body = fs::read_to_string(dir.path().join("projects/surrogate.md")).unwrap();
    assert!(
        body.contains("[[questions/research/surrogate-cost]]"),
        "wikilink in frontmatter:\n{body}"
    );
}

#[test]
fn state_replaces_current_state_section() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);

    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::State {
            slug: "x".to_owned(),
            text: "Updated state.".to_owned(),
        },
    )
    .expect("state");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(body.contains("Updated state."), "state present:\n{body}");
}

#[test]
fn park_moves_file_to_parked_folder() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);

    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Park {
            slug: "x".to_owned(),
        },
    )
    .expect("park");

    assert!(!dir.path().join("projects/x.md").is_file());
    assert!(dir.path().join("projects/_parked/x.md").is_file());
}

#[test]
fn activate_moves_file_back_and_flips_status() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Park {
            slug: "x".to_owned(),
        },
    )
    .expect("park");

    project::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ProjectCommands::Activate {
            slug: "x".to_owned(),
        },
    )
    .expect("activate");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(body.contains("status: active"));
}

#[test]
fn list_succeeds_with_and_without_active_projects() {
    let dir = vault();
    project::run(dir.path(), moment(2026, 5, 2, 9, 0), ProjectCommands::List)
        .expect("list (empty)");

    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "Alpha", Context::Work);
    project::run(dir.path(), moment(2026, 5, 2, 10, 0), ProjectCommands::List).expect("list (one)");
}

#[test]
fn show_succeeds_for_active_parked_and_completed() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "Alpha", Context::Work);

    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Show {
            slug: "alpha".to_owned(),
        },
    )
    .expect("show active");

    project::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ProjectCommands::Park {
            slug: "alpha".to_owned(),
        },
    )
    .expect("park");

    project::run(
        dir.path(),
        moment(2026, 5, 2, 12, 0),
        ProjectCommands::Show {
            slug: "alpha".to_owned(),
        },
    )
    .expect("show parked");

    // Hand-write a completed project to exercise the Completed
    // print_summary arm.
    let completed = "---\ntype: project\ncontext: work\nstatus: completed\ncreated: 2026-04-01\n---\n\n# Done\n\n## Current State\nShipped.\n\n## Next Actions\n\n## Waiting On\n(nothing yet)\n";
    fs::write(dir.path().join("projects/done.md"), completed).unwrap();
    project::run(
        dir.path(),
        moment(2026, 5, 2, 13, 0),
        ProjectCommands::Show {
            slug: "done".to_owned(),
        },
    )
    .expect("show completed");
}

#[test]
fn show_renders_no_open_actions_branch() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    // Complete the template's default action to leave Next Actions
    // empty, exercising the `top_action: None` branch in print_summary.
    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Complete {
            project: "x".to_owned(),
            query: "first concrete".to_owned(),
        },
    )
    .expect("action complete");

    project::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ProjectCommands::Show {
            slug: "x".to_owned(),
        },
    )
    .expect("show with no open actions");
}

#[test]
fn show_renders_state_none_branch() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::State {
            slug: "x".to_owned(),
            text: "  ".to_owned(),
        },
    )
    .expect("state");

    project::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ProjectCommands::Show {
            slug: "x".to_owned(),
        },
    )
    .expect("show with empty state");
}

#[test]
fn show_renders_top_action_without_energy_branch() {
    let dir = vault();
    let body = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# X\n\n## Current State\nFoo.\n\n## Next Actions\n- [ ] Bare\n\n## Waiting On\n(nothing yet)\n";
    fs::write(dir.path().join("projects/x.md"), body).unwrap();

    project::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ProjectCommands::Show {
            slug: "x".to_owned(),
        },
    )
    .expect("show with bare action");
}

#[test]
fn milestone_add_writes_hard_bullet() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);

    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Milestone {
            action: MilestoneCommands::Add {
                slug: "x".to_owned(),
                title: "Submit".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 5, 22).unwrap(),
                hard: true,
            },
        },
    )
    .expect("milestone add");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(body.contains("hard: 2026-05-22"));
}

#[test]
fn milestone_done_marks_with_completion_date() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Milestone {
            action: MilestoneCommands::Add {
                slug: "x".to_owned(),
                title: "Submit".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 5, 22).unwrap(),
                hard: true,
            },
        },
    )
    .expect("milestone add");

    project::run(
        dir.path(),
        moment(2026, 5, 22, 16, 0),
        ProjectCommands::Milestone {
            action: MilestoneCommands::Done {
                slug: "x".to_owned(),
                query: "Submit".to_owned(),
            },
        },
    )
    .expect("milestone done");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(body.contains("- [x] Submit"));
}

#[test]
fn waiting_add_and_resolve_round_trip() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);

    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Waiting {
            action: WaitingCommands::Add {
                slug: "x".to_owned(),
                description: "Compute allocation".to_owned(),
            },
        },
    )
    .expect("waiting add");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(body.contains("- Compute allocation"));

    project::run(
        dir.path(),
        moment(2026, 5, 2, 12, 0),
        ProjectCommands::Waiting {
            action: WaitingCommands::Resolve {
                slug: "x".to_owned(),
                query: "Compute".to_owned(),
            },
        },
    )
    .expect("waiting resolve");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(!body.contains("Compute allocation"));
}

// ---------------------------------------------------------------------
// parse_iso_date — exposed publicly because clap's value_parser path
// runs in a subprocess on the binary tests, which Linux tarpaulin
// can't instrument. Direct calls here keep the helper measured.
// ---------------------------------------------------------------------

#[test]
fn parse_iso_date_accepts_valid_yyyy_mm_dd() {
    assert_eq!(
        parse_iso_date("2026-05-22").unwrap(),
        NaiveDate::from_ymd_opt(2026, 5, 22).unwrap()
    );
}

#[test]
fn parse_iso_date_rejects_other_formats_with_helpful_message() {
    let err = parse_iso_date("May 22 2026").unwrap_err();
    assert!(err.contains("YYYY-MM-DD"), "missing format hint: {err}");
    assert!(err.contains("May 22 2026"), "missing input echo: {err}");
}
