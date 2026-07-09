//! In-process tests for `commands::frontmatter::run` (#301).

use std::fs;

use cdno_cli::commands::frontmatter::{self, FrontmatterCommands};
use cdno_cli::commands::init;
use chrono::{Datelike, Local};
use tempfile::tempdir;

#[test]
fn frontmatter_set_flips_a_daily_flag_through_the_index() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    // Declare a settable `meds` bool for daily notes.
    let cfg = dir.path().join(".cuaderno/config.toml");
    let mut contents = fs::read_to_string(&cfg).expect("config.toml written by init");
    contents.push_str("\n[schemas.daily.fields.meds]\ntype = \"bool\"\nsettable = true\n");
    fs::write(&cfg, contents).unwrap();

    // Seed today's daily note carrying `meds: false` (the setter runs against
    // `Local::now()`, so the note must be today's).
    let today = Local::now().naive_local().date();
    let rel = format!(
        "journal/{}/daily/{}.md",
        today.year(),
        today.format("%Y-%m-%d")
    );
    let daily = dir.path().join(&rel);
    fs::create_dir_all(daily.parent().unwrap()).unwrap();
    let body = format!(
        "---\ntype: daily\ndate: {}\nmeds: false\n---\n\n# Day\n\n## Logs\n",
        today.format("%Y-%m-%d")
    );
    fs::write(&daily, &body).unwrap();

    frontmatter::run(
        dir.path(),
        FrontmatterCommands::Set {
            note: "today".to_owned(),
            key: "meds".to_owned(),
            value: "true".to_owned(),
        },
        false,
    )
    .expect("frontmatter set succeeds");

    let content = fs::read_to_string(&daily).expect("daily note exists");
    assert!(content.contains("meds: true"), "flag flipped: {content}");
}

#[test]
fn frontmatter_set_rejects_a_non_settable_field() {
    let dir = tempdir().unwrap();
    init::run(dir.path()).expect("init");

    // A declared field with no `settable = true` is default-denied.
    let cfg = dir.path().join(".cuaderno/config.toml");
    let mut contents = fs::read_to_string(&cfg).unwrap();
    contents.push_str("\n[schemas.daily.fields.meds]\ntype = \"bool\"\n");
    fs::write(&cfg, contents).unwrap();

    let today = Local::now().naive_local().date();
    let rel = format!(
        "journal/{}/daily/{}.md",
        today.year(),
        today.format("%Y-%m-%d")
    );
    let daily = dir.path().join(&rel);
    fs::create_dir_all(daily.parent().unwrap()).unwrap();
    fs::write(
        &daily,
        format!(
            "---\ntype: daily\ndate: {}\nmeds: false\n---\n\n# Day\n\n## Logs\n",
            today.format("%Y-%m-%d")
        ),
    )
    .unwrap();

    let err = frontmatter::run(
        dir.path(),
        FrontmatterCommands::Set {
            note: "today".to_owned(),
            key: "meds".to_owned(),
            value: "true".to_owned(),
        },
        false,
    )
    .expect_err("a non-settable field must be rejected");
    assert!(
        format!("{err}").contains("not settable"),
        "unexpected error: {err}"
    );
}
