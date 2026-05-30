//! In-process tests for `commands::question::run` and
//! `commands::questions::run` (the top-level list). All tests pass
//! `no_interactive = true` so prompts never fire; missing-flag tests
//! assert the convention.

use std::fs;
use std::path::Path;

use cdno_cli::commands::question::{self, QuestionCommands};
use cdno_cli::commands::{init, questions};
use cdno_domain::frontmatter::QuestionDomain;
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

fn create(root: &Path, at: NaiveDateTime, domain: QuestionDomain, text: &str) {
    question::run(
        root,
        at,
        QuestionCommands::Create {
            domain: Some(domain),
            text: Some(text.to_owned()),
        },
        true,
    )
    .expect("create");
}

// ---------------------------------------------------------------------
// create
// ---------------------------------------------------------------------

#[test]
fn create_writes_question_under_domain_folder() {
    let dir = vault();
    create(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        QuestionDomain::Research,
        "Does sparse beat dense on OOD?",
    );

    let path = dir
        .path()
        .join("questions/research/does-sparse-beat-dense-on-ood.md");
    let raw = fs::read_to_string(&path).expect("file exists");
    assert!(raw.contains("type: question"));
    assert!(raw.contains("domain: research"));
    assert!(raw.contains("status: active"));
    assert!(raw.contains("# Does sparse beat dense on OOD?"));
}

#[test]
fn create_errors_when_missing_domain_in_non_interactive() {
    let dir = vault();
    let err = question::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        QuestionCommands::Create {
            domain: None,
            text: Some("Anything?".to_owned()),
        },
        true,
    )
    .expect_err("missing --domain should error");
    assert!(format!("{err:#}").contains("--domain"));
}

#[test]
fn create_errors_when_missing_text_in_non_interactive() {
    let dir = vault();
    let err = question::run(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        QuestionCommands::Create {
            domain: Some(QuestionDomain::Life),
            text: None,
        },
        true,
    )
    .expect_err("missing --text should error");
    assert!(format!("{err:#}").contains("--text"));
}

// ---------------------------------------------------------------------
// transitions
// ---------------------------------------------------------------------

/// Daily note pre-seeded so `set_question_status` (which appends a
/// log entry) succeeds.
fn seed_daily(root: &Path, date: NaiveDate) {
    let path = root.join(format!(
        "journal/{}/daily/{}.md",
        date.format("%Y"),
        date.format("%Y-%m-%d")
    ));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!(
            "---\ndate: {}\ntype: daily\n---\n\n# Heading\n\n## Logs\n",
            date.format("%Y-%m-%d")
        ),
    )
    .unwrap();
}

#[test]
fn park_transitions_status_and_logs_to_daily() {
    let dir = vault();
    create(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        QuestionDomain::Research,
        "Does sparse beat dense?",
    );
    seed_daily(dir.path(), NaiveDate::from_ymd_opt(2026, 5, 1).unwrap());

    question::run(
        dir.path(),
        moment(2026, 5, 1, 14, 30),
        QuestionCommands::Park {
            slug: Some("does-sparse-beat-dense".to_owned()),
        },
        true,
    )
    .expect("park");

    let raw = fs::read_to_string(
        dir.path()
            .join("questions/research/does-sparse-beat-dense.md"),
    )
    .unwrap();
    assert!(raw.contains("status: parked"));
    assert!(raw.contains("updated: 2026-05-01"));

    let daily = fs::read_to_string(dir.path().join("journal/2026/daily/2026-05-01.md")).unwrap();
    assert!(daily.contains("status on [[questions/research/does-sparse-beat-dense]]"));
    assert!(daily.contains("was: active"));
    assert!(daily.contains("now: parked"));
}

#[test]
fn answer_retire_activate_each_set_the_target_status() {
    let dir = vault();
    create(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        QuestionDomain::Research,
        "Does sparse beat dense?",
    );
    let qpath = dir
        .path()
        .join("questions/research/does-sparse-beat-dense.md");

    // answer: active -> answered
    seed_daily(dir.path(), NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
    question::run(
        dir.path(),
        moment(2026, 2, 1, 9, 0),
        QuestionCommands::Answer {
            slug: Some("does-sparse-beat-dense".to_owned()),
        },
        true,
    )
    .unwrap();
    assert!(
        fs::read_to_string(&qpath)
            .unwrap()
            .contains("status: answered")
    );

    // retire: answered -> retired
    seed_daily(dir.path(), NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
    question::run(
        dir.path(),
        moment(2026, 3, 1, 9, 0),
        QuestionCommands::Retire {
            slug: Some("does-sparse-beat-dense".to_owned()),
        },
        true,
    )
    .unwrap();
    assert!(
        fs::read_to_string(&qpath)
            .unwrap()
            .contains("status: retired")
    );

    // activate: retired -> active
    seed_daily(dir.path(), NaiveDate::from_ymd_opt(2026, 4, 1).unwrap());
    question::run(
        dir.path(),
        moment(2026, 4, 1, 9, 0),
        QuestionCommands::Activate {
            slug: Some("does-sparse-beat-dense".to_owned()),
        },
        true,
    )
    .unwrap();
    assert!(
        fs::read_to_string(&qpath)
            .unwrap()
            .contains("status: active")
    );
}

#[test]
fn park_errors_when_missing_slug_in_non_interactive() {
    let dir = vault();
    let err = question::run(
        dir.path(),
        moment(2026, 5, 1, 9, 0),
        QuestionCommands::Park { slug: None },
        true,
    )
    .expect_err("missing --slug should error");
    assert!(format!("{err:#}").contains("--slug"));
}

#[test]
fn park_errors_when_slug_unknown() {
    let dir = vault();
    let err = question::run(
        dir.path(),
        moment(2026, 5, 1, 9, 0),
        QuestionCommands::Park {
            slug: Some("nonexistent".to_owned()),
        },
        true,
    )
    .expect_err("unknown slug should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("parking question"), "msg: {msg}");
}

// ---------------------------------------------------------------------
// cdno questions (top-level list)
// ---------------------------------------------------------------------

#[test]
fn questions_list_render_empty_placeholder() {
    let out = questions::render(&[]);
    assert!(out.contains("Active questions"));
    assert!(out.contains("none"), "out:\n{out}");
}

#[test]
fn questions_list_render_groups_by_domain() {
    let dir = vault();
    create(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        QuestionDomain::Research,
        "Does sparse beat dense?",
    );
    create(
        dir.path(),
        moment(2026, 1, 11, 9, 0),
        QuestionDomain::Research,
        "Can surrogates cut cost 10x?",
    );
    create(
        dir.path(),
        moment(2026, 1, 12, 9, 0),
        QuestionDomain::Life,
        "Where do I want to be in five years?",
    );

    let (vault_obj, _r) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let active = vault_obj.active_questions().unwrap();
    let out = questions::render(&active);

    // Domain headings, capitalised.
    assert!(out.contains("\nResearch\n"), "out:\n{out}");
    assert!(out.contains("\nLife\n"), "out:\n{out}");
    // Slugs + H1 text shown.
    assert!(out.contains("does-sparse-beat-dense"));
    assert!(out.contains("can-surrogates-cut-cost-10x"));
    assert!(out.contains("where-do-i-want-to-be"));
    assert!(out.contains("\u{2014} Does sparse beat dense?"));
}

#[test]
fn questions_list_skips_non_active() {
    let dir = vault();
    create(
        dir.path(),
        moment(2026, 1, 10, 9, 0),
        QuestionDomain::Research,
        "Does sparse beat dense?",
    );
    seed_daily(dir.path(), NaiveDate::from_ymd_opt(2026, 5, 1).unwrap());
    question::run(
        dir.path(),
        moment(2026, 5, 1, 9, 0),
        QuestionCommands::Park {
            slug: Some("does-sparse-beat-dense".to_owned()),
        },
        true,
    )
    .unwrap();

    let (vault_obj, _r) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let active = vault_obj.active_questions().unwrap();
    assert!(active.is_empty(), "{active:?}");
    let out = questions::render(&active);
    assert!(out.contains("none"));
}
