//! In-process tests for `commands::portfolio::run` and
//! `commands::file::run`. All tests pass `no_interactive = true` so
//! prompts never fire; missing-flag tests assert the convention.

use std::fs;
use std::path::Path;

use cdno_cli::commands::portfolio::{self, PortfolioCommands};
use cdno_cli::commands::{file, init};
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

/// Create a sample portfolio "sparse-vs-dense" via `portfolio create`.
fn seed_portfolio(root: &Path) {
    portfolio::run(
        root,
        moment(2026, 2, 1, 9, 0),
        PortfolioCommands::Create {
            question: Some("Sparse vs dense OOD".to_owned()),
            project: None,
        },
        true,
    )
    .expect("create portfolio");
}

// ---------------------------------------------------------------------
// portfolio create
// ---------------------------------------------------------------------

#[test]
fn portfolio_create_writes_index_file() {
    let dir = vault();
    portfolio::run(
        dir.path(),
        moment(2026, 2, 1, 9, 0),
        PortfolioCommands::Create {
            question: Some("Does sparse beat dense on OOD?".to_owned()),
            project: Some("projects/surrogate".to_owned()),
        },
        true,
    )
    .expect("create");

    let path = dir
        .path()
        .join("portfolios/does-sparse-beat-dense-on-ood/_index.md");
    let body = fs::read_to_string(&path).expect("index file exists");
    assert!(body.contains("type: portfolio"));
    assert!(body.contains("Does sparse beat dense on OOD?"));
    assert!(body.contains("project: \"[[projects/surrogate]]\""));
}

#[test]
fn portfolio_create_errors_when_missing_question_in_non_interactive() {
    let dir = vault();
    let err = portfolio::run(
        dir.path(),
        moment(2026, 2, 1, 9, 0),
        PortfolioCommands::Create {
            question: None,
            project: None,
        },
        true,
    )
    .expect_err("missing --question should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("--question"), "error message: {msg}");
}

// ---------------------------------------------------------------------
// portfolio list
// ---------------------------------------------------------------------

#[test]
fn portfolio_list_renders_empty_placeholder_then_summaries() {
    let dir = vault();

    // Empty case via render_list directly so we don't capture stdout.
    let empty = portfolio::render_list(&[]);
    assert!(empty.contains("no portfolios"), "empty:\n{empty}");

    // After seeding one, build summaries via the public Vault API.
    seed_portfolio(dir.path());
    let (vault_obj, _r) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let summaries = vault_obj
        .list_portfolios(moment(2026, 4, 1, 9, 0).date())
        .unwrap();
    let listed = portfolio::render_list(&summaries);
    assert!(listed.contains("sparse-vs-dense-ood"), "listed:\n{listed}");
    assert!(listed.contains("Sparse vs dense OOD"));
    assert!(listed.contains("no evidence yet"));
}

// ---------------------------------------------------------------------
// portfolio show
// ---------------------------------------------------------------------

#[test]
fn portfolio_show_renders_frontmatter_and_evidence() {
    let dir = vault();
    seed_portfolio(dir.path());
    file::run(
        dir.path(),
        moment(2026, 3, 15, 10, 0),
        Some("sparse-vs-dense-ood".to_owned()),
        Some("Chen 2025".to_owned()),
        Some("projects/surrogate".to_owned()),
        "They show 4x speedup at 95% accuracy.\n".to_owned(),
        None,
        false,
        true,
    )
    .expect("file evidence");

    let (vault_obj, _r) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let fm = vault_obj.get_portfolio("sparse-vs-dense-ood").unwrap();
    let entries = vault_obj
        .get_portfolio_contents("sparse-vs-dense-ood")
        .unwrap();
    let summaries = vault_obj
        .list_portfolios(moment(2026, 4, 1, 9, 0).date())
        .unwrap();
    let summary = summaries.iter().find(|s| s.slug == "sparse-vs-dense-ood");

    let out = portfolio::render_show("sparse-vs-dense-ood", &fm, summary, &entries);
    assert!(out.contains("sparse-vs-dense-ood \u{2014} Sparse vs dense OOD"));
    assert!(out.contains("Created: 2026-02-01"));
    assert!(out.contains("Project: (none)"));
    assert!(out.contains("Evidence (1 notes, last 2026-03-15)"));
    assert!(out.contains("Chen 2025"));
    assert!(out.contains("origin: [[projects/surrogate]]"));
}

#[test]
fn portfolio_show_tags_attachment_evidence_with_its_kind() {
    // An attachment stub renders with a `[kind]` media tag so a
    // non-markdown artefact reads distinctly from prose evidence (#154).
    let dir = vault();
    seed_portfolio(dir.path());
    let artefact = dir.path().join("derivation.pdf");
    fs::write(&artefact, b"%PDF fake").unwrap();
    file::run(
        dir.path(),
        moment(2026, 3, 15, 10, 0),
        Some("sparse-vs-dense-ood".to_owned()),
        Some("Chen derivation".to_owned()),
        Some("projects/surrogate".to_owned()),
        "The closed-form bound.".to_owned(),
        Some(artefact),
        false,
        true,
    )
    .expect("attach");

    let (vault_obj, _r) = cdno_cli::bootstrap::open_vault(dir.path()).unwrap();
    let fm = vault_obj.get_portfolio("sparse-vs-dense-ood").unwrap();
    let entries = vault_obj
        .get_portfolio_contents("sparse-vs-dense-ood")
        .unwrap();
    let summaries = vault_obj
        .list_portfolios(moment(2026, 4, 1, 9, 0).date())
        .unwrap();
    let summary = summaries.iter().find(|s| s.slug == "sparse-vs-dense-ood");

    let out = portfolio::render_show("sparse-vs-dense-ood", &fm, summary, &entries);
    assert!(
        out.contains("[pdf] Chen derivation"),
        "attachment must render with a media tag:\n{out}"
    );
}

#[test]
fn portfolio_show_errors_when_missing_portfolio_in_non_interactive() {
    let dir = vault();
    let err = portfolio::run(
        dir.path(),
        moment(2026, 4, 1, 9, 0),
        PortfolioCommands::Show { portfolio: None },
        true,
    )
    .expect_err("missing --portfolio should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("--portfolio"), "error message: {msg}");
}

// ---------------------------------------------------------------------
// cdno file
// ---------------------------------------------------------------------

#[test]
fn file_writes_evidence_note_with_wrapped_origin() {
    let dir = vault();
    seed_portfolio(dir.path());

    file::run(
        dir.path(),
        moment(2026, 3, 15, 10, 0),
        Some("sparse-vs-dense-ood".to_owned()),
        Some("Chen 2025".to_owned()),
        Some("projects/surrogate".to_owned()),
        "Body text.\n".to_owned(),
        None,
        false,
        true,
    )
    .expect("file");

    let path = dir
        .path()
        .join("portfolios/sparse-vs-dense-ood/2026-03-15-chen-2025.md");
    let raw = fs::read_to_string(&path).unwrap();
    assert!(raw.contains("type: evidence"));
    assert!(raw.contains("source: \"Chen 2025\""));
    assert!(raw.contains("origin: \"[[projects/surrogate]]\""));
    assert!(raw.contains("Body text."));
}

#[test]
fn file_writes_empty_body_when_content_omitted() {
    let dir = vault();
    seed_portfolio(dir.path());

    file::run(
        dir.path(),
        moment(2026, 3, 15, 10, 0),
        Some("sparse-vs-dense-ood".to_owned()),
        Some("Bare capture".to_owned()),
        Some("projects/surrogate".to_owned()),
        String::new(),
        None,
        false,
        true,
    )
    .expect("file with empty content");

    let path = dir
        .path()
        .join("portfolios/sparse-vs-dense-ood/2026-03-15-bare-capture.md");
    let raw = fs::read_to_string(&path).unwrap();
    // Frontmatter present, body essentially empty.
    assert!(raw.contains("source: \"Bare capture\""));
    let body = raw.split("---\n").nth(2).unwrap_or("");
    assert!(
        body.trim().is_empty(),
        "body should be empty / whitespace:\n{body:?}"
    );
}

#[test]
fn file_errors_when_missing_portfolio_in_non_interactive() {
    let dir = vault();
    let err = file::run(
        dir.path(),
        moment(2026, 3, 15, 10, 0),
        None,
        Some("Chen 2025".to_owned()),
        Some("projects/foo".to_owned()),
        String::new(),
        None,
        false,
        true,
    )
    .expect_err("missing --portfolio should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("--portfolio"), "error message: {msg}");
}

#[test]
fn file_errors_on_prewrapped_origin() {
    let dir = vault();
    seed_portfolio(dir.path());

    let err = file::run(
        dir.path(),
        moment(2026, 3, 15, 10, 0),
        Some("sparse-vs-dense-ood".to_owned()),
        Some("Chen 2025".to_owned()),
        Some("[[projects/foo]]".to_owned()),
        String::new(),
        None,
        false,
        true,
    )
    .expect_err("pre-wrapped origin should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("malformed wikilink"), "error message: {msg}");
}

// ---------------------------------------------------------------------
// file --attach (#154)
// ---------------------------------------------------------------------

#[test]
fn file_attach_imports_artefact_and_writes_linked_stub() {
    let dir = vault();
    seed_portfolio(dir.path());
    let artefact = dir.path().join("derivation.pdf");
    fs::write(&artefact, b"%PDF fake").unwrap();

    file::run(
        dir.path(),
        moment(2026, 6, 13, 10, 0),
        Some("sparse-vs-dense-ood".to_owned()),
        Some("Chen derivation".to_owned()),
        Some("projects/surrogate".to_owned()),
        "The closed-form bound.".to_owned(),
        Some(artefact.clone()),
        false,
        true,
    )
    .expect("attach");

    let stub = dir
        .path()
        .join("portfolios/sparse-vs-dense-ood/2026-06-13-chen-derivation.md");
    let imported = dir
        .path()
        .join("portfolios/sparse-vs-dense-ood/2026-06-13-chen-derivation/derivation.pdf");
    assert!(stub.is_file(), "stub written");
    assert!(imported.is_file(), "artefact imported into sibling folder");
    let body = fs::read_to_string(&stub).unwrap();
    assert!(body.contains("kind: pdf"), "{body}");
    assert!(
        body.contains("[derivation.pdf](<./2026-06-13-chen-derivation/derivation.pdf>)"),
        "{body}"
    );
    assert!(
        artefact.is_file(),
        "copy (default) leaves the source in place"
    );
}

#[test]
fn file_attach_move_removes_the_source() {
    let dir = vault();
    seed_portfolio(dir.path());
    let artefact = dir.path().join("clip.mp4");
    fs::write(&artefact, b"fake").unwrap();

    file::run(
        dir.path(),
        moment(2026, 6, 13, 10, 0),
        Some("sparse-vs-dense-ood".to_owned()),
        Some("Recording".to_owned()),
        Some("projects/surrogate".to_owned()),
        String::new(),
        Some(artefact.clone()),
        true, // --move
        true,
    )
    .expect("attach move");

    assert!(!artefact.exists(), "--move removes the source");
    assert!(
        dir.path()
            .join("portfolios/sparse-vs-dense-ood/2026-06-13-recording/clip.mp4")
            .is_file()
    );
}
