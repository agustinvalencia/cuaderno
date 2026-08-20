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
        ProjectCommands::List,
        true,
        false,
    )
    .expect("list (empty)");

    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "Alpha", Context::Work);
    project::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ProjectCommands::List,
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

#[test]
fn milestone_add_in_non_interactive_errors_when_missing_date() {
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
                hard: false,
            },
        },
        true,
        false,
    )
    .expect_err("missing --date should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("--date"), "error message: {msg}");
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
