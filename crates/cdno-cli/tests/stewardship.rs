//! In-process tests for `commands::stewardship::run` and
//! `commands::track::run`. `no_interactive = true` so prompts never
//! fire; missing-flag tests assert the convention.

use std::fs;

use cdno_cli::commands::stewardship::{self, StewardshipCommands};
use cdno_cli::commands::{init, track};
use cdno_domain::frontmatter::Context;
use cdno_domain::recurrence::Recurrence;
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

// ---------------------------------------------------------------------
// stewardship create
// ---------------------------------------------------------------------

#[test]
fn create_flat_writes_root_level_file() {
    let dir = vault();
    stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::Create {
            name: Some("Finances".to_owned()),
            context: Some(Context::Household),
            tracking: false,
        },
        true,
        false,
    )
    .expect("create flat");

    let raw = fs::read_to_string(dir.path().join("stewardships/finances.md")).unwrap();
    assert!(raw.contains("type: stewardship"));
    assert!(raw.contains("context: household"));
    assert!(raw.contains("# Finances"));
}

#[test]
fn create_with_tracking_writes_expanded_layout() {
    let dir = vault();
    stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::Create {
            name: Some("Health".to_owned()),
            context: Some(Context::Personal),
            tracking: true,
        },
        true,
        false,
    )
    .expect("create expanded");

    assert!(dir.path().join("stewardships/health/_index.md").exists());
    assert!(!dir.path().join("stewardships/health.md").exists());
}

#[test]
fn create_errors_when_missing_required_flags_in_non_interactive() {
    let dir = vault();
    let err = stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::Create {
            name: None,
            context: Some(Context::Household),
            tracking: false,
        },
        true,
        false,
    )
    .expect_err("missing --name should error");
    assert!(format!("{err:#}").contains("--name"));
}

// ---------------------------------------------------------------------
// stewardship list / show renderers
// ---------------------------------------------------------------------

#[test]
fn list_render_empty_placeholder() {
    let out = stewardship::render_list(&[]);
    assert!(out.contains("Stewardships"));
    assert!(out.contains("none"), "out:\n{out}");
}

#[test]
fn list_render_shows_each_with_variant_and_activity_badge() {
    let dir = vault();
    stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::Create {
            name: Some("Finances".to_owned()),
            context: Some(Context::Household),
            tracking: false,
        },
        true,
        false,
    )
    .unwrap();
    stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::Create {
            name: Some("Health".to_owned()),
            context: Some(Context::Personal),
            tracking: true,
        },
        true,
        false,
    )
    .unwrap();

    let (vault_obj, _r) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let summaries = vault_obj
        .list_stewardships(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap())
        .unwrap();
    let out = stewardship::render_list(&summaries);
    assert!(out.contains("finances"));
    assert!(out.contains("[flat]"), "out:\n{out}");
    assert!(out.contains("health"));
    assert!(out.contains("[expanded]"));
    assert!(out.contains("no tracking yet"));
}

#[test]
fn show_errors_when_missing_slug_in_non_interactive() {
    let dir = vault();
    let err = stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::Show { slug: None },
        true,
        false,
    )
    .expect_err("missing --slug should error");
    assert!(format!("{err:#}").contains("--slug"));
}

// ---------------------------------------------------------------------
// stewardship add-periodic
// ---------------------------------------------------------------------

#[test]
fn add_periodic_appends_line_to_dashboard() {
    let dir = vault();
    stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::Create {
            name: Some("Finances".to_owned()),
            context: Some(Context::Household),
            tracking: false,
        },
        true,
        false,
    )
    .unwrap();

    stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::AddPeriodic {
            stewardship: Some("finances".to_owned()),
            title: Some("Tax declaration".to_owned()),
            every: Some(Recurrence::Yearly),
            next: Some(NaiveDate::from_ymd_opt(2026, 5, 2).unwrap()),
        },
        true,
        false,
    )
    .unwrap();

    let raw = fs::read_to_string(dir.path().join("stewardships/finances.md")).unwrap();
    assert!(
        raw.contains("- Tax declaration \u{2014} yearly \u{2014} next: 2026-05-02"),
        "raw:\n{raw}"
    );
}

#[test]
fn add_periodic_errors_when_missing_flags_in_non_interactive() {
    let dir = vault();
    stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::Create {
            name: Some("Finances".to_owned()),
            context: Some(Context::Household),
            tracking: false,
        },
        true,
        false,
    )
    .unwrap();
    let err = stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::AddPeriodic {
            stewardship: Some("finances".to_owned()),
            title: None,
            every: Some(Recurrence::Yearly),
            next: Some(NaiveDate::from_ymd_opt(2026, 5, 2).unwrap()),
        },
        true,
        false,
    )
    .expect_err("missing --title should error");
    assert!(format!("{err:#}").contains("--title"));
}

// ---------------------------------------------------------------------
// cdno track
// ---------------------------------------------------------------------

#[test]
fn track_writes_gym_note_under_expanded_stewardship() {
    let dir = vault();
    stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::Create {
            name: Some("Health".to_owned()),
            context: Some(Context::Personal),
            tracking: true,
        },
        true,
        false,
    )
    .unwrap();

    track::run(
        dir.path(),
        moment(2026, 4, 6, 19, 0),
        "gym".to_owned(),
        Some("health".to_owned()),
        Some("upper-body-a".to_owned()),
        "Energy was good.".to_owned(),
        true,
        false,
    )
    .expect("track");

    let raw = fs::read_to_string(
        dir.path()
            .join("stewardships/health/tracking/2026-04-06-gym.md"),
    )
    .unwrap();
    assert!(raw.contains("# Gym \u{2014} 6 April 2026"));
    assert!(raw.contains("routine: \"[[stewardships/health/routines/upper-body-a]]\""));
    assert!(raw.contains("Energy was good."));
}

#[test]
fn track_defaults_to_only_expanded_stewardship_when_unambiguous() {
    let dir = vault();
    // One expanded stewardship; `--stewardship` omitted should
    // resolve to it automatically.
    stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::Create {
            name: Some("Health".to_owned()),
            context: Some(Context::Personal),
            tracking: true,
        },
        true,
        false,
    )
    .unwrap();

    track::run(
        dir.path(),
        moment(2026, 4, 7, 9, 0),
        "body".to_owned(),
        None,
        None,
        String::new(),
        true,
        false,
    )
    .expect("track defaults to single expanded");

    assert!(
        dir.path()
            .join("stewardships/health/tracking/2026-04-07-body.md")
            .exists()
    );
}

#[test]
fn track_errors_when_no_default_and_no_flag_in_non_interactive() {
    let dir = vault();
    // Zero expanded stewardships: no default, no flag, non-interactive -> error.
    let err = track::run(
        dir.path(),
        moment(2026, 4, 7, 9, 0),
        "gym".to_owned(),
        None,
        None,
        String::new(),
        true,
        false,
    )
    .expect_err("ambiguous default should require --stewardship");
    assert!(format!("{err:#}").contains("--stewardship"));
}

#[test]
fn track_errors_on_flat_stewardship_for_explicit_slug() {
    let dir = vault();
    stewardship::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        StewardshipCommands::Create {
            name: Some("Finances".to_owned()),
            context: Some(Context::Household),
            tracking: false,
        },
        true,
        false,
    )
    .unwrap();
    let err = track::run(
        dir.path(),
        moment(2026, 4, 7, 9, 0),
        "gym".to_owned(),
        Some("finances".to_owned()),
        None,
        String::new(),
        true,
        false,
    )
    .expect_err("flat stewardship has no tracking subdir");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("flat") || msg.contains("tracking"),
        "msg: {msg}"
    );
}
