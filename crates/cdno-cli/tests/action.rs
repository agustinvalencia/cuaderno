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
use predicates::prelude::PredicateBooleanExt;
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
    let dropped = palette.paint(status_role(ActionStatus::Dropped), "[dropped]");
    assert_ne!(sgr_of(&active), sgr_of(&blocked));
    assert_ne!(sgr_of(&active), sgr_of(&completed));
    assert_ne!(sgr_of(&blocked), sgr_of(&completed));
    // A drop must not read as a success. Nothing was achieved, and
    // colouring it like a completion is the same false claim in a
    // different medium.
    assert_ne!(
        sgr_of(&dropped),
        sgr_of(&completed),
        "a dropped action must not be styled as a completion"
    );
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

#[test]
fn a_rendered_listing_actually_uses_the_status_role() {
    // Same shape: `status_role` being correct does not prove
    // `render_list` calls it. Collapsing the three statuses at the call
    // site survived every other test in this file.
    use cdno_cli::output::style::with_colour;
    use cdno_domain::ActionListEntry;
    use cdno_domain::frontmatter::ActionStatus;

    let entry = |text: &str, status: ActionStatus| ActionListEntry {
        text: text.to_owned(),
        energy: None,
        attached: Some(cdno_domain::AttachedAction {
            slug: text.to_owned(),
            status,
        }),
    };
    let entries = vec![
        entry("blocked one", ActionStatus::Blocked),
        entry("done one", ActionStatus::Completed),
        entry("active one", ActionStatus::Active),
    ];
    let out = with_colour(true, || action::render_list("x", &entries));
    let styling_of = |needle: &str| -> String {
        let line = out.lines().find(|l| l.contains(needle)).expect("a bullet");
        sgr_of(line)
    };
    assert_ne!(styling_of("blocked one"), styling_of("active one"));
    assert_ne!(styling_of("done one"), styling_of("active one"));
    assert_ne!(styling_of("blocked one"), styling_of("done one"));
}

/// #559: the whole point is that the daily log must not claim the work
/// was done.
#[test]
fn drop_logs_a_drop_not_a_completion() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Add {
            project: Some("x".to_owned()),
            title: Some("Prepare the demo proposal".to_owned()),
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
        ActionCommands::Drop {
            project: Some("x".to_owned()),
            query: Some("demo proposal".to_owned()),
            reason: Some("superseded by the demo-planning action".to_owned()),
        },
        true,
        false,
    )
    .expect("drop");

    let project = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(
        !project.contains("Prepare the demo proposal"),
        "bullet not removed:\n{project}"
    );

    let daily = fs::read_to_string(dir.path().join("journal/2026/daily/2026-05-02.md")).unwrap();
    assert!(
        daily.contains("action dropped on [[x]]"),
        "drop entry missing:\n{daily}"
    );
    assert!(
        daily.contains("reason: superseded by the demo-planning action"),
        "reason missing:\n{daily}"
    );
    assert!(
        !daily.contains("action done on"),
        "the vault must not assert work that never happened:\n{daily}"
    );
}

#[test]
fn drop_in_non_interactive_errors_when_missing_query() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    let err = action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Drop {
            project: Some("x".to_owned()),
            query: None,
            reason: None,
        },
        true,
        false,
    )
    .expect_err("missing --query should error");
    assert!(format!("{err:#}").contains("--query"), "{err:#}");
}

#[test]
fn drop_in_non_interactive_errors_when_missing_project() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    let err = action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Drop {
            project: None,
            query: Some("anything".to_owned()),
            reason: None,
        },
        true,
        false,
    )
    .expect_err("missing --project should error");
    assert!(format!("{err:#}").contains("--project"), "{err:#}");
}

// --- start (#568) ------------------------------------------------------

#[test]
fn start_logs_the_resolved_bullet_not_the_query() {
    // The query may be energy-stripped; what lands in the log must be
    // the whole bullet, because that is what `complete_action` will
    // later write and `current_focus` pairs them by exact text.
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    action::run(
        dir.path(),
        moment(2026, 5, 2, 9, 30),
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
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Start {
            project: Some("x".to_owned()),
            query: Some("Run ablation".to_owned()),
            unplanned: false,
            title: None,
            energy: None,
        },
        true,
        false,
    )
    .expect("start");

    let daily = fs::read_to_string(dir.path().join("journal/2026/daily/2026-05-02.md")).unwrap();
    assert!(
        daily.contains("started [[x]] \u{2014} Run ablation (deep)"),
        "the resolved bullet, energy and all:\n{daily}"
    );
}

#[test]
fn start_refuses_an_action_that_is_not_on_the_map() {
    // The #568 change: a start names a bullet. Free text could be
    // started and never closed, leaving the focus pinned for ever.
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);

    let err = action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Start {
            project: Some("x".to_owned()),
            query: Some("Buy milk".to_owned()),
            unplanned: false,
            title: None,
            energy: None,
        },
        true,
        false,
    )
    .expect_err("a start that names nothing must fail");
    assert!(
        format!("{err:#}").contains("no action matching"),
        "not-found, not a silent log: {err:#}"
    );
}

#[test]
fn an_ambiguous_start_lists_its_candidates_readably() {
    // `AmbiguousAction` carries the candidates as a Vec<String>. Left
    // to anyhow they reach the user as a Rust debug vec. All four verbs
    // that resolve a bullet by substring now unpack them instead --
    // `start` first, then complete, drop and promote, which share the
    // same helper; the three cases below are theirs.
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    for title in ["Run sweep B", "Run sweep C"] {
        action::run(
            dir.path(),
            moment(2026, 5, 2, 9, 30),
            ActionCommands::Add {
                project: Some("x".to_owned()),
                title: Some(title.to_owned()),
                energy: Some(EnergyLevel::Deep),
                note: false,
                var: vec![],
            },
            true,
            false,
        )
        .expect("add");
    }

    let err = action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Start {
            project: Some("x".to_owned()),
            query: Some("Run sweep".to_owned()),
            unplanned: false,
            title: None,
            energy: None,
        },
        true,
        false,
    )
    .expect_err("ambiguous");

    let shown = format!("{err:#}");
    assert!(shown.contains("Run sweep B"), "candidate listed:\n{shown}");
    assert!(shown.contains("Run sweep C"), "candidate listed:\n{shown}");
    assert!(
        !shown.contains("[\""),
        "candidates must not arrive as a Rust debug vec:\n{shown}"
    );
}

#[test]
fn unplanned_start_adds_the_bullet_and_starts_it() {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);

    action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Start {
            project: Some("x".to_owned()),
            query: None,
            unplanned: true,
            title: Some("Fix the CI badge".to_owned()),
            energy: Some(EnergyLevel::Light),
        },
        true,
        false,
    )
    .expect("unplanned start");

    let map = fs::read_to_string(dir.path().join("projects/x.md")).unwrap();
    assert!(
        map.contains("- [ ] Fix the CI badge (light)"),
        "map:\n{map}"
    );
    let daily = fs::read_to_string(dir.path().join("journal/2026/daily/2026-05-02.md")).unwrap();
    assert!(
        daily.contains("action added to [[x]] \u{2014} Fix the CI badge (light)"),
        "origin logged:\n{daily}"
    );
    assert!(
        daily.contains("started [[x]] \u{2014} Fix the CI badge (light)"),
        "start logged:\n{daily}"
    );
}

#[test]
fn identical_bullets_still_report_readably_rather_than_a_debug_vec() {
    // `action add` allows byte-identical bullets, and then the domain's
    // exact-match tiebreak sees TWO exact matches, declines, and the
    // substring rule re-ambiguates. The interactive branch re-queries by
    // the chosen text, so it hits that second error — which must land in
    // the readable message, not escape through anyhow as a debug vec.
    // (Non-interactive here; the assertion is on the message shape both
    // exits now share.)
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    for _ in 0..2 {
        action::run(
            dir.path(),
            moment(2026, 5, 2, 9, 30),
            ActionCommands::Add {
                project: Some("x".to_owned()),
                title: Some("Dup task".to_owned()),
                energy: Some(EnergyLevel::Deep),
                note: false,
                var: vec![],
            },
            true,
            false,
        )
        .expect("add");
    }

    let err = action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Start {
            project: Some("x".to_owned()),
            query: Some("Dup task".to_owned()),
            unplanned: false,
            title: None,
            energy: None,
        },
        true,
        false,
    )
    .expect_err("two identical bullets are ambiguous");

    let shown = format!("{err:#}");
    assert!(
        !shown.contains("[\""),
        "no Rust debug vec on any ambiguity exit:\n{shown}"
    );
    assert!(
        shown.contains("Dup task (deep)"),
        "candidates listed:\n{shown}"
    );
}

#[test]
fn the_picker_re_entry_reports_readably_when_the_choice_is_still_ambiguous() {
    // The interactive half of the ambiguity fix, reachable without a
    // pty. Round 1 found that the re-entrant call leaked the debug vec
    // when two bullets carry identical text; round 3 found the fix
    // itself was unpinned -- deleting it left the whole suite green.
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    for _ in 0..2 {
        action::run(
            dir.path(),
            moment(2026, 5, 2, 9, 30),
            ActionCommands::Add {
                project: Some("x".to_owned()),
                title: Some("Dup task".to_owned()),
                energy: Some(EnergyLevel::Deep),
                note: false,
                var: vec![],
            },
            true,
            false,
        )
        .expect("add");
    }
    let (vault_handle, _report) = cdno_cli::bootstrap::open_vault(dir.path()).expect("open");
    let candidates = vec!["Dup task (deep)".to_owned(), "Dup task (deep)".to_owned()];

    let at = moment(2026, 5, 2, 10, 0);
    let err = cdno_cli::commands::action::resolve_chosen(
        "x",
        "Dup task (deep)",
        &candidates,
        "starting action",
        |q| vault_handle.start_action(at, "x", q),
    )
    .expect_err("the picked candidate is still ambiguous");

    let shown = format!("{err:#}");
    assert!(
        !shown.contains("[\""),
        "the re-entry must not leak the debug vec:\n{shown}"
    );
    assert!(
        shown.contains("Dup task (deep)"),
        "candidates listed:\n{shown}"
    );
}

#[test]
fn an_ambiguous_candidate_cannot_drive_the_terminal() {
    // The candidates are note-derived bullet text. The debug vec this
    // listing replaced escaped control characters as a side effect of
    // `{:?}`, so printing them plainly would have been a regression:
    // the one path that exists to make the error readable would be the
    // one path that lets a bullet repaint the terminal. Same rule as
    // an_action_bullet_cannot_drive_the_terminal, which guards the
    // listing renderer.
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    for suffix in ["one", "two"] {
        action::run(
            dir.path(),
            moment(2026, 5, 2, 9, 30),
            ActionCommands::Add {
                project: Some("x".to_owned()),
                title: Some(format!("review draft \u{1b}[31mRED\u{1b}[0m {suffix}")),
                energy: Some(EnergyLevel::Deep),
                note: false,
                var: vec![],
            },
            true,
            false,
        )
        .expect("add");
    }

    let err = action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Start {
            project: Some("x".to_owned()),
            query: Some("review draft".to_owned()),
            unplanned: false,
            title: None,
            energy: None,
        },
        true,
        false,
    )
    .expect_err("ambiguous");

    let shown = format!("{err:#}");
    assert!(
        !shown.contains('\u{1b}'),
        "no ESC reaches the terminal from a candidate:\n{shown:?}"
    );
    assert!(
        shown.contains("review draft"),
        "the readable text still survives:\n{shown}"
    );
}

/// `complete`, `drop` and `promote` resolve through the same matcher as
/// `start` and have always been able to raise `AmbiguousAction`, but each
/// handed it to anyhow, so the candidates reached the user as a Rust debug
/// vec. #588 routed only `start` through the readable message and said so;
/// this closes the gap. Non-interactive, which is the exit a script and a
/// piped terminal both take.
fn vault_with_two_identical_bullets() -> TempDir {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    for _ in 0..2 {
        action::run(
            dir.path(),
            moment(2026, 5, 2, 9, 30),
            ActionCommands::Add {
                project: Some("x".to_owned()),
                title: Some("Sweep run".to_owned()),
                energy: Some(EnergyLevel::Deep),
                note: false,
                var: vec![],
            },
            true,
            false,
        )
        .expect("add");
    }
    dir
}

fn assert_readable_ambiguity(err: anyhow::Error, verb: &str) {
    let shown = format!("{err:#}");
    assert!(
        !shown.contains("[\""),
        "{verb} must not print the debug vec:\n{shown}"
    );
    assert!(
        shown.contains("Sweep run (deep)"),
        "{verb} must list the candidates:\n{shown}"
    );
}

#[test]
fn complete_reports_an_ambiguous_query_readably() {
    let dir = vault_with_two_identical_bullets();
    let err = action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Complete {
            project: Some("x".to_owned()),
            query: Some("Sweep".to_owned()),
        },
        false,
        false,
    )
    .expect_err("ambiguous");
    assert_readable_ambiguity(err, "complete");
}

#[test]
fn drop_reports_an_ambiguous_query_readably() {
    let dir = vault_with_two_identical_bullets();
    let err = action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Drop {
            project: Some("x".to_owned()),
            query: Some("Sweep".to_owned()),
            reason: None,
        },
        false,
        false,
    )
    .expect_err("ambiguous");
    assert_readable_ambiguity(err, "drop");
}

#[test]
fn promote_reports_an_ambiguous_query_readably() {
    let dir = vault_with_two_identical_bullets();
    let err = action::run(
        dir.path(),
        moment(2026, 5, 2, 10, 0),
        ActionCommands::Promote {
            project: Some("x".to_owned()),
            query: Some("Sweep".to_owned()),
            var: vec![],
        },
        false,
        false,
    )
    .expect_err("ambiguous");
    assert_readable_ambiguity(err, "promote");
}

/// The per-verb anyhow context survives the shared helper.
///
/// `resolving_ambiguity` bails on `AmbiguousAction` *before* applying
/// `context`, so the ambiguity tests above cannot catch a copy/paste swap
/// between verbs -- and the helper takes the string as a parameter, which
/// is exactly the kind of argument that gets pasted wrong. A query that
/// matches nothing takes the `Err(e) => Err(e).context(context)` arm, so
/// it is the path that pins it. The CHANGELOG makes this a headline
/// claim ("completing action" must not become a generic "resolving
/// action"), so it needs an assertion behind it.
#[test]
fn each_verb_keeps_its_own_error_context() {
    for (verb, command, expected) in [
        (
            "complete",
            ActionCommands::Complete {
                project: Some("x".to_owned()),
                query: Some("nothing matches this".to_owned()),
            },
            "completing action",
        ),
        (
            "drop",
            ActionCommands::Drop {
                project: Some("x".to_owned()),
                query: Some("nothing matches this".to_owned()),
                reason: None,
            },
            "dropping action",
        ),
        (
            "promote",
            ActionCommands::Promote {
                project: Some("x".to_owned()),
                query: Some("nothing matches this".to_owned()),
                var: vec![],
            },
            "promoting action",
        ),
        (
            "start",
            ActionCommands::Start {
                project: Some("x".to_owned()),
                query: Some("nothing matches this".to_owned()),
                unplanned: false,
                title: None,
                energy: None,
            },
            "starting action",
        ),
    ] {
        let dir = vault_with_two_identical_bullets();
        let err = action::run(dir.path(), moment(2026, 5, 2, 10, 0), command, false, false)
            .expect_err("no bullet matches");
        let shown = format!("{err:#}");
        assert!(
            shown.contains(expected),
            "{verb} must keep its own context `{expected}`:\n{shown}"
        );
    }
}

// --- pause / resume (RFC 0005, T12) --------------------------------------

/// A vault with project `x` and one open bullet, "Run ablation".
fn vault_with_bullet() -> TempDir {
    let dir = vault();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "X", Context::Work);
    add_bullet(dir.path(), "x", "Run ablation");
    dir
}

fn add_bullet(root: &Path, project: &str, title: &str) {
    action::run(
        root,
        moment(2026, 5, 2, 9, 30),
        ActionCommands::Add {
            project: Some(project.to_owned()),
            title: Some(title.to_owned()),
            energy: Some(EnergyLevel::Deep),
            note: false,
            var: vec![],
        },
        true,
        false,
    )
    .expect("add");
}

fn run_action(root: &Path, at: NaiveDateTime, command: ActionCommands) -> anyhow::Result<()> {
    action::run(root, at, command, true, false)
}

fn start(root: &Path, at: NaiveDateTime, project: &str, query: &str) {
    run_action(
        root,
        at,
        ActionCommands::Start {
            project: Some(project.to_owned()),
            query: Some(query.to_owned()),
            unplanned: false,
            title: None,
            energy: None,
        },
    )
    .expect("start");
}

/// The binary against `root`, never inheriting the developer's vault.
fn cdno_in(root: &Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd.arg("--vault").arg(root);
    cmd
}

/// The daily note a verb reports in its `logged to <path>` line, so no
/// test recomputes "today" against the binary's own clock.
fn logged_daily(root: &Path, stdout: &[u8]) -> String {
    let out = String::from_utf8_lossy(stdout);
    let path = out
        .lines()
        .find_map(|l| l.split("logged to ").nth(1))
        .unwrap_or_else(|| panic!("no `logged to` line in {out:?}"))
        .trim();
    fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn pause_prompts_once_for_next_and_never_confirms() {
    // The suite has no pty helper, so the question is tested at the seam
    // the handler routes it through: `pause_hint` is the whole of the
    // prompt logic, and `pause` has no `prompted` flag to feed a confirm.
    let mut asked = 0;
    let hint = action::pause_hint(None, true, || {
        asked += 1;
        Ok("   ".to_owned())
    })
    .unwrap();
    assert_eq!(asked, 1, "exactly one question");
    assert_eq!(hint, None, "Enter (blank) skips");

    let hint = action::pause_hint(None, true, || Ok(" pick up at step 3 ".to_owned())).unwrap();
    assert_eq!(hint.as_deref(), Some("pick up at step 3"));

    // A typed flag is never asked about again; non-interactive never asks.
    let hint = action::pause_hint(Some("given".to_owned()), true, || {
        panic!("must not ask when --next is given")
    })
    .unwrap();
    assert_eq!(hint.as_deref(), Some("given"));
    let hint = action::pause_hint(None, false, || panic!("must not ask")).unwrap();
    assert_eq!(hint, None);

    // And the written line lands without `next:` when the hint is skipped.
    let dir = vault_with_bullet();
    start(dir.path(), moment(2026, 5, 2, 10, 0), "x", "Run ablation");
    run_action(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ActionCommands::Pause {
            next: None,
            reason: None,
        },
    )
    .expect("pause");
    let daily = fs::read_to_string(dir.path().join("journal/2026/daily/2026-05-02.md")).unwrap();
    assert!(
        daily.contains("action paused on [[x]] \u{2014} Run ablation (deep)"),
        "{daily}"
    );
    assert!(!daily.contains("next:"), "{daily}");
}

#[test]
fn pause_with_no_interactive_never_prompts() {
    let dir = vault_with_bullet();
    cdno_in(dir.path())
        .args(["--no-interactive", "action", "start"])
        .args(["--project", "x", "--query", "Run ablation"])
        .assert()
        .success();
    // Null stdin (`< /dev/null`) and `--no-interactive`: a prompt would
    // die in the prompt library or hang; neither is allowed.
    let out = cdno_in(dir.path())
        .args(["--no-interactive", "action", "pause"])
        .write_stdin("")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let daily = logged_daily(dir.path(), &out);
    assert!(
        daily.contains("action paused on [[x]] \u{2014} Run ablation (deep)"),
        "{daily}"
    );
}

#[test]
fn pause_with_nothing_started_says_so_gently() {
    let dir = vault_with_bullet();
    let err = run_action(
        dir.path(),
        moment(2026, 5, 2, 11, 0),
        ActionCommands::Pause {
            next: None,
            reason: None,
        },
    )
    .expect_err("nothing to pause");
    assert_eq!(
        format!("{err:#}"),
        "Nothing started \u{2014} nothing to pause."
    );

    // Through the binary: the message, no cause chain, non-zero exit.
    cdno_in(dir.path())
        .args(["--no-interactive", "action", "pause"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "Nothing started \u{2014} nothing to pause.",
        ))
        .stderr(predicates::str::contains("Caused by").not());
}

#[test]
fn resume_prints_the_next_hint() {
    let dir = vault_with_bullet();
    for args in [
        &[
            "action",
            "start",
            "--project",
            "x",
            "--query",
            "Run ablation",
        ][..],
        &["action", "pause", "--next", "pick up at step 3"][..],
    ] {
        cdno_in(dir.path())
            .arg("--no-interactive")
            .args(args)
            .assert()
            .success();
    }
    let out = cdno_in(dir.path())
        .args(["--no-interactive", "action", "resume"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Resumed on x, logged to"))
        .stdout(predicates::str::contains("next: pick up at step 3"))
        .get_output()
        .stdout
        .clone();
    assert!(logged_daily(dir.path(), &out).contains("resumed [[x]]"));
}

#[test]
fn resume_with_project_resumes_that_pause() {
    let dir = vault_with_bullet();
    create_project(dir.path(), moment(2026, 5, 2, 9, 0), "Y", Context::Work);
    add_bullet(dir.path(), "y", "Write report");
    start(dir.path(), moment(2026, 5, 2, 10, 0), "x", "Run ablation");
    let pause = |at, next: &str| {
        run_action(
            dir.path(),
            at,
            ActionCommands::Pause {
                next: Some(next.to_owned()),
                reason: None,
            },
        )
        .expect("pause");
    };
    pause(moment(2026, 5, 2, 10, 30), "x hint");
    start(dir.path(), moment(2026, 5, 2, 11, 0), "y", "Write report");
    pause(moment(2026, 5, 2, 11, 30), "y hint");

    run_action(
        dir.path(),
        moment(2026, 5, 2, 12, 0),
        ActionCommands::Resume {
            project: Some("x".to_owned()),
        },
    )
    .expect("resume x");
    let daily = fs::read_to_string(dir.path().join("journal/2026/daily/2026-05-02.md")).unwrap();
    assert!(
        daily.contains("resumed [[x]] \u{2014} Run ablation (deep)"),
        "{daily}"
    );
    assert!(!daily.contains("resumed [[y]]"), "{daily}");

    // With x in focus, resuming y is refused and says what to do.
    let err = run_action(
        dir.path(),
        moment(2026, 5, 2, 12, 30),
        ActionCommands::Resume {
            project: Some("y".to_owned()),
        },
    )
    .expect_err("slot taken");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("already in focus") && msg.contains("action switch"),
        "{msg}"
    );
}

/// Drive the pause handler as an interactive run, counting questions.
fn interactive_pause(
    root: &Path,
    at: NaiveDateTime,
    next: Option<&str>,
    answer: &str,
) -> (anyhow::Result<()>, usize) {
    let (vault, _) = cdno_cli::bootstrap::open_vault(root).unwrap();
    let mut asked = 0;
    let r = action::pause_asking(
        &vault,
        at,
        next.map(str::to_owned),
        None,
        true,
        false,
        || {
            asked += 1;
            Ok(answer.to_owned())
        },
    );
    (r, asked)
}

#[test]
fn the_pause_handler_asks_exactly_once_when_nothing_is_given() {
    let dir = vault_with_bullet();
    start(dir.path(), moment(2026, 5, 2, 10, 0), "x", "Run ablation");
    let (r, asked) = interactive_pause(dir.path(), moment(2026, 5, 2, 11, 0), None, "step 3");
    r.expect("pause");
    assert_eq!(asked, 1);
    let daily = fs::read_to_string(dir.path().join("journal/2026/daily/2026-05-02.md")).unwrap();
    assert!(daily.contains("next: step 3"), "{daily}");
}

#[test]
fn the_pause_handler_never_asks_when_nothing_is_in_focus() {
    let dir = vault_with_bullet();
    let (r, asked) = interactive_pause(dir.path(), moment(2026, 5, 2, 11, 0), None, "x");
    assert!(r.is_err());
    assert_eq!(asked, 0, "the refusal comes before the question");
}

#[test]
fn the_pause_handler_never_asks_when_next_is_given() {
    let dir = vault_with_bullet();
    start(dir.path(), moment(2026, 5, 2, 10, 0), "x", "Run ablation");
    let (r, asked) = interactive_pause(dir.path(), moment(2026, 5, 2, 11, 0), Some("given"), "x");
    r.expect("pause");
    assert_eq!(asked, 0);
    let daily = fs::read_to_string(dir.path().join("journal/2026/daily/2026-05-02.md")).unwrap();
    assert!(daily.contains("next: given"), "{daily}");
}

#[test]
fn resuming_the_action_already_in_focus_says_nothing_to_resume() {
    let dir = vault_with_bullet();
    start(dir.path(), moment(2026, 5, 2, 10, 0), "x", "Run ablation");
    let err = run_action(
        dir.path(),
        moment(2026, 5, 2, 10, 30),
        ActionCommands::Resume { project: None },
    )
    .expect_err("already focused");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("already in focus") && msg.contains("nothing to resume"),
        "{msg}"
    );
    assert!(!msg.contains("switch") && !msg.contains("pause"), "{msg}");
}

#[test]
fn resume_with_a_project_that_has_nothing_names_it() {
    let dir = vault_with_bullet();
    let msg = |p: Option<&str>| {
        let e = run_action(
            dir.path(),
            moment(2026, 5, 2, 10, 30),
            ActionCommands::Resume {
                project: p.map(str::to_owned),
            },
        )
        .expect_err("nothing");
        format!("{e:#}")
    };
    assert_eq!(
        msg(Some("x")),
        "Nothing to resume on x \u{2014} no carried focus or pause there."
    );
    assert_eq!(
        msg(None),
        "Nothing to resume \u{2014} nothing is carried over or paused."
    );
}

#[test]
fn pause_and_resume_report_in_json() {
    let dir = vault_with_bullet();
    let run = |args: &[&str]| -> serde_json::Value {
        let out = cdno_in(dir.path())
            .args(["--json", "action"])
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice(&out).expect("json")
    };
    run(&["start", "--project", "x", "--query", "Run ablation"]);
    let p = run(&["pause", "--next", "step 3", "--reason", "lunch"]);
    assert!(p["path"].is_string() && p["message"].as_str().unwrap().starts_with("Paused on x"));
    let r = run(&["resume"]);
    assert!(r["message"].as_str().unwrap().starts_with("Resumed on x"));
    assert_eq!(r["resumed_from"]["kind"], "paused");
    assert_eq!(r["resumed_from"]["next"], "step 3");
    assert_eq!(r["resumed_from"]["reason"], "lunch");
}

#[test]
fn resume_re_anchors_a_carried_focus_through_the_cli() {
    let dir = vault_with_bullet();
    // Two days back inside a two-day window: a run that crosses midnight
    // moves the binary's clock by a day and still finds the focus.
    let cfg = dir.path().join(".cuaderno/config.toml");
    let mut body = fs::read_to_string(&cfg).unwrap();
    assert!(
        !body.lines().any(|l| l.trim() == "[focus]"),
        "init config grew a [focus] table"
    );
    body.push_str("\n[focus]\ncarry_over_days = 2\n");
    fs::write(&cfg, body).unwrap();
    let two_days_ago = chrono::Local::now().naive_local() - chrono::Duration::days(2);
    start(dir.path(), two_days_ago, "x", "Run ablation");
    let out = cdno_in(dir.path())
        .args(["--no-interactive", "action", "resume"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Resumed on x"))
        .get_output()
        .stdout
        .clone();
    assert!(logged_daily(dir.path(), &out).contains("resumed [[x]]"));
}

// --- switch and the start refusal (RFC 0005, T13) -------------------------

/// A vault with the stock project and two open bullets.
fn surrogate_vault() -> TempDir {
    let dir = vault();
    create_project(
        dir.path(),
        moment(2026, 9, 30, 9, 0),
        "Surrogate model",
        Context::Work,
    );
    add_bullet(dir.path(), "surrogate-model", "Draft methods section");
    add_bullet(dir.path(), "surrogate-model", "Run ablation");
    dir
}

fn daily_of(root: &Path, day: &str) -> String {
    let year = &day[..4];
    fs::read_to_string(root.join(format!("journal/{year}/daily/{day}.md"))).unwrap_or_default()
}

#[test]
fn switch_requires_the_unplanned_flag_for_title() {
    let dir = surrogate_vault();
    cdno_in(dir.path())
        .args(["--no-interactive", "action", "switch"])
        .args(["--project", "surrogate-model", "--title", "Sketch"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--unplanned"));
    // And the query form stays apart from the unplanned form.
    cdno_in(dir.path())
        .args(["--no-interactive", "action", "switch"])
        .args([
            "--project",
            "surrogate-model",
            "--query",
            "x",
            "--unplanned",
        ])
        .assert()
        .failure();
}

#[test]
fn switch_preview_names_both_sides() {
    let dir = surrogate_vault();
    start(
        dir.path(),
        moment(2026, 10, 1, 9, 10),
        "surrogate-model",
        "Draft methods",
    );
    let (vault, _) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let focus = vault
        .current_focus(moment(2026, 10, 1, 10, 0).date())
        .unwrap();
    let focus = focus.expect("a focus");
    let resolved = action::Resolved::Bullet {
        project: "surrogate-model".to_owned(),
        query: "Run ablation".to_owned(),
    };
    let preview = action::switch_preview(Some(&focus), &resolved, &Some("step 3".to_owned()));
    assert!(preview.contains("pause Draft methods section"), "{preview}");
    assert!(preview.contains("start 'Run ablation'"), "{preview}");
    assert!(preview.contains("next:  step 3"), "{preview}");
    // Without a typed hint the preview shows none: the question comes later.
    let preview = action::switch_preview(Some(&focus), &resolved, &None);
    assert!(!preview.contains("next:"), "{preview}");
    // Nothing open: no pause side.
    let preview = action::switch_preview(None, &resolved, &None);
    assert!(!preview.contains("pause"), "{preview}");
}

#[test]
fn switch_with_nothing_open_reports_the_ignored_next() {
    let dir = surrogate_vault();
    cdno_in(dir.path())
        .args(["--no-interactive", "action", "switch"])
        .args(["--project", "surrogate-model", "--query", "Run ablation"])
        .args(["--next", "step 3"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Nothing was open \u{2014} started Run ablation.",
        ))
        .stdout(predicates::str::contains(
            "(--next ignored: nothing to attach it to)",
        ));
    // Without --next the second line is absent.
    let dir = surrogate_vault();
    cdno_in(dir.path())
        .args(["--no-interactive", "action", "switch"])
        .args(["--project", "surrogate-model", "--query", "Run ablation"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--next ignored").not());
}

#[test]
fn switch_pauses_then_starts_and_the_hint_prompt_is_asked_once() {
    let dir = surrogate_vault();
    start(
        dir.path(),
        moment(2026, 10, 1, 9, 10),
        "surrogate-model",
        "Draft methods",
    );
    let (vault, _) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let mut asked = 0;
    action::switch_after_gather(
        &vault,
        moment(2026, 10, 1, 10, 0),
        resolved_ablation(),
        false,
        None,
        Some("lunch".to_owned()),
        true,
        false,
        None,
        |_| panic!("not prompted, so no confirm"),
        || {
            asked += 1;
            Ok("step 3".to_owned())
        },
    )
    .expect("switch");
    assert_eq!(asked, 1);
    let daily = daily_of(dir.path(), "2026-10-01");
    assert!(
        daily.contains("action paused on [[surrogate-model]] \u{2014} Draft methods section"),
        "{daily}"
    );
    assert!(
        daily.contains("next: step 3") && daily.contains("reason: lunch"),
        "{daily}"
    );
    assert!(
        daily.contains("started [[surrogate-model]] \u{2014} Run ablation (deep)"),
        "{daily}"
    );
}

#[test]
fn start_refusal_names_the_switch_command() {
    let dir = surrogate_vault();
    cdno_in(dir.path())
        .args(["--no-interactive", "action", "start"])
        .args(["--project", "surrogate-model", "--query", "Draft methods"])
        .assert()
        .success();
    let out = cdno_in(dir.path())
        .args(["--no-interactive", "action", "start"])
        .args(["--project", "surrogate-model", "--query", "Run ablation"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let err = String::from_utf8_lossy(&out);
    assert!(
        err.contains("Draft methods section is already in focus"),
        "{err}"
    );
    assert!(
        err.contains(" action switch --project surrogate-model --query 'Run ablation'"),
        "{err}"
    );
    // The person passed --vault, so the suggestion carries it.
    assert!(
        err.contains(&format!(
            "cdno --vault {} action switch",
            dir.path().display()
        )),
        "{err}"
    );
    assert!(!err.contains("Caused by"), "{err}");

    // The unplanned form names its own command and creates nothing.
    let out = cdno_in(dir.path())
        .args(["--no-interactive", "action", "start"])
        .args(["--project", "surrogate-model", "--unplanned"])
        .args(["--title", "Sketch", "--energy", "light"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let err = String::from_utf8_lossy(&out);
    assert!(
        err.contains(
            " action switch --project surrogate-model --unplanned --title Sketch --energy light"
        ),
        "{err}"
    );
    let map = fs::read_to_string(dir.path().join("projects/surrogate-model.md")).unwrap();
    assert!(!map.contains("Sketch"), "{map}");
}

#[test]
fn start_on_the_focused_action_says_there_is_nothing_to_do() {
    let dir = surrogate_vault();
    start(
        dir.path(),
        moment(2026, 10, 1, 9, 10),
        "surrogate-model",
        "Draft methods",
    );
    let err = run_action(
        dir.path(),
        moment(2026, 10, 1, 9, 30),
        ActionCommands::Start {
            project: Some("surrogate-model".to_owned()),
            query: Some("Draft methods".to_owned()),
            unplanned: false,
            title: None,
            energy: None,
        },
    )
    .expect_err("refused");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("nothing to do") && !msg.contains("switch"),
        "{msg}"
    );
}

/// The fixture T14's MCP rejection is compared against too.
fn rejection_fixture() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cdno-mcp/tests/fixtures/focus_open_rejection.json");
    serde_json::from_str(&fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}")))
        .expect("fixture is json")
}

#[test]
fn start_refusal_json_matches_the_rejection_shape() {
    // Fixed times, so the comparison is exact: the focus is the real
    // domain refusal for a start at 09:10 on 2026-10-01, asked again at 10:00.
    let dir = surrogate_vault();
    start(
        dir.path(),
        moment(2026, 10, 1, 9, 10),
        "surrogate-model",
        "Draft methods",
    );
    let (vault, _) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let err = vault
        .start_action(
            moment(2026, 10, 1, 10, 0),
            "surrogate-model",
            "Run ablation",
        )
        .expect_err("refused");
    let cdno_domain::error::DomainError::FocusOpen {
        focus,
        same_action,
        carried,
    } = err
    else {
        panic!("not FocusOpen: {err:?}");
    };
    assert_eq!(
        action::focus_open_rejection(
            &focus,
            same_action,
            carried,
            Some(serde_json::json!({"project": "surrogate-model", "query": "Run ablation"}))
        ),
        rejection_fixture()
    );

    // The remedies for the other two cases.
    assert_eq!(action::focus_open_remedy(true, false), "already_focused");
    assert_eq!(action::focus_open_remedy(true, true), "resume_action");

    // Through the binary, on today's clock: the shape, the exit code, and
    // no write.
    let live = surrogate_vault();
    cdno_in(live.path())
        .args(["--no-interactive", "action", "start"])
        .args(["--project", "surrogate-model", "--query", "Draft methods"])
        .assert()
        .success();
    let out = cdno_in(live.path())
        .args(["--json", "action", "start"])
        .args(["--project", "surrogate-model", "--query", "Run ablation"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("json on stdout");
    let mut expected = rejection_fixture();
    // Only the clock-dependent values differ from the fixture.
    expected["details"]["focus"]["started"] = v["details"]["focus"]["started"].clone();
    expected["details"]["focus"]["date"] = v["details"]["focus"]["date"].clone();
    assert_eq!(v, expected);
}

#[test]
fn pause_and_resume_refusals_are_json_objects_under_json() {
    let dir = surrogate_vault();
    let run = |args: &[&str]| -> serde_json::Value {
        let out = cdno_in(dir.path())
            .args(["--json", "action"])
            .args(args)
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice(&out).expect("json on stdout")
    };
    for verb in [["pause"], ["resume"]] {
        let v = run(&verb);
        assert_eq!(v["code"], "no_focus");
        assert_eq!(v["message"], "Nothing is in focus or paused.");
        assert_eq!(v["details"], serde_json::json!({}));
    }
    cdno_in(dir.path())
        .args(["--no-interactive", "action", "start"])
        .args(["--project", "surrogate-model", "--query", "Draft methods"])
        .assert()
        .success();
    // A different action cannot be resumed over the focus.
    cdno_in(dir.path())
        .args(["--no-interactive", "action", "pause"])
        .assert()
        .success();
    cdno_in(dir.path())
        .args(["--no-interactive", "action", "start"])
        .args(["--project", "surrogate-model", "--query", "Run ablation"])
        .assert()
        .success();
    let v = run(&["resume"]);
    assert_eq!(v["code"], "focus_open");
    assert_eq!(v["details"]["remedy"], "switch_action");
}

fn resolved_ablation() -> action::Resolved {
    action::Resolved::Bullet {
        project: "surrogate-model".to_owned(),
        query: "Run ablation".to_owned(),
    }
}

/// Drive a start from resolved arguments as an interactive run; `answer`
/// is the offer's reply, and the default the offer was made with is
/// returned beside the number of times it was asked.
fn ask_start(
    root: &Path,
    at: NaiveDateTime,
    answer: bool,
    flag: Option<&Path>,
) -> (anyhow::Result<()>, usize, Option<bool>) {
    let (vault, _) = cdno_cli::bootstrap::open_vault(root).unwrap();
    let mut asked = 0;
    let mut default = None;
    let r = action::start_after_gather(
        &vault,
        at,
        resolved_ablation(),
        false,
        true,
        false,
        flag,
        |_| panic!("not prompted, so no preview"),
        |question, d| {
            assert_eq!(question, action::SWITCH_OFFER);
            asked += 1;
            default = Some(d);
            Ok(answer)
        },
    );
    (r, asked, default)
}

#[test]
fn start_refusal_offers_the_switch_and_defaults_to_no() {
    // The suite has no pty helper, so the offer is driven through the
    // injected confirm of `start_after_gather`, which is handed the default
    // the call site passes; flipping that literal fails here.
    let dir = surrogate_vault();
    start(
        dir.path(),
        moment(2026, 10, 1, 9, 10),
        "surrogate-model",
        "Draft methods",
    );
    let before = daily_of(dir.path(), "2026-10-01");

    // Declined: asked once with default false, nothing written.
    let (r, asked, default) = ask_start(dir.path(), moment(2026, 10, 1, 10, 0), false, None);
    r.expect("declining is not an error");
    assert_eq!(asked, 1);
    assert_eq!(default, Some(false), "Enter must not move focus");
    assert_eq!(daily_of(dir.path(), "2026-10-01"), before);

    // Accepted: the switch runs with the resolved arguments, and no
    // --next question is asked (run_switch is given none to ask).
    let (r, asked, _) = ask_start(dir.path(), moment(2026, 10, 1, 10, 0), true, None);
    r.expect("switch");
    assert_eq!(asked, 1);
    let daily = daily_of(dir.path(), "2026-10-01");
    assert!(
        daily.contains("action paused on [[surrogate-model]]"),
        "{daily}"
    );
    assert!(!daily.contains("next:"), "{daily}");
    assert!(
        daily.contains("started [[surrogate-model]] \u{2014} Run ablation (deep)"),
        "{daily}"
    );
}

#[test]
fn the_offer_is_not_made_for_the_action_already_in_focus() {
    let dir = surrogate_vault();
    start(
        dir.path(),
        moment(2026, 10, 1, 9, 10),
        "surrogate-model",
        "Run ablation",
    );
    let (r, asked, _) = ask_start(dir.path(), moment(2026, 10, 1, 10, 0), true, None);
    assert!(r.is_err());
    assert_eq!(asked, 0);
}

#[test]
fn a_refused_start_never_asks_to_proceed_first() {
    let dir = surrogate_vault();
    start(
        dir.path(),
        moment(2026, 10, 1, 9, 10),
        "surrogate-model",
        "Draft methods",
    );
    let (vault, _) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let mut offered = false;
    // A prompted run (`prompted = true`): the preview must not be shown.
    action::start_after_gather(
        &vault,
        moment(2026, 10, 1, 10, 0),
        resolved_ablation(),
        true,
        true,
        false,
        None,
        |_| panic!("a refused start must not ask 'Proceed?' first"),
        |_, _| {
            offered = true;
            Ok(false)
        },
    )
    .expect("declined");
    assert!(offered);
    // With nothing open the preview is still shown.
    assert!(action::start_confirms(true, false));
    assert!(!action::start_confirms(true, true));
    assert!(!action::start_confirms(false, false));
}

#[test]
fn a_prompted_switch_confirms_without_the_hint_and_asks_for_it_after() {
    let dir = surrogate_vault();
    start(
        dir.path(),
        moment(2026, 10, 1, 9, 10),
        "surrogate-model",
        "Draft methods",
    );
    let (vault, _) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let preview = std::cell::RefCell::new(String::new());
    let mut asked_after_confirm = false;
    action::switch_after_gather(
        &vault,
        moment(2026, 10, 1, 10, 0),
        resolved_ablation(),
        true,
        None,
        None,
        true,
        false,
        None,
        |p| {
            *preview.borrow_mut() = p.to_owned();
            Ok(true)
        },
        || {
            asked_after_confirm = !preview.borrow().is_empty();
            Ok("step 3".to_owned())
        },
    )
    .expect("switch");
    let preview = preview.into_inner();
    assert!(asked_after_confirm, "the hint is asked after the confirm");
    assert!(
        !preview.contains("step 3") && !preview.contains("next:"),
        "{preview}"
    );
    assert!(preview.contains("pause Draft methods section"), "{preview}");
    assert!(daily_of(dir.path(), "2026-10-01").contains("next: step 3"));
}

#[test]
fn no_hint_is_asked_when_nothing_is_open() {
    let dir = surrogate_vault();
    let (vault, _) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    action::switch_after_gather(
        &vault,
        moment(2026, 10, 1, 10, 0),
        resolved_ablation(),
        false,
        None,
        None,
        true,
        false,
        None,
        |_| panic!("not prompted"),
        || panic!("nothing is open, so there is nothing to ask a hint for"),
    )
    .expect("a plain start");
    assert!(daily_of(dir.path(), "2026-10-01").contains("started [[surrogate-model]]"));
}

#[test]
fn the_picker_leaves_out_the_bullet_in_focus() {
    let dir = surrogate_vault();
    start(
        dir.path(),
        moment(2026, 10, 1, 9, 10),
        "surrogate-model",
        "Draft methods",
    );
    let (vault, _) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let focus = vault
        .current_focus(moment(2026, 10, 1, 10, 0).date())
        .unwrap()
        .expect("focus");
    let entries = vault.list_actions("surrogate-model").unwrap();
    let all = action::pickable_labels(&entries, "surrogate-model", None);
    assert!(all.iter().any(|l| l == &focus.action));
    let picked = action::pickable_labels(&entries, "surrogate-model", Some(&focus));
    assert_eq!(picked.len(), all.len() - 1);
    assert!(!picked.iter().any(|l| l == &focus.action));
    // The same text on another project is not the focus.
    let other = action::pickable_labels(&entries, "elsewhere", Some(&focus));
    assert_eq!(other.len(), all.len());
}

#[test]
fn a_carried_focus_names_its_day_and_the_vault_flag_is_kept() {
    let dir = surrogate_vault();
    start(
        dir.path(),
        moment(2026, 10, 1, 14, 5),
        "surrogate-model",
        "Draft methods",
    );
    // Thursday's focus, asked about on Friday (carry_over_days defaults to 1).
    let flag = dir.path().to_path_buf();
    let (r, _, _) = ask_start(
        dir.path(),
        moment(2026, 10, 2, 10, 0),
        false,
        Some(flag.as_path()),
    );
    r.expect("declined");
    // Not interactive: the text is the error, which we can read.
    let (vault, _) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let err = action::start_after_gather(
        &vault,
        moment(2026, 10, 2, 10, 0),
        resolved_ablation(),
        false,
        false,
        false,
        Some(flag.as_path()),
        |_| panic!(),
        |_, _| panic!(),
    )
    .expect_err("refused");
    let msg = format!("{err:#}");
    assert!(msg.contains("(since Thursday 14:05)"), "{msg}");
    let quoted = flag.to_string_lossy();
    assert!(
        msg.contains(&format!("cdno --vault {quoted} action switch --project")),
        "{msg}"
    );
}

#[test]
fn an_unplanned_switch_names_the_daily_note_it_logged_to() {
    let dir = surrogate_vault();
    // Through the binary, so the focus is open on the binary's own clock.
    cdno_in(dir.path())
        .args(["--no-interactive", "action", "start"])
        .args(["--project", "surrogate-model", "--query", "Draft methods"])
        .assert()
        .success();
    let out = cdno_in(dir.path())
        .args(["--no-interactive", "action", "switch"])
        .args(["--project", "surrogate-model", "--unplanned"])
        .args(["--title", "Sketch", "--energy", "light"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8_lossy(&out);
    assert!(out.contains("logged to journal/"), "{out}");
    assert!(!out.contains("logged to projects/"), "{out}");
}
