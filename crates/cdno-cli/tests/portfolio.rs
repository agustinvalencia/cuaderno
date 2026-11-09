//! In-process tests for `commands::portfolio::run` and
//! `commands::file::run`. All tests pass `no_interactive = true` so
//! prompts never fire; missing-flag tests assert the convention.

use std::fs;
use std::path::Path;

use cdno_cli::commands::portfolio::{self, PortfolioCommands};
use cdno_cli::commands::question::{self, QuestionCommands};
use cdno_cli::commands::{file, init};
use cdno_core::error::StoreError;
use cdno_core::path::VaultPath;
use cdno_core::store::VaultStore;
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

/// Create a sample portfolio "sparse-vs-dense" via `portfolio create`.
fn seed_portfolio(root: &Path) {
    portfolio::run(
        root,
        moment(2026, 2, 1, 9, 0),
        PortfolioCommands::Create {
            question: Some("Sparse vs dense OOD".to_owned()),
            project: None,
            var: vec![],
            slug: None,
        },
        true,
        false,
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
            var: vec![],
            slug: None,
        },
        true,
        false,
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
            var: vec![],
            slug: None,
        },
        true,
        false,
    )
    .expect_err("missing --question should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("--question"), "error message: {msg}");
}

#[test]
fn portfolio_create_backlinks_existing_question_note() {
    // The issue #200 repro: create a question, then a portfolio for
    // the same text, end to end through the CLI commands.
    let dir = vault();

    question::run(
        dir.path(),
        moment(2026, 2, 1, 9, 0),
        QuestionCommands::Create {
            domain: Some(QuestionDomain::Research),
            text: Some("Sparse vs dense OOD".to_owned()),
            var: vec![],
            slug: None,
        },
        true,
        false,
    )
    .expect("create question");
    seed_portfolio(dir.path());

    let question_note = dir.path().join("questions/research/sparse-vs-dense-ood.md");
    let raw = fs::read_to_string(&question_note).expect("question note exists");
    assert!(
        raw.contains("## Related Portfolios\n- [[portfolios/sparse-vs-dense-ood/_index]]"),
        "question note should backlink the portfolio:\n{raw}"
    );
}

#[test]
fn a_mixed_case_question_text_creates_a_lowercase_folder() {
    // A case-insensitive disk (macOS) would match `Surrogate-Calibration`
    // to `surrogate-calibration.md` if it were tried as a question
    // reference; only slug-shaped text is, so the folder is slugified.
    let dir = vault();
    question::run(
        dir.path(),
        moment(2026, 2, 1, 9, 0),
        QuestionCommands::Create {
            domain: Some(QuestionDomain::Research),
            text: Some("Surrogate calibration".to_owned()),
            var: vec![],
            slug: None,
        },
        true,
        false,
    )
    .expect("create question");
    portfolio::run(
        dir.path(),
        moment(2026, 2, 1, 9, 5),
        PortfolioCommands::Create {
            question: Some("Surrogate-Calibration".to_owned()),
            project: None,
            var: vec![],
            slug: None,
        },
        true,
        false,
    )
    .expect("create portfolio");

    let names: Vec<String> = fs::read_dir(dir.path().join("portfolios"))
        .expect("portfolios dir")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["surrogate-calibration".to_owned()]);
}

#[test]
fn portfolio_link_backlinks_a_retrofit_question() {
    // A standalone portfolio created first, then a question with a
    // different slug, linked after the fact via `portfolio link`.
    let dir = vault();
    seed_portfolio(dir.path()); // portfolios/sparse-vs-dense-ood
    question::run(
        dir.path(),
        moment(2026, 2, 1, 9, 0),
        QuestionCommands::Create {
            domain: Some(QuestionDomain::Research),
            text: Some("Where does the budget go".to_owned()),
            var: vec![],
            slug: None,
        },
        true,
        false,
    )
    .expect("create question");

    portfolio::run(
        dir.path(),
        moment(2026, 2, 2, 9, 0),
        PortfolioCommands::Link {
            portfolio: Some("sparse-vs-dense-ood".to_owned()),
            question: Some("where-does-the-budget-go".to_owned()),
            project: None,
        },
        true,
        false,
    )
    .expect("link");

    let question_note = dir
        .path()
        .join("questions/research/where-does-the-budget-go.md");
    let question_raw = fs::read_to_string(&question_note).expect("question note exists");
    assert!(
        question_raw.contains("## Related Portfolios\n- [[portfolios/sparse-vs-dense-ood/_index]]"),
        "retrofit link should land in the question note:\n{question_raw}"
    );
    // Both ends are written.
    let portfolio_note = dir.path().join("portfolios/sparse-vs-dense-ood/_index.md");
    let portfolio_raw = fs::read_to_string(&portfolio_note).expect("portfolio note exists");
    assert!(
        portfolio_raw
            .contains("## Related Questions\n- [[questions/research/where-does-the-budget-go]]"),
        "retrofit link should land in the portfolio note:\n{portfolio_raw}"
    );
}

#[test]
fn portfolio_link_errors_when_missing_question_in_non_interactive() {
    let dir = vault();
    seed_portfolio(dir.path());
    let err = portfolio::run(
        dir.path(),
        moment(2026, 2, 1, 9, 0),
        PortfolioCommands::Link {
            portfolio: Some("sparse-vs-dense-ood".to_owned()),
            question: None,
            project: None,
        },
        true,
        false,
    )
    .expect_err("missing both --question and --project should error");
    let msg = format!("{err:#}");
    assert!(msg.contains("--question"), "error message: {msg}");
}

#[test]
fn portfolio_link_to_project_backfills_the_project_map() {
    // A standalone portfolio linked to an existing project via
    // `portfolio link --project`.
    let dir = vault();
    seed_portfolio(dir.path()); // portfolios/sparse-vs-dense-ood
    let project = dir.path().join("projects/surrogate-model.md");
    fs::create_dir_all(project.parent().unwrap()).unwrap();
    fs::write(
        &project,
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\ncore_question: null\n---\n\n# Surrogate Model\n\n## Current State\nGoing.\n\n## Links\n- Portfolio: (none yet)\n",
    )
    .unwrap();

    portfolio::run(
        dir.path(),
        moment(2026, 2, 2, 9, 0),
        PortfolioCommands::Link {
            portfolio: Some("sparse-vs-dense-ood".to_owned()),
            question: None,
            project: Some("projects/surrogate-model".to_owned()),
        },
        true,
        false,
    )
    .expect("link to project");

    let project_raw = fs::read_to_string(&project).expect("project note exists");
    assert!(
        project_raw.contains("- Portfolio: [[portfolios/sparse-vs-dense-ood/_index]]"),
        "project ## Links should list the portfolio:\n{project_raw}"
    );
    let portfolio_raw =
        fs::read_to_string(dir.path().join("portfolios/sparse-vs-dense-ood/_index.md")).unwrap();
    assert!(
        portfolio_raw.contains("project: \"[[projects/surrogate-model]]\""),
        "portfolio frontmatter should record the project:\n{portfolio_raw}"
    );
}

#[test]
fn portfolio_link_errors_when_both_question_and_project_given() {
    let dir = vault();
    seed_portfolio(dir.path());
    let err = portfolio::run(
        dir.path(),
        moment(2026, 2, 1, 9, 0),
        PortfolioCommands::Link {
            portfolio: Some("sparse-vs-dense-ood".to_owned()),
            question: Some("q".to_owned()),
            project: Some("projects/p".to_owned()),
        },
        true,
        false,
    )
    .expect_err("both targets should error");
    assert!(format!("{err:#}").contains("only one"), "error: {err:#}");
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
        vec![],
        file::FileFormat::Markdown,
        None,
        true,
        false,
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
    // origin is its own table column now, no inline "origin:" label.
    assert!(out.contains("[[projects/surrogate]]"));
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
        vec![],
        file::FileFormat::Markdown,
        None,
        true,
        false,
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
        false,
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
        vec![],
        file::FileFormat::Markdown,
        None,
        true,
        false,
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
        vec![],
        file::FileFormat::Markdown,
        None,
        true,
        false,
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
        vec![],
        file::FileFormat::Markdown,
        None,
        true,
        false,
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
        vec![],
        file::FileFormat::Markdown,
        None,
        true,
        false,
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
        vec![],
        file::FileFormat::Markdown,
        None,
        true,
        false,
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
        vec![],
        file::FileFormat::Markdown,
        None,
        true,
        false,
    )
    .expect("attach move");

    assert!(!artefact.exists(), "--move removes the source");
    assert!(
        dir.path()
            .join("portfolios/sparse-vs-dense-ood/2026-06-13-recording/clip.mp4")
            .is_file()
    );
}

/// The binary against `root`, never inheriting the developer's vault.
fn cdno_in(root: &std::path::Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("cdno").expect("cdno binary built");
    cmd.env_remove("CUADERNO_VAULT_PATH");
    cmd.arg("--vault").arg(root);
    cmd
}

#[test]
fn create_with_slug_names_the_file() {
    let dir = vault();
    cdno_in(dir.path())
        .args(["portfolio", "create", "--question"])
        .arg("Does a surrogate beat the simulator?")
        .args(["--slug", "surrogate-model"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "portfolios/surrogate-model/_index.md",
        ));
    assert!(
        dir.path()
            .join("portfolios/surrogate-model/_index.md")
            .is_file()
    );
}

#[test]
fn a_question_slug_is_accepted_and_the_portfolio_takes_its_text() {
    let dir = vault();
    cdno_in(dir.path())
        .args(["question", "create", "--domain", "research"])
        .args(["--text", "Does a surrogate beat the simulator?"])
        .args(["--slug", "surrogate-model"])
        .assert()
        .success();

    // Same slug: the portfolio takes the question's text as its title.
    cdno_in(dir.path())
        .args(["portfolio", "create", "--question", "surrogate-model"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "portfolios/surrogate-model/_index.md",
        ));
    let index =
        fs::read_to_string(dir.path().join("portfolios/surrogate-model/_index.md")).unwrap();
    assert!(
        index.contains("Does a surrogate beat the simulator?"),
        "{index}"
    );
}

#[test]
fn a_mismatched_slug_is_refused_as_json() {
    let dir = vault();
    cdno_in(dir.path())
        .args(["question", "create", "--domain", "research"])
        .args(["--text", "Does a surrogate beat the simulator?"])
        .args(["--slug", "surrogate-model"])
        .assert()
        .success();

    let out = cdno_in(dir.path())
        .args(["--json", "portfolio", "create"])
        .args([
            "--question",
            "surrogate-model",
            "--slug",
            "surrogate-calibration",
        ])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("a JSON object");
    assert_eq!(v["code"], "portfolio_slug_mismatch", "{v}");
    assert_eq!(v["details"]["question_slug"], "surrogate-model", "{v}");
    assert_eq!(v["details"]["slug"], "surrogate-calibration", "{v}");
    assert!(!dir.path().join("portfolios/surrogate-calibration").exists());
}

// ---------------------------------------------------------------------
// Renaming a portfolio (RFC 0007 T22), on real files. The CLI verb is
// T24, so the domain verb is driven through the composition root.
// ---------------------------------------------------------------------

/// Bytes no UTF-8 read would survive, so a move that read the artefact as
/// text could not keep them.
const SCAN_BYTES: &[u8] = b"%PDF-1.4\n\xff\xfe\x00\x01 the residuals";

/// Portfolio `old` with an evidence note and an attached PDF, beside a
/// sibling portfolio `surrogate-emulator` holding an empty `figures/`.
fn seed_renamable_portfolio(root: &Path) {
    for (question, slug) in [
        ("How large is the error?", "old"),
        ("Is the emulator faster?", "surrogate-emulator"),
    ] {
        portfolio::run(
            root,
            moment(2026, 10, 1, 9, 0),
            PortfolioCommands::Create {
                question: Some(question.to_owned()),
                project: None,
                var: vec![],
                slug: Some(slug.to_owned()),
            },
            true,
            false,
        )
        .expect("create portfolio");
    }
    fs::create_dir_all(root.join("portfolios/surrogate-emulator/figures")).unwrap();
    file::run(
        root,
        moment(2026, 10, 12, 10, 0),
        Some("old".to_owned()),
        Some("Baseline".to_owned()),
        Some("portfolios/old".to_owned()),
        "The baseline sweep.\n".to_owned(),
        None,
        false,
        vec![],
        file::FileFormat::Markdown,
        None,
        true,
        false,
    )
    .expect("file evidence");
    let artefact = root.join("scan.pdf");
    fs::write(&artefact, SCAN_BYTES).unwrap();
    file::run(
        root,
        moment(2026, 10, 13, 10, 0),
        Some("old".to_owned()),
        Some("Scan".to_owned()),
        Some("portfolios/old".to_owned()),
        "The residuals, scanned.".to_owned(),
        Some(artefact),
        true,
        vec![],
        file::FileFormat::Markdown,
        None,
        true,
        false,
    )
    .expect("attach");
    assert!(
        root.join("portfolios/old/2026-10-13-scan/scan.pdf")
            .is_file()
    );
}

fn rename_portfolio(root: &Path) {
    let opened = cdno_domain::bootstrap::open_vault(root).expect("open");
    opened
        .vault
        .rename_note(
            "portfolio:old",
            "new",
            None,
            None,
            moment(2026, 10, 14, 9, 0),
            || Ok(()),
        )
        .expect("rename");
}

fn assert_lint_clean(root: &Path) {
    use predicates::prelude::*;
    cdno_in(root)
        .arg("lint")
        .assert()
        .success()
        .stdout(predicate::str::contains("[error]").not())
        .stdout(predicate::str::contains("[warning]").not());
}

#[test]
fn an_emptied_folder_is_pruned() {
    let dir = vault();
    let root = dir.path();
    seed_renamable_portfolio(root);
    assert_lint_clean(root);

    rename_portfolio(root);

    assert!(
        !root.join("portfolios/old").exists(),
        "the old folder and its emptied subfolder are pruned"
    );
    for moved in [
        "portfolios/new/_index.md",
        "portfolios/new/2026-10-12-baseline.md",
        "portfolios/new/2026-10-13-scan.md",
    ] {
        assert!(root.join(moved).is_file(), "{moved}");
    }
    assert_eq!(
        fs::read(root.join("portfolios/new/2026-10-13-scan/scan.pdf")).unwrap(),
        SCAN_BYTES,
        "the artefact's bytes are the ones filed"
    );
    let baseline = fs::read_to_string(root.join("portfolios/new/2026-10-12-baseline.md")).unwrap();
    assert!(baseline.contains("\nportfolio: new\n"), "{baseline}");
    assert!(
        baseline.contains("origin: \"[[portfolios/new]]\""),
        "{baseline}"
    );
    // Nothing above the renamed folder is pruned: the sibling portfolio's
    // empty folder is where it was.
    assert!(root.join("portfolios/surrogate-emulator/figures").is_dir());
    assert_lint_clean(root);
}

/// An `FsVaultStore` with two hooks: once armed, the next walk of the
/// vault root writes `stray` into the old folder on its way out, so the file
/// arrives after the rename's snapshot was taken; and with `fail_moves`
/// every `move_file` fails, so the commit rolls back.
struct HookedFs {
    inner: cdno_core::store::FsVaultStore,
    root: std::path::PathBuf,
    stray: std::sync::Mutex<Option<&'static str>>,
    fail_moves: bool,
}

impl HookedFs {
    fn new(root: &Path) -> Self {
        Self {
            inner: cdno_core::store::FsVaultStore::new(root),
            root: root.to_path_buf(),
            stray: std::sync::Mutex::new(None),
            fail_moves: false,
        }
    }
}

impl VaultStore for HookedFs {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        self.inner.read_file(path)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        self.inner.read_bytes(path)
    }
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.write_file(path, content)
    }
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        self.inner.append_to_file(path, content)
    }
    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError> {
        if self.fail_moves {
            return Err(StoreError::PermissionDenied(dest.to_string()));
        }
        self.inner.move_file(src, dest)
    }
    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError> {
        self.inner.delete_file(path)
    }
    fn remove_dir_if_empty(&self, dir: &VaultPath) -> Result<bool, StoreError> {
        self.inner.remove_dir_if_empty(dir)
    }
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        self.inner.exists(path)
    }
    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.list_dir(path)
    }
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        let walked = self.inner.walk_dir(path);
        if path.as_path().as_os_str().is_empty()
            && let Some(stray) = self.stray.lock().unwrap().take()
        {
            fs::write(self.root.join(stray), b"arrived late").unwrap();
        }
        walked
    }
    fn walk_dirs(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.walk_dirs(path)
    }
    fn metadata(&self, path: &VaultPath) -> Result<cdno_core::file_meta::FileMeta, StoreError> {
        self.inner.metadata(path)
    }
    fn import_external(&self, src: &Path, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.import_external(src, dest)
    }
    fn acquire_write_lock(&self) -> Result<cdno_core::store::VaultWriteLock, StoreError> {
        self.inner.acquire_write_lock()
    }
}

/// A vault at `root` over `store` and the real SQLite index.
fn vault_over(root: &Path, store: std::sync::Arc<HookedFs>) -> cdno_domain::Vault {
    let config = cdno_core::config::VaultConfig::load(root).unwrap();
    let index = cdno_core::index::SqliteIndex::open(root.join(cdno_core::paths::INDEX_DB)).unwrap();
    cdno_domain::Vault::new(store, std::sync::Arc::new(index), config)
        .expect("vault opens")
        .0
}

/// A `.DS_Store`-like file that arrives in the old folder after the
/// rename's snapshot was taken, so no plan could list it: the folder stays,
/// holding it, and the rename still succeeded.
#[test]
fn a_folder_with_a_stray_file_is_left() {
    let dir = vault();
    let root = dir.path();
    seed_renamable_portfolio(root);
    let store = std::sync::Arc::new(HookedFs::new(root));
    let vault = vault_over(root, store.clone());
    // Armed after the open, so the rename's own snapshot is the walk that
    // misses it: `.DS_Store` arrives in the old folder once the plan is
    // taken, as Finder writes it into a folder it is showing.
    *store.stray.lock().unwrap() = Some("portfolios/old/.DS_Store");
    vault
        .rename_note(
            "portfolio:old",
            "new",
            None,
            None,
            moment(2026, 10, 14, 9, 0),
            || Ok(()),
        )
        .expect("the rename succeeds");

    // Everything the plan listed moved; the folder the late file keeps
    // stays, holding only it, and the subfolder the move emptied is pruned.
    assert!(root.join("portfolios/new/_index.md").is_file());
    assert!(
        root.join("portfolios/new/2026-10-13-scan/scan.pdf")
            .is_file()
    );
    let left: Vec<String> = fs::read_dir(root.join("portfolios/old"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(left, vec![".DS_Store".to_owned()]);
    drop(vault);
    assert_lint_clean(root);
}

#[test]
fn a_failed_portfolio_rename_leaves_no_new_folder_behind() {
    let dir = vault();
    let root = dir.path();
    seed_renamable_portfolio(root);
    let store = std::sync::Arc::new(HookedFs {
        fail_moves: true,
        ..HookedFs::new(root)
    });
    let vault = vault_over(root, store);
    vault
        .rename_note(
            "portfolio:old",
            "new",
            None,
            None,
            moment(2026, 10, 14, 9, 0),
            || Ok(()),
        )
        .expect_err("the artefact's move fails, so the commit rolls back");
    drop(vault);

    // The note written into the new folder before the move failed was taken
    // back out, and the folders its write created went with it.
    assert!(!root.join("portfolios/new").exists());
    assert_eq!(
        fs::read(root.join("portfolios/old/2026-10-13-scan/scan.pdf")).unwrap(),
        SCAN_BYTES
    );
    assert!(root.join("portfolios/old/_index.md").is_file());

    // A second attempt proceeds.
    rename_portfolio(root);
    assert!(
        root.join("portfolios/new/2026-10-13-scan/scan.pdf")
            .is_file()
    );
    assert!(!root.join("portfolios/old").exists());
    assert_lint_clean(root);
}

#[test]
fn an_empty_destination_folder_is_free() {
    // A `portfolios/new/` holding no file at any depth (made by hand, or
    // left by an attempt whose prune could not run) holds nothing to merge
    // with: the rename moves into it.
    let dir = vault();
    let root = dir.path();
    seed_renamable_portfolio(root);
    fs::create_dir_all(root.join("portfolios/new/figures")).unwrap();
    rename_portfolio(root);
    assert!(root.join("portfolios/new/_index.md").is_file());
    assert!(!root.join("portfolios/old").exists());

    // One holding a file is still taken.
    let dir = vault();
    let root = dir.path();
    seed_renamable_portfolio(root);
    fs::create_dir_all(root.join("portfolios/new")).unwrap();
    fs::write(root.join("portfolios/new/notes.txt"), b"mine").unwrap();
    let opened = cdno_domain::bootstrap::open_vault(root).expect("open");
    let err = opened
        .vault
        .rename_note(
            "portfolio:old",
            "new",
            None,
            None,
            moment(2026, 10, 14, 9, 0),
            || Ok(()),
        )
        .expect_err("a folder holding a file is taken");
    assert!(
        matches!(err, cdno_domain::error::DomainError::SlugTaken { .. }),
        "{err:?}"
    );
}

/// Whether the filesystem under `dir` folds case (the macOS default):
/// a `probe-A` written there is also found as `probe-a`.
fn folds_case(dir: &Path) -> bool {
    fs::write(dir.join("probe-A"), b"").unwrap();
    let folds = dir.join("probe-a").exists();
    fs::remove_file(dir.join("probe-A")).unwrap();
    folds
}

#[test]
fn an_empty_case_variant_destination_is_pruned_or_refused() {
    let dir = vault();
    let root = dir.path();
    let case_insensitive = folds_case(root);
    seed_renamable_portfolio(root);
    fs::create_dir_all(root.join("portfolios/New")).unwrap();

    let opened = cdno_domain::bootstrap::open_vault(root).expect("open");
    opened
        .vault
        .rename_note(
            "portfolio:old",
            "new",
            None,
            None,
            moment(2026, 10, 14, 9, 0),
            || Ok(()),
        )
        .expect("the empty variant is pruned, or is another folder");
    drop(opened);
    let listed: Vec<String> = fs::read_dir(root.join("portfolios"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    // Either way the files are under a folder listed as exactly `new`.
    assert!(listed.contains(&"new".to_owned()), "{listed:?}");
    assert!(root.join("portfolios/new/_index.md").is_file());
    if case_insensitive {
        // `New` and `new` are one folder here: the empty variant was
        // pruned under the lock and the folder recreated in the right case.
        assert!(!listed.contains(&"New".to_owned()), "{listed:?}");
    } else {
        // Two folders here: the empty `New/` is someone else's, untouched.
        assert!(listed.contains(&"New".to_owned()), "{listed:?}");
        assert!(root.join("portfolios/New").is_dir());
        assert_eq!(
            fs::read_dir(root.join("portfolios/New")).unwrap().count(),
            0
        );
    }
    assert_lint_clean(root);
}

#[test]
fn the_preview_refuses_an_empty_case_variant_it_may_not_prune() {
    let dir = vault();
    let root = dir.path();
    let case_insensitive = folds_case(root);
    seed_renamable_portfolio(root);
    fs::create_dir_all(root.join("portfolios/New")).unwrap();
    let opened = cdno_domain::bootstrap::open_vault(root).expect("open");
    let preview =
        opened
            .vault
            .rename_preview("portfolio:old", "new", None, moment(2026, 10, 14, 9, 0));
    if case_insensitive {
        let err = preview.expect_err("a case variant is not the folder's name");
        assert!(
            matches!(err, cdno_domain::error::DomainError::SlugTaken { .. }),
            "{err:?}"
        );
    } else {
        preview.expect("no `new` at all on a case-sensitive filesystem");
    }
    assert!(
        root.join("portfolios/New").is_dir(),
        "the preview prunes nothing"
    );

    // An empty folder under exactly the name is free for the preview too.
    fs::remove_dir(root.join("portfolios/New")).unwrap();
    fs::create_dir_all(root.join("portfolios/new/figures")).unwrap();
    opened
        .vault
        .rename_preview("portfolio:old", "new", None, moment(2026, 10, 14, 9, 0))
        .expect("an empty folder of the right name is free");
}

#[test]
fn a_dangling_symlinked_artefact_is_refused_before_the_write() {
    let dir = vault();
    let root = dir.path();
    seed_renamable_portfolio(root);
    std::os::unix::fs::symlink(
        root.join("nowhere.pdf"),
        root.join("portfolios/old/2026-10-13-scan/link.pdf"),
    )
    .unwrap();
    let opened = cdno_domain::bootstrap::open_vault(root).expect("open");
    for refused in [
        opened
            .vault
            .rename_preview("portfolio:old", "new", None, moment(2026, 10, 14, 9, 0)),
        opened
            .vault
            .rename_note(
                "portfolio:old",
                "new",
                None,
                None,
                moment(2026, 10, 14, 9, 0),
                || Ok(()),
            )
            .map(|o| o.plan),
    ] {
        let err = refused.expect_err("the link reaches nothing");
        assert!(
            matches!(
                err,
                cdno_domain::error::DomainError::Store(
                    StoreError::NotFound(_) | StoreError::OutsideVault(_)
                )
            ),
            "{err:?}"
        );
    }
    assert!(!root.join("portfolios/new").exists(), "nothing was written");
    assert!(root.join("portfolios/old/_index.md").is_file());
}

// ---------------------------------------------------------------------
// file --format typst (RFC 0008 T20)
// ---------------------------------------------------------------------

/// A vault with a portfolio and an origin project, so `cdno lint` has
/// nothing to say about the filed note.
fn typst_filing_vault() -> TempDir {
    let dir = vault();
    seed_portfolio(dir.path());
    cdno_in(dir.path())
        .args([
            "--no-interactive",
            "project",
            "create",
            "--title",
            "Surrogate model",
            "--context",
            "work",
        ])
        .assert()
        .success();
    dir
}

fn file_typst_args(root: &Path) -> assert_cmd::Command {
    let mut cmd = cdno_in(root);
    cmd.args([
        "--no-interactive",
        "file",
        "--portfolio",
        "sparse-vs-dense-ood",
        "--source",
        "Error budget derivation",
        "--origin",
        "projects/surrogate-model",
        "--format",
        "typst",
    ]);
    cmd
}

#[test]
fn file_format_typst_writes_a_typ_note() {
    use predicates::prelude::*;
    let dir = typst_filing_vault();
    let body = dir.path().join("derivation.typ");
    fs::write(&body, "The error splits as $ sigma^2 $.\n").unwrap();

    file_typst_args(dir.path())
        .arg("--body-file")
        .arg(&body)
        .assert()
        .success()
        .stdout(predicate::str::contains(".typ"));

    let rel = fs::read_dir(dir.path().join("portfolios/sparse-vs-dense-ood"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .find(|n| n.ends_with(".typ"))
        .expect("a .typ note");
    let raw =
        fs::read_to_string(dir.path().join("portfolios/sparse-vs-dense-ood").join(&rel)).unwrap();
    assert!(raw.starts_with("#metadata(("), "{raw}");
    assert!(
        raw.ends_with("<cdno>\n\nThe error splits as $ sigma^2 $.\n"),
        "{raw}"
    );

    cdno_in(dir.path())
        .args([
            "open",
            "--path",
            &format!("portfolios/sparse-vs-dense-ood/{rel}"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(".typ"));
    assert_lint_clean(dir.path());
}

#[test]
fn file_format_typst_takes_content_as_the_body() {
    let dir = typst_filing_vault();
    file_typst_args(dir.path())
        .args(["--content", "A short derivation.\n"])
        .assert()
        .success();
}

#[test]
fn file_format_typst_without_a_body_fails_non_interactively() {
    use predicates::prelude::*;
    let dir = typst_filing_vault();
    file_typst_args(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("body-file"));
}

#[test]
fn file_format_typst_refuses_var() {
    let dir = typst_filing_vault();
    let out = file_typst_args(dir.path())
        .args(["--content", "Body.\n", "--var", "k=v", "--json"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["code"], "vars_unsupported_for_typst");
    assert!(json["details"]["remedy"].is_string());
}

#[test]
fn file_format_typst_refuses_an_unescaped_link_with_remedy() {
    let dir = typst_filing_vault();
    let body = dir.path().join("bad.typ");
    fs::write(&body, "See [[concepts/bias#variance]] here.\n").unwrap();
    let out = file_typst_args(dir.path())
        .arg("--body-file")
        .arg(&body)
        .arg("--json")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["code"], "link_needs_escape");
    assert!(
        json["details"]["remedy"]
            .as_str()
            .unwrap()
            .contains("Backslash")
    );
    assert!(json["message"].as_str().unwrap().contains("Backslash"));
}

#[test]
fn file_format_typst_text_refusal_carries_the_remedy() {
    use predicates::prelude::*;
    let dir = typst_filing_vault();
    let body = dir.path().join("bad.typ");
    fs::write(&body, "See [[concepts/bias#variance]] here.\n").unwrap();
    file_typst_args(dir.path())
        .arg("--body-file")
        .arg(&body)
        .assert()
        .failure()
        .stderr(predicate::str::contains("Backslash-escape"));
}

#[test]
fn file_format_typst_with_attach_is_a_clear_error() {
    use predicates::prelude::*;
    let dir = typst_filing_vault();
    let art = dir.path().join("fig.png");
    fs::write(&art, b"png").unwrap();
    file_typst_args(dir.path())
        .arg("--attach")
        .arg(&art)
        .assert()
        .failure()
        .stderr(predicate::str::contains("later release"));
}

#[test]
fn file_without_format_is_unchanged() {
    let dir = typst_filing_vault();
    cdno_in(dir.path())
        .args([
            "--no-interactive",
            "file",
            "--portfolio",
            "sparse-vs-dense-ood",
            "--source",
            "Chen 2025",
            "--origin",
            "projects/surrogate-model",
            "--content",
            "Body text.\n",
        ])
        .assert()
        .success();
    let names: Vec<String> = fs::read_dir(dir.path().join("portfolios/sparse-vs-dense-ood"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(
        names.iter().any(|n| n.ends_with("-chen-2025.md")),
        "{names:?}"
    );
    assert!(!names.iter().any(|n| n.ends_with(".typ")), "{names:?}");
}
