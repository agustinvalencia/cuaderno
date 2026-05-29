//! In-process tests for `commands::action::run`. Calls the dispatcher
//! directly — Linux tarpaulin can't instrument subprocess code, so
//! direct dispatch is the only way to keep coverage honest.

use std::fs;
use std::path::Path;

use cdno_cli::commands::action::{self, ActionCommands};
use cdno_cli::commands::init;
use cdno_cli::commands::project::{self, ProjectCommands};
use cdno_domain::frontmatter::{Context, EnergyLevel};
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
    .expect("create project");
}

// ---------------------------------------------------------------------
// add (plain bullet)
// ---------------------------------------------------------------------

#[test]
fn add_appends_open_bullet_with_energy() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);

    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Add {
            project: "x".to_owned(),
            title: "Run ablation".to_owned(),
            energy: EnergyLevel::Deep,
            note: false,
        },
    )
    .expect("action add");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(
        body.contains("- [ ] Run ablation (deep)"),
        "bullet:\n{body}"
    );
    // No action note was created.
    assert!(!dir.path().join("actions/run-ablation.md").exists());
}

// ---------------------------------------------------------------------
// add --note
// ---------------------------------------------------------------------

#[test]
fn add_with_note_writes_note_and_wikilink_bullet() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);

    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Add {
            project: "x".to_owned(),
            title: "Characterise sample efficiency".to_owned(),
            energy: EnergyLevel::Deep,
            note: true,
        },
    )
    .expect("action add --note");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(
        body.contains("- [ ] [[actions/characterise-sample-efficiency]] (deep)"),
        "wikilink bullet:\n{body}"
    );
    let note = fs::read_to_string(dir.path().join("actions/characterise-sample-efficiency.md"))
        .expect("action note exists");
    assert!(note.contains("type: action"));
    assert!(note.contains("status: active"));
    assert!(note.contains("project: x"));
    assert!(note.contains("energy: deep"));
}

// ---------------------------------------------------------------------
// promote
// ---------------------------------------------------------------------

#[test]
fn promote_attaches_note_to_existing_bullet() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    // Seed a plain bullet via `add`.
    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Add {
            project: "x".to_owned(),
            title: "Draft methods section".to_owned(),
            energy: EnergyLevel::Deep,
            note: false,
        },
    )
    .unwrap();

    action::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ActionCommands::Promote {
            project: "x".to_owned(),
            query: "draft methods".to_owned(),
        },
    )
    .expect("promote");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(
        body.contains("- [ ] [[actions/draft-methods-section]] (deep)"),
        "bullet rewritten:\n{body}"
    );
    assert!(
        !body.contains("- [ ] Draft methods section (deep)"),
        "plain bullet gone:\n{body}"
    );
    assert!(
        dir.path()
            .join("actions/draft-methods-section.md")
            .is_file()
    );
}

// ---------------------------------------------------------------------
// complete (plain and wikilinked round-trip)
// ---------------------------------------------------------------------

#[test]
fn complete_removes_matching_plain_bullet() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Add {
            project: "x".to_owned(),
            title: "Run ablation".to_owned(),
            energy: EnergyLevel::Deep,
            note: false,
        },
    )
    .expect("add");

    action::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ActionCommands::Complete {
            project: "x".to_owned(),
            query: "ablation".to_owned(),
        },
    )
    .expect("complete");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(!body.contains("- [ ] Run ablation"), "matched bullet gone");
}

#[test]
fn complete_on_wikilink_bullet_archives_the_note() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Add {
            project: "x".to_owned(),
            title: "Characterise sample efficiency".to_owned(),
            energy: EnergyLevel::Deep,
            note: true,
        },
    )
    .unwrap();

    action::run(
        dir.path(),
        moment(2026, 5, 3, 17, 0),
        ActionCommands::Complete {
            project: "x".to_owned(),
            query: "characterise".to_owned(),
        },
    )
    .expect("complete");

    assert!(
        !dir.path()
            .join("actions/characterise-sample-efficiency.md")
            .exists(),
        "active note moved",
    );
    let done = dir
        .path()
        .join("actions/_done/2026/characterise-sample-efficiency.md");
    let raw = fs::read_to_string(&done).expect("archived note exists");
    assert!(raw.contains("status: completed"));
    assert!(raw.contains("completed: 2026-05-03"));
}

#[test]
fn complete_errors_when_action_not_found() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);

    let err = action::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ActionCommands::Complete {
            project: "x".to_owned(),
            query: "nothing-like-this".to_owned(),
        },
    )
    .expect_err("query should not match");
    assert!(format!("{err:#}").contains("nothing-like-this"));
}

// ---------------------------------------------------------------------
// list
// ---------------------------------------------------------------------

#[test]
fn list_renders_plain_and_attached_bullets_with_status() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    // First, complete the template's default action so list starts
    // from a known state.
    action::run(
        dir.path(),
        moment(2026, 5, 2, 9, 30),
        ActionCommands::Complete {
            project: "x".to_owned(),
            query: "first concrete".to_owned(),
        },
    )
    .unwrap();
    // Add one plain bullet and one wikilink bullet.
    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Add {
            project: "x".to_owned(),
            title: "Run ablation".to_owned(),
            energy: EnergyLevel::Deep,
            note: false,
        },
    )
    .unwrap();
    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 5),
        ActionCommands::Add {
            project: "x".to_owned(),
            title: "Characterise sample efficiency".to_owned(),
            energy: EnergyLevel::Medium,
            note: true,
        },
    )
    .unwrap();

    // The `list` command prints; we render the same data via the pure
    // helper to assert content without capturing stdout.
    let (vault_obj, _r) = cdno_cli::bootstrap::open_vault(dir.path()).expect("open");
    let entries = vault_obj.list_actions("x").expect("list");
    let out = action::render_list("x", &entries);

    assert!(out.contains("Actions for projects/x.md"), "header:\n{out}");
    assert!(
        out.contains("- Run ablation (deep)"),
        "plain bullet:\n{out}"
    );
    assert!(
        out.contains("- [[actions/characterise-sample-efficiency]] (medium)  [active]"),
        "wikilink bullet with status:\n{out}",
    );
}

#[test]
fn list_on_empty_section_shows_placeholder() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    // Complete the template's default action.
    action::run(
        dir.path(),
        moment(2026, 5, 2, 9, 30),
        ActionCommands::Complete {
            project: "x".to_owned(),
            query: "first concrete".to_owned(),
        },
    )
    .unwrap();

    let (vault_obj, _r) = cdno_cli::bootstrap::open_vault(dir.path()).expect("open");
    let entries = vault_obj.list_actions("x").expect("list");
    let out = action::render_list("x", &entries);
    assert!(out.contains("(no open actions)"), "placeholder:\n{out}");
}
