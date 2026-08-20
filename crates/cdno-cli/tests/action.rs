//! In-process tests for `commands::action::run`. Calls the dispatcher
//! directly — Linux tarpaulin can't instrument subprocess code, so
//! direct dispatch is the only way to keep coverage honest.
//!
//! All tests pass `no_interactive = true` so prompts never fire: the
//! ergonomics convention only kicks in when at least one promptable
//! field is `None`, and these tests always provide every field
//! explicitly. That mirrors the agentic (MCP / Tauri) shape, which
//! also supplies full args at the transport boundary.

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
            title: Some(title.to_owned()),
            context: Some(context),
            question: None,
            var: vec![],
        },
        true,
        false,
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
            project: Some("x".to_owned()),
            title: Some("Run ablation".to_owned()),
            energy: Some(EnergyLevel::Deep),
            note: false,
            var: vec![],
        },
        true,
        false,
    )
    .expect("action add");

    let body = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(
        body.contains("- [ ] Run ablation (deep)"),
        "bullet:\n{body}"
    );
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
            project: Some("x".to_owned()),
            title: Some("Characterise sample efficiency".to_owned()),
            energy: Some(EnergyLevel::Deep),
            note: true,
            var: vec![],
        },
        true,
        false,
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
    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Add {
            project: Some("x".to_owned()),
            title: Some("Draft methods section".to_owned()),
            energy: Some(EnergyLevel::Deep),
            note: false,
            var: vec![],
        },
        true,
        false,
    )
    .unwrap();

    action::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ActionCommands::Promote {
            project: Some("x".to_owned()),
            query: Some("draft methods".to_owned()),
            var: vec![],
        },
        true,
        false,
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
            project: Some("x".to_owned()),
            title: Some("Run ablation".to_owned()),
            energy: Some(EnergyLevel::Deep),
            note: false,
            var: vec![],
        },
        true,
        false,
    )
    .expect("add");

    action::run(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ActionCommands::Complete {
            project: Some("x".to_owned()),
            query: Some("ablation".to_owned()),
        },
        true,
        false,
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
            project: Some("x".to_owned()),
            title: Some("Characterise sample efficiency".to_owned()),
            energy: Some(EnergyLevel::Deep),
            note: true,
            var: vec![],
        },
        true,
        false,
    )
    .unwrap();

    action::run(
        dir.path(),
        moment(2026, 5, 3, 17, 0),
        ActionCommands::Complete {
            project: Some("x".to_owned()),
            query: Some("characterise".to_owned()),
        },
        true,
        false,
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
            project: Some("x".to_owned()),
            query: Some("nothing-like-this".to_owned()),
        },
        true,
        false,
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
    action::run(
        dir.path(),
        moment(2026, 5, 2, 9, 30),
        ActionCommands::Complete {
            project: Some("x".to_owned()),
            query: Some("first concrete".to_owned()),
        },
        true,
        false,
    )
    .unwrap();
    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Add {
            project: Some("x".to_owned()),
            title: Some("Run ablation".to_owned()),
            energy: Some(EnergyLevel::Deep),
            note: false,
            var: vec![],
        },
        true,
        false,
    )
    .unwrap();
    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 5),
        ActionCommands::Add {
            project: Some("x".to_owned()),
            title: Some("Characterise sample efficiency".to_owned()),
            energy: Some(EnergyLevel::Medium),
            note: true,
            var: vec![],
        },
        true,
        false,
    )
    .unwrap();

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
    action::run(
        dir.path(),
        moment(2026, 5, 2, 9, 30),
        ActionCommands::Complete {
            project: Some("x".to_owned()),
            query: Some("first concrete".to_owned()),
        },
        true,
        false,
    )
    .unwrap();

    let (vault_obj, _r) = cdno_cli::bootstrap::open_vault(dir.path()).expect("open");
    let entries = vault_obj.list_actions("x").expect("list");
    let out = action::render_list("x", &entries);
    assert!(out.contains("(no open actions)"), "placeholder:\n{out}");
}

// ---------------------------------------------------------------------
// Non-interactive ergonomics: missing required flag errors clearly.
// ---------------------------------------------------------------------

#[test]
fn add_without_project_in_non_interactive_errors() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);

    let err = action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Add {
            project: None,
            title: Some("Run ablation".to_owned()),
            energy: Some(EnergyLevel::Deep),
            note: false,
            var: vec![],
        },
        true,
        false,
    )
    .expect_err("missing --project should error in non-interactive mode");
    let msg = format!("{err:#}");
    assert!(msg.contains("--project"), "error message: {msg}");
}

#[test]
fn action_statuses_are_distinguishable_in_the_rendered_listing() {
    // Goes through `render_list`, which is the point: the previous
    // version of this test re-derived the mapping by hand and asserted
    // that `Palette` distinguishes three roles it was handed, so
    // collapsing all three statuses to one role at the call site left it
    // green.
    use cdno_cli::output::style::Palette;
    let dir = vault();
    let at = moment(2026, 5, 1, 9, 0);
    create_project(dir.path(), at, "X", Context::Work);
    for (title, promote) in [("blocked one", true), ("plain one", false)] {
        action::run(
            dir.path(),
            at,
            ActionCommands::Add {
                project: Some("x".to_owned()),
                title: Some(title.to_owned()),
                energy: Some(EnergyLevel::Deep),
                note: promote,
                var: vec![],
            },
            true,
            false,
        )
        .unwrap();
    }
    let (vault_obj, _r) = cdno_cli::bootstrap::open_vault(dir.path()).expect("open");
    let entries = vault_obj.list_actions("x").expect("list");
    let out = action::render_list("x", &entries);

    // With colour off the status text still distinguishes them; the
    // colour mapping is asserted against the palette that produced it.
    assert!(
        out.contains("[active]"),
        "an attached action shows a status:\n{out}"
    );
    // Read the roles the renderer actually uses, rather than restating
    // them here — restating is what let all three collapse to one role
    // while this test stayed green.
    use cdno_cli::commands::action::status_role;
    use cdno_domain::frontmatter::ActionStatus;
    let palette = Palette::forced();
    let active = palette.paint(status_role(ActionStatus::Active), "[active]");
    let blocked = palette.paint(status_role(ActionStatus::Blocked), "[blocked]");
    let completed = palette.paint(status_role(ActionStatus::Completed), "[completed]");
    assert_ne!(sgr_of(&active), sgr_of(&blocked));
    assert_ne!(sgr_of(&active), sgr_of(&completed));
    assert_ne!(sgr_of(&blocked), sgr_of(&completed));
}

/// The SGR parameters of `text`, with visible characters removed, so two
/// strings compare equal only when styled identically.
fn sgr_of(text: &str) -> String {
    text.split('\u{1b}')
        .skip(1)
        .filter_map(|c| c.strip_prefix('[').and_then(|c| c.split('m').next()))
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn an_action_bullet_cannot_drive_the_terminal() {
    use cdno_domain::ActionListEntry;
    let entries = vec![ActionListEntry {
        text: "safe\ttab\u{1b}[41mRED\u{1b}[2J tail".to_owned(),
        energy: None,
        attached: None,
    }];
    let out = action::render_list("x", &entries);
    assert!(!out.contains('\u{1b}'), "escape survived: {out:?}");
    assert!(!out.contains('\t'), "tab survived: {out:?}");
    assert!(out.contains("RED"), "content was dropped: {out}");
}

#[test]
fn an_empty_action_listing_hugs_its_title() {
    // Every other empty state hugs; the blank line separates a title
    // from content, and there is none.
    let out = action::render_list("x", &[]);
    assert!(
        out.starts_with("Actions for projects/x.md\n  ("),
        "empty listing should hug its title: {out:?}"
    );
}
