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
            title: Some(title.to_owned()),
            context: Some(context),
            question: None,
            var: vec![],
        },
        true,
        false,
    )
    .expect("create");
}

/// Add a `[variables.prompt]` config entry and a custom project template
/// using the matching `{{name}}` placeholder, so `--var`/prompt behaviour
/// can be exercised through `project create`.
fn seed_prompt_var_template(root: &Path) {
    let config = root.join(".cuaderno/config.toml");
    let mut body = fs::read_to_string(&config).unwrap_or_default();
    body.push_str("\n[variables.prompt]\nticket = \"Ticket?\"\n");
    fs::write(&config, body).unwrap();
    fs::write(
        root.join(".cuaderno/templates/project.md"),
        "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\ncore_question: {{core_question}}\nticket: {{ticket}}\n---\n# {{title}}\n",
    )
    .unwrap();
}

#[test]
fn create_without_var_errors_when_a_prompt_variable_is_unsatisfied() {
    // Non-interactive (no_interactive = true), no `--var`: the prompted
    // template variable can't be gathered, so the command errors rather
    // than writing a note with a literal `{{ticket}}`.
    let dir = vault();
    seed_prompt_var_template(dir.path());

    let err = project::run(
        dir.path(),
        moment(2026, 5, 2, 9, 0),
        ProjectCommands::Create {
            title: Some("Alpha".to_owned()),
            context: Some(Context::Work),
            question: None,
            var: vec![],
        },
        true,
        false,
    )
    .expect_err("should error without --var");
    assert!(
        err.to_string().contains("ticket"),
        "error should name the missing variable: {err}"
    );
    assert!(
        !dir.path().join("projects/alpha.md").exists(),
        "no note should be written when the prompt is unsatisfied"
    );
}

#[test]
fn create_with_var_supplies_the_prompted_value() {
    let dir = vault();
    seed_prompt_var_template(dir.path());

    project::run(
        dir.path(),
        moment(2026, 5, 2, 9, 0),
        ProjectCommands::Create {
            title: Some("Alpha".to_owned()),
            context: Some(Context::Work),
            question: None,
            var: vec![("ticket".to_owned(), "ABC-1".to_owned())],
        },
        true,
        false,
    )
    .expect("create with --var");

    let body = fs::read_to_string(dir.path().join("projects/alpha.md")).unwrap();
    assert!(body.contains("ticket: ABC-1"), "prompted value:\n{body}");
    assert!(!body.contains("{{ticket}}"), "{body}");
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
            title: Some("Surrogate".to_owned()),
            context: Some(Context::Work),
            question: Some("questions/research/surrogate-cost".to_owned()),
            var: vec![],
        },
        true,
        false,
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
            slug: Some("x".to_owned()),
            text: Some("Updated state.".to_owned()),
        },
        true,
        false,
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
            slug: Some("x".to_owned()),
        },
        true,
        false,
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
            slug: Some("x".to_owned()),
        },
        true,
        false,
    )
    .expect("park");

    project::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ProjectCommands::Activate {
            slug: Some("x".to_owned()),
        },
        true,
        false,
    )
    .expect("activate");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(body.contains("status: active"));
}

#[test]
fn list_succeeds_with_and_without_active_projects() {
    let dir = vault();
    project::run(
        dir.path(),
        moment(2026, 5, 2, 9, 0),
        ProjectCommands::List { closed: false },
        true,
        false,
    )
    .expect("list (empty)");

    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "Alpha", Context::Work);
    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::List { closed: false },
        true,
        false,
    )
    .expect("list (one)");
}

#[test]
fn show_succeeds_for_active_parked_and_completed() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "Alpha", Context::Work);

    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Show {
            slug: Some("alpha".to_owned()),
        },
        true,
        false,
    )
    .expect("show active");

    project::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ProjectCommands::Park {
            slug: Some("alpha".to_owned()),
        },
        true,
        false,
    )
    .expect("park");

    project::run(
        dir.path(),
        moment(2026, 5, 2, 12, 0),
        ProjectCommands::Show {
            slug: Some("alpha".to_owned()),
        },
        true,
        false,
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
            slug: Some("done".to_owned()),
        },
        true,
        false,
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
            project: Some("x".to_owned()),
            query: Some("first concrete".to_owned()),
        },
        true,
        false,
    )
    .expect("action complete");

    project::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ProjectCommands::Show {
            slug: Some("x".to_owned()),
        },
        true,
        false,
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
            slug: Some("x".to_owned()),
            text: Some("  ".to_owned()),
        },
        true,
        false,
    )
    .expect("state");

    project::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ProjectCommands::Show {
            slug: Some("x".to_owned()),
        },
        true,
        false,
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
            slug: Some("x".to_owned()),
        },
        true,
        false,
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
                slug: Some("x".to_owned()),
                title: Some("Submit".to_owned()),
                date: Some(NaiveDate::from_ymd_opt(2026, 5, 22).unwrap()),
                hard: true,
            },
        },
        true,
        false,
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
                slug: Some("x".to_owned()),
                title: Some("Submit".to_owned()),
                date: Some(NaiveDate::from_ymd_opt(2026, 5, 22).unwrap()),
                hard: true,
            },
        },
        true,
        false,
    )
    .expect("milestone add");

    project::run(
        dir.path(),
        moment(2026, 5, 22, 16, 0),
        ProjectCommands::Milestone {
            action: MilestoneCommands::Done {
                slug: Some("x".to_owned()),
                query: Some("Submit".to_owned()),
            },
        },
        true,
        false,
    )
    .expect("milestone done");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(body.contains("- [x] Submit"));
}

#[test]
fn milestone_drop_removes_the_bullet_without_recording_a_completion() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Milestone {
            action: MilestoneCommands::Add {
                slug: Some("x".to_owned()),
                title: Some("Book the venue".to_owned()),
                date: Some(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()),
                hard: false,
            },
        },
        true,
        false,
    )
    .expect("milestone add");

    project::run(
        dir.path(),
        moment(2026, 5, 22, 16, 0),
        ProjectCommands::Milestone {
            action: MilestoneCommands::Drop {
                slug: Some("x".to_owned()),
                query: Some("venue".to_owned()),
                reason: Some("the funder withdrew".to_owned()),
            },
        },
        true,
        false,
    )
    .expect("milestone drop");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(!body.contains("Book the venue"), "bullet removed:\n{body}");
    assert!(!body.contains("- [x]"), "nothing is ticked done:\n{body}");

    let daily = fs::read_to_string(dir.path().join("journal/2026/daily/2026-05-22.md")).unwrap();
    assert!(
        daily.contains("milestone dropped on [[x]] \u{2014} Book the venue"),
        "the drop is logged:\n{daily}"
    );
    assert!(
        daily.contains("  reason: the funder withdrew"),
        "the reason rides a continuation line:\n{daily}"
    );
}

/// `--reason` is genuinely optional and never prompted for, so its
/// absence must not trip the missing-flag path the way `--query` does.
#[test]
fn milestone_drop_in_non_interactive_accepts_a_missing_reason() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Milestone {
            action: MilestoneCommands::Add {
                slug: Some("x".to_owned()),
                title: Some("Book the venue".to_owned()),
                date: None,
                hard: false,
            },
        },
        true,
        false,
    )
    .expect("milestone add");

    project::run(
        dir.path(),
        moment(2026, 5, 22, 16, 0),
        ProjectCommands::Milestone {
            action: MilestoneCommands::Drop {
                slug: Some("x".to_owned()),
                query: Some("venue".to_owned()),
                reason: None,
            },
        },
        true,
        false,
    )
    .expect("a drop needs no reason");

    let daily = fs::read_to_string(dir.path().join("journal/2026/daily/2026-05-22.md")).unwrap();
    assert!(!daily.contains("reason:"), "no empty reason line:\n{daily}");
}

#[test]
fn milestone_drop_in_non_interactive_errors_when_missing_query() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    let err = project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Milestone {
            action: MilestoneCommands::Drop {
                slug: Some("x".to_owned()),
                query: None,
                reason: None,
            },
        },
        true,
        false,
    )
    .expect_err("missing --query should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("--query"), "error message: {msg}");
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
                slug: Some("x".to_owned()),
                description: Some("Compute allocation".to_owned()),
            },
        },
        true,
        false,
    )
    .expect("waiting add");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(body.contains("- Compute allocation"));

    project::run(
        dir.path(),
        moment(2026, 5, 2, 12, 0),
        ProjectCommands::Waiting {
            action: WaitingCommands::Resolve {
                slug: Some("x".to_owned()),
                query: Some("Compute".to_owned()),
            },
        },
        true,
        false,
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

// ---------------------------------------------------------------------
// Non-interactive ergonomics for the retrofitted verbs (#114).
// ---------------------------------------------------------------------

#[test]
fn create_in_non_interactive_errors_when_missing_title() {
    let dir = vault();
    let err = project::run(
        dir.path(),
        moment(2026, 5, 2, 9, 0),
        ProjectCommands::Create {
            title: None,
            context: Some(Context::Work),
            question: None,
            var: vec![],
        },
        true,
        false,
    )
    .expect_err("missing --title should error in non-interactive mode");
    let msg = format!("{err:#}");
    assert!(msg.contains("--title"), "error message: {msg}");
}

#[test]
fn state_in_non_interactive_errors_when_missing_slug() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);

    let err = project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::State {
            slug: None,
            text: Some("Some state".to_owned()),
        },
        true,
        false,
    )
    .expect_err("missing --slug should error in non-interactive mode");
    let msg = format!("{err:#}");
    assert!(msg.contains("--slug"), "error message: {msg}");
}

#[test]
fn park_in_non_interactive_errors_when_missing_slug() {
    let dir = vault();
    let err = project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Park { slug: None },
        true,
        false,
    )
    .expect_err("missing --slug should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("--slug"), "error message: {msg}");
}

#[test]
fn activate_in_non_interactive_errors_when_missing_slug() {
    let dir = vault();
    let err = project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Activate { slug: None },
        true,
        false,
    )
    .expect_err("missing --slug should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("--slug"), "error message: {msg}");
}

/// #521 changed this: an omitted `--date` used to be a missing-flag
/// error, and is now a valid value (an undated, condition-gated
/// milestone). The non-interactive path must take the absence at face
/// value rather than erroring.
#[test]
fn milestone_add_in_non_interactive_accepts_a_missing_date() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Milestone {
            action: MilestoneCommands::Add {
                slug: Some("x".to_owned()),
                title: Some("All replies in".to_owned()),
                date: None,
                hard: false,
            },
        },
        true,
        false,
    )
    .expect("an undated milestone is a valid milestone");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(
        body.contains("- [ ] All replies in \u{2014} target: TBD"),
        "body:\n{body}"
    );
}

/// `--date` left the required set in #521, but `--slug` and `--title`
/// did not. Deleting the old missing-date test took the only
/// missing-flag guard this verb had with it, on the one handler that
/// now deliberately routes an argument around `gather_or_error`.
#[test]
fn milestone_add_in_non_interactive_errors_when_missing_title() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    let err = project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Milestone {
            action: MilestoneCommands::Add {
                slug: Some("x".to_owned()),
                title: None,
                date: Some(NaiveDate::from_ymd_opt(2026, 5, 22).unwrap()),
                hard: false,
            },
        },
        true,
        false,
    )
    .expect_err("missing --title should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("--title"), "error message: {msg}");
}

#[test]
fn milestone_add_rejects_hard_without_a_date() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    let err = project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Milestone {
            action: MilestoneCommands::Add {
                slug: Some("x".to_owned()),
                title: Some("Submit".to_owned()),
                date: None,
                hard: true,
            },
        },
        true,
        false,
    )
    .expect_err("a hard deadline with no date is not a thing");
    let msg = format!("{err:#}");
    assert!(msg.contains("hard"), "error message: {msg}");
}

#[test]
fn waiting_add_in_non_interactive_errors_when_missing_description() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    let err = project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::Waiting {
            action: WaitingCommands::Add {
                slug: Some("x".to_owned()),
                description: None,
            },
        },
        true,
        false,
    )
    .expect_err("missing --description should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("--description"), "error message: {msg}");
}

// ---------------------------------------------------------------------
// Rendering.
//
// `render_list` and `render_show` are pure, so these assert on the text
// directly rather than through a subprocess — which is also the only way
// tarpaulin sees them.
// ---------------------------------------------------------------------

use cdno_cli::commands::project::{render_list, render_show};
use cdno_domain::ProjectSummary;
use cdno_domain::frontmatter::ProjectStatus;

fn summary(slug: &str, context: Context, state: &str) -> ProjectSummary {
    ProjectSummary {
        slug: slug.to_owned(),
        status: ProjectStatus::Active,
        context,
        state_snippet: state.to_owned(),
        top_action: None,
        last_paused: None,
    }
}

#[test]
fn an_empty_list_says_so_in_the_house_shape() {
    // Every empty listing in the CLI is a title then an indented dim
    // parenthetical; this one used to be a bare sentence with no title,
    // no indent, and no colour.
    let out = render_list(&[]);
    assert_eq!(
        out,
        "Active projects\n  (none — create one with `cdno project create`)\n"
    );
    assert!(
        !out.contains('▎'),
        "an empty listing draws no card: {out:?}"
    );
}

#[test]
fn the_list_counts_projects_and_agrees_with_itself_on_plurals() {
    let one = render_list(&[summary("alpha", Context::Work, "state")]);
    assert!(one.starts_with("1 active project\n"), "{one}");

    let two = render_list(&[
        summary("alpha", Context::Work, "state"),
        summary("beta", Context::Personal, "state"),
    ]);
    assert!(two.starts_with("2 active projects\n"), "{two}");
}

#[test]
fn each_project_becomes_a_card_carrying_its_state() {
    let out = render_list(&[
        summary(
            "alpha",
            Context::Work,
            "Kicked off; waiting on the data drop.",
        ),
        summary("beta", Context::Family, "Venue booked."),
    ]);
    assert!(out.contains("▎ alpha"), "{out}");
    assert!(
        out.contains("▎ Kicked off; waiting on the data drop."),
        "{out}"
    );
    assert!(out.contains("▎ Venue booked."), "{out}");
    // Each card carries the project's top action; dropping the `next:`
    // line entirely used to pass the whole suite.
    assert!(
        out.lines().any(|l| l.starts_with("▎ next: ")),
        "every card needs its next action:\n{out}"
    );
    // The badge is the context, and both badges share a column.
    let alpha = out.lines().find(|l| l.contains("alpha")).unwrap();
    let beta = out.lines().find(|l| l.contains("beta")).unwrap();
    assert_eq!(alpha.find("work"), beta.find("family"), "{out}");
}

#[test]
fn a_project_with_no_state_says_so_rather_than_rendering_a_gap() {
    let out = render_list(&[summary("alpha", Context::Work, "   ")]);
    assert!(out.contains("(no state recorded)"), "{out}");
}

#[test]
fn the_list_never_leaves_trailing_whitespace() {
    // A wall of prose is what this command exists to fix; a ragged right
    // edge would undo half of it.
    let out = render_list(&[summary(
        "alpha",
        Context::Work,
        "A state long enough to wrap across more than one line of the card body, \
         so the padding path is genuinely exercised rather than skipped.",
    )]);
    assert!(!out.lines().any(|l| l.ends_with(' ')), "{out:?}");
}

#[test]
fn show_keeps_its_line_shape_rather_than_becoming_a_card() {
    // Cards are for lists. A detail view has no boundary to mark, so it
    // must not grow a gutter.
    let out = render_show(&summary("alpha", Context::Work, "Kicked off."));
    assert!(!out.contains('▎'), "show must not draw a gutter:\n{out}");
    assert!(out.starts_with("[alpha] (active)"), "{out}");
    assert!(out.contains("  State:\n    Kicked off."), "{out}");
    assert!(out.contains("  Top: (no open actions)"), "{out}");
}

#[test]
fn show_names_an_absent_state() {
    let out = render_show(&summary("alpha", Context::Work, ""));
    assert!(out.contains("  State: (none)"), "{out}");
}

#[test]
fn a_rendered_listing_actually_uses_the_context_accent() {
    // `Accent::for_context` being right is not the same as the renderer
    // using it: replacing the accent with a constant at the call site
    // left every test green, because `render_list` reads the process
    // colour gate and the suite has no terminal. `with_colour` forces the
    // gate for the length of the call so the choice is observable.
    use cdno_cli::output::style::with_colour;
    let summaries = [
        summary("alpha", Context::Work, "state one"),
        summary("beta", Context::Family, "state two"),
    ];
    let out = with_colour(true, || render_list(&summaries));

    let gutter_of = |slug: &str| -> String {
        out.lines()
            .find(|l| l.contains(slug))
            .map(|l| l.split('▎').next().unwrap_or("").to_owned())
            .expect("a card header")
    };
    assert_ne!(
        gutter_of("alpha"),
        gutter_of("beta"),
        "a work project and a family project must not share a gutter colour:\n{out}"
    );
    assert!(
        out.contains('\u{1b}'),
        "forcing colour should paint:\n{out}"
    );
}

#[test]
fn core_question_sets_then_clears_the_field() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);

    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::CoreQuestion {
            slug: Some("x".to_owned()),
            question: Some("questions/research/foo".to_owned()),
            clear: false,
        },
        true,
        false,
    )
    .expect("core-question set");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(
        body.contains("core_question: \"[[questions/research/foo]]\""),
        "body:\n{body}"
    );

    project::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ProjectCommands::CoreQuestion {
            slug: Some("x".to_owned()),
            question: None,
            clear: true,
        },
        true,
        false,
    )
    .expect("core-question clear");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(body.contains("core_question: null"), "body:\n{body}");
}

/// Neither `--question` nor `--clear` in a non-interactive run is a
/// missing-flag error, not a silent detach: dropping a project's
/// question is a decision, and must be asked for.
#[test]
fn core_question_in_non_interactive_errors_when_missing_question() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    let err = project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::CoreQuestion {
            slug: Some("x".to_owned()),
            question: None,
            clear: false,
        },
        true,
        false,
    )
    .expect_err("missing --question should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("--question"), "error message: {msg}");
}

// ---------------------------------------------------------------------
// complete / drop (RFC 0004 §6.3)
// ---------------------------------------------------------------------

/// A map with nothing open, written straight to disk (the vault
/// reconciles it on open).
fn write_map(root: &Path, rel: &str, status: &str, title: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        format!(
            "---\ntype: project\ncontext: work\nstatus: {status}\ncreated: 2026-04-01\ncore_question: null\nclosed: null\n---\n\n# {title}\n\n## Current State\nWrapping up.\n\n## Next Actions\n- [x] Submit (deep)\n\n## Milestones\n- [x] Paper submitted \u{2014} 2026-05-22\n"
        ),
    )
    .unwrap();
}

/// A map with one open bullet and one open hard milestone.
fn write_open_map(root: &Path, slug: &str) {
    fs::write(
        root.join(format!("projects/{slug}.md")),
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\ncore_question: null\nclosed: null\n---\n\n# Surrogate\n\n## Next Actions\n- [ ] Run feature set B (deep)\n\n## Milestones\n- [ ] ICML \u{2014} hard: 2026-10-22\n",
    )
    .unwrap();
}

fn daily(root: &Path, date: &str) -> String {
    fs::read_to_string(root.join(format!("journal/{}/daily/{date}.md", &date[..4])))
        .unwrap_or_default()
}

#[test]
fn complete_moves_a_map_with_nothing_open_to_done() {
    let dir = vault();
    write_map(dir.path(), "projects/surrogate.md", "active", "Surrogate");

    project::run(
        dir.path(),
        moment(2026, 9, 29, 10, 0),
        ProjectCommands::Complete {
            slug: Some("surrogate".to_owned()),
        },
        true,
        false,
    )
    .expect("complete");

    assert!(!dir.path().join("projects/surrogate.md").exists());
    let raw = fs::read_to_string(dir.path().join("projects/_done/2026/surrogate.md")).unwrap();
    assert!(raw.contains("status: completed"), "{raw}");
    assert!(raw.contains("closed: 2026-09-29"), "{raw}");
    assert!(
        daily(dir.path(), "2026-09-29")
            .contains("project completed [[surrogate]] \u{2014} Surrogate"),
        "{}",
        daily(dir.path(), "2026-09-29")
    );
}

#[test]
fn complete_in_vault_without_done_folder() {
    // A vault made before `projects/_done/` existed needs no migration.
    let dir = vault();
    fs::remove_dir_all(dir.path().join("projects/_done")).unwrap();
    write_map(dir.path(), "projects/surrogate.md", "active", "Surrogate");

    project::run(
        dir.path(),
        moment(2026, 9, 29, 10, 0),
        ProjectCommands::Complete {
            slug: Some("surrogate".to_owned()),
        },
        true,
        false,
    )
    .expect("complete");

    assert!(dir.path().join("projects/_done/2026/surrogate.md").exists());
}

#[test]
fn complete_missing_slug_non_interactive_errors_with_missing_flag() {
    let dir = vault();

    let err = project::run(
        dir.path(),
        moment(2026, 9, 29, 10, 0),
        ProjectCommands::Complete { slug: None },
        true,
        false,
    )
    .unwrap_err();

    assert!(format!("{err:#}").contains("--slug"), "{err:#}");
}

#[test]
fn drop_with_drop_open_lets_the_open_items_go() {
    let dir = vault();
    create_project(
        dir.path(),
        moment(2026, 9, 29, 9, 0),
        "Alpha",
        Context::Work,
    );

    project::run(
        dir.path(),
        moment(2026, 9, 29, 10, 0),
        ProjectCommands::Drop {
            slug: Some("alpha".to_owned()),
            reason: None,
            drop_open: true,
        },
        true,
        false,
    )
    .expect("drop --drop-open");

    let raw = fs::read_to_string(dir.path().join("projects/_done/2026/alpha.md")).unwrap();
    assert!(raw.contains("status: dropped"), "{raw}");
    assert!(!raw.contains("- [ ]"), "{raw}");
    let log = daily(dir.path(), "2026-09-29");
    assert!(
        log.contains("action dropped on [[alpha]] \u{2014} Define first concrete step (light)"),
        "{log}"
    );
    assert!(log.contains("milestone dropped on [[alpha]]"), "{log}");
    assert!(
        log.contains("project dropped on [[alpha]] \u{2014} Alpha"),
        "{log}"
    );
}

#[test]
fn drop_with_reason_writes_reason_line() {
    let dir = vault();
    write_map(dir.path(), "projects/surrogate.md", "active", "Surrogate");

    project::run(
        dir.path(),
        moment(2026, 9, 29, 10, 0),
        ProjectCommands::Drop {
            slug: Some("surrogate".to_owned()),
            reason: Some("superseded".to_owned()),
            drop_open: false,
        },
        true,
        false,
    )
    .expect("drop");

    let log = daily(dir.path(), "2026-09-29");
    assert!(
        log.contains("project dropped on [[surrogate]] \u{2014} Surrogate\n  reason: superseded"),
        "{log}"
    );
}

#[test]
fn drop_parked_project_needs_no_slot() {
    let dir = vault();
    for i in 1..=5 {
        write_map(
            dir.path(),
            &format!("projects/live-{i}.md"),
            "active",
            &format!("Live {i}"),
        );
    }
    write_map(
        dir.path(),
        "projects/_parked/shelved.md",
        "parked",
        "Shelved",
    );

    project::run(
        dir.path(),
        moment(2026, 9, 29, 10, 0),
        ProjectCommands::Drop {
            slug: Some("shelved".to_owned()),
            reason: None,
            drop_open: false,
        },
        true,
        false,
    )
    .expect("a parked project drops at the cap");

    assert!(dir.path().join("projects/_done/2026/shelved.md").exists());
    assert!(!daily(dir.path(), "2026-09-29").contains("activated"));
}

// The refusal exits the process (as `config validate` does), so these run
// the binary.

fn cdno_bin() -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd
}

#[test]
fn complete_non_interactive_lists_open_items_and_fails() {
    let dir = vault();
    write_open_map(dir.path(), "surrogate");

    let out = cdno_bin()
        .args(["--no-interactive", "--vault"])
        .arg(dir.path())
        .args(["project", "complete", "--slug", "surrogate"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(out).unwrap();
    assert!(stdout.contains("surrogate has 2 open items:"), "{stdout}");
    assert!(
        stdout.contains("  - [ ] Run feature set B (deep)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("  - [ ] ICML \u{2014} hard: 2026-10-22"),
        "{stdout}"
    );
    assert!(stdout.contains("then run again"), "{stdout}");
    assert!(dir.path().join("projects/surrogate.md").exists());
}

#[test]
fn complete_json_refusal_matches_mcp_shape() {
    let dir = vault();
    write_open_map(dir.path(), "surrogate");

    let out = cdno_bin()
        .args(["--json", "--vault"])
        .arg(dir.path())
        .args(["project", "complete", "--slug", "surrogate"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&out).expect("stdout is JSON");
    assert_eq!(value["code"], "project_has_open_items");
    assert!(value["message"].as_str().unwrap().contains("surrogate"));
    let details = &value["details"];
    assert_eq!(details["slug"], "surrogate");
    assert!(
        details["open_items_hash"]
            .as_str()
            .is_some_and(|h| !h.is_empty())
    );
    assert_eq!(details["actions"][0]["text"], "Run feature set B (deep)");
    assert_eq!(details["milestones"][0]["title"], "ICML");
    assert_eq!(details["milestones"][0]["hard"], true);
    assert_eq!(details["milestones"][0]["date"], "2026-10-22");
    assert!(
        details["untouched_commitments"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn complete_rejects_drop_open_flag() {
    let dir = vault();
    write_open_map(dir.path(), "surrogate");

    cdno_bin()
        .args(["--no-interactive", "--vault"])
        .arg(dir.path())
        .args(["project", "complete", "--slug", "surrogate", "--drop-open"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--drop-open"));
    assert!(dir.path().join("projects/surrogate.md").exists());
}

#[test]
fn drop_non_interactive_without_drop_open_lists_and_fails() {
    let dir = vault();
    write_open_map(dir.path(), "surrogate");
    let before = fs::read_to_string(dir.path().join("projects/surrogate.md")).unwrap();

    let out = cdno_bin()
        .args(["--no-interactive", "--vault"])
        .arg(dir.path())
        .args(["project", "drop", "--slug", "surrogate"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(out).unwrap();
    assert!(stdout.contains("--drop-open"), "{stdout}");
    assert_eq!(
        fs::read_to_string(dir.path().join("projects/surrogate.md")).unwrap(),
        before
    );
}

#[test]
fn drop_drop_open_succeeds_and_prints_destination() {
    let dir = vault();
    write_open_map(dir.path(), "surrogate");
    let year = chrono::Local::now().format("%Y").to_string();

    let out = cdno_bin()
        .args(["--json", "--vault"])
        .arg(dir.path())
        .args(["project", "drop", "--slug", "surrogate", "--drop-open"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&out).expect("stdout is JSON");
    assert_eq!(value["path"], format!("projects/_done/{year}/surrogate.md"));
    assert!(
        value["message"]
            .as_str()
            .unwrap()
            .contains("Let go: 1 action, 1 milestone."),
        "{value}"
    );
    assert_eq!(value["dropped_actions"][0], "Run feature set B (deep)");
    assert_eq!(value["dropped_milestones"][0], "ICML");
}

// ---------------------------------------------------------------------
// list --closed, activate from _done (RFC 0004 §6.3)
// ---------------------------------------------------------------------

fn write_closed_map(root: &Path, year: &str, slug: &str, status: &str, closed: &str) {
    let path = root.join(format!("projects/_done/{year}/{slug}.md"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        format!(
            "---\ntype: project\ncontext: work\nstatus: {status}\ncreated: 2025-01-01\ncore_question: null\nclosed: {closed}\n---\n\n# Title of {slug}\n"
        ),
    )
    .unwrap();
}

#[test]
fn list_closed_shows_outcome_and_date() {
    let entries = vec![
        cdno_domain::ClosedProjectEntry {
            slug: "shipped".to_owned(),
            title: "Shipped it".to_owned(),
            context: Context::Work,
            outcome: ProjectStatus::Completed,
            closed_on: NaiveDate::from_ymd_opt(2026, 9, 24).unwrap(),
        },
        cdno_domain::ClosedProjectEntry {
            slug: "abandoned".to_owned(),
            title: "Not happening".to_owned(),
            context: Context::Personal,
            outcome: ProjectStatus::Dropped,
            closed_on: NaiveDate::from_ymd_opt(2026, 9, 22).unwrap(),
        },
    ];

    let out = project::render_closed_list(&entries);

    assert!(out.starts_with("2 closed projects\n"), "{out}");
    assert!(out.contains("completed on 2026-09-24"), "{out}");
    assert!(out.contains("dropped on 2026-09-22"), "{out}");
    assert!(out.contains("Shipped it"), "{out}");
    assert!(
        out.find("shipped").unwrap() < out.find("abandoned").unwrap(),
        "rendered in the order given: {out}"
    );
    assert_eq!(
        project::render_closed_list(&[]),
        "Closed projects\n  (none — close one with `cdno project complete` or `cdno project drop`)\n"
    );
}

#[test]
fn list_closed_json_is_newest_first() {
    let dir = vault();
    write_closed_map(dir.path(), "2025", "older", "completed", "2025-11-01");
    write_closed_map(dir.path(), "2026", "newer", "dropped", "2026-03-01");

    let out = cdno_bin()
        .args(["--json", "--vault"])
        .arg(dir.path())
        .args(["project", "list", "--closed"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let rows: serde_json::Value = serde_json::from_slice(&out).expect("stdout is JSON");
    assert_eq!(rows[0]["slug"], "newer");
    assert_eq!(rows[0]["outcome"], "dropped");
    assert_eq!(rows[0]["closed_on"], "2026-03-01");
    assert_eq!(rows[0]["title"], "Title of newer");
    assert_eq!(rows[1]["slug"], "older");
    assert_eq!(rows[1]["outcome"], "completed");
}

#[test]
fn activate_from_done_works() {
    let dir = vault();
    write_closed_map(dir.path(), "2025", "old", "dropped", "2025-11-01");

    project::run(
        dir.path(),
        moment(2026, 9, 29, 10, 0),
        ProjectCommands::Activate {
            slug: Some("old".to_owned()),
        },
        true,
        false,
    )
    .expect("activate from _done");

    let raw = fs::read_to_string(dir.path().join("projects/old.md")).unwrap();
    assert!(raw.contains("status: active"), "{raw}");
    assert!(raw.contains("closed: null"), "{raw}");
    assert!(!dir.path().join("projects/_done/2025/old.md").exists());
}

#[test]
fn project_list_json_does_not_expose_last_paused() {
    // RFC 0005 T9 adds last_paused to ProjectSummary, but it is internal
    // and should not appear in CLI JSON output. Exposure in MCP and CLI
    // are decided later in RFC 0005. This test pins that last_paused is not
    // in `cdno project list --json` output, even with a genuinely unconsumed
    // pause logged.
    let dir = vault();
    fs::write(
        dir.path().join("projects/test-proj.md"),
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Test\n\n## Current State\nActive.\n",
    )
    .unwrap();
    // Add a paused action to the daily log. No later start/resume/done/drop
    // of the same text, so the pause remains genuinely unconsumed.
    let daily = format!("{}/journal/2026/daily/2026-09-29.md", dir.path().display());
    fs::write(
        &daily,
        "---\ndate: 2026-09-29\ntype: daily\n---\n\n# 2026-09-29\n\n## Logs\n\
         - **14:15**: action paused on [[test-proj]] — Some task\n  \
         next: Resume later\n",
    )
    .unwrap();

    let out = cdno_bin()
        .args(["--json", "--vault"])
        .arg(dir.path())
        .args(["project", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let rows: serde_json::Value = serde_json::from_slice(&out).expect("stdout is JSON");
    if let Some(arr) = rows.as_array() {
        for proj in arr {
            assert!(
                !proj.get("last_paused").is_some(),
                "ProjectSummary.last_paused must not be in JSON: {proj}"
            );
        }
    }
}
