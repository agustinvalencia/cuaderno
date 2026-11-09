//! In-process tests for the 9 operation handlers implemented in #47.
//!
//! Same pattern as `handlers_context.rs`: call the handler methods
//! directly on `CuadernoServer` (they're `pub async fn`) with
//! `Parameters(input)`, decode the JSON payload of the returned
//! `CallToolResult`, and assert on shape + side effects.
//!
//! Operation handlers all return a `WriteResultDto { path, message,
//! verification }` and have a side effect on the vault — we assert
//! both. The `verification` half (GH #539) has its own target,
//! `handlers_verification.rs`; nothing here needs to restate it.

use std::sync::Arc;

use cdno_core::config::{StateOverflow, VaultConfig};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::{Context, EnergyLevel, QuestionDomain};
use cdno_mcp::CuadernoServer;
use cdno_mcp::server::{
    ActionQueryInput, AddActionInput, AddMilestoneInput, AddPeriodicCommitmentInput,
    AddWaitingOnInput, AppendToLogInput, CaptureInput, CompleteCommitmentInput,
    CompleteMilestoneInput, CreateCommitmentInput, CreateCustomNoteInput, CreatePortfolioInput,
    CreateProjectInput, CreateQuestionInput, CreateStewardshipInput, CreateTrackingEntryInput,
    DiscardInboxItemInput, DropActionInput, DropProjectInput, FileToPortfolioInput,
    HandOffProjectInput, LinkPortfolioToProjectInput, LinkPortfolioToQuestionInput,
    NoteToDailyInput, OpenItemsChoice, PauseActionInput, ProjectSlugInput, PromoteActionInput,
    ReadDailyNoteInput, ReadMonthlyNoteInput, ReadNoteInput, ReadWeeklyNoteInput, RenameNoteInput,
    ResolveWaitingOnInput, ResumeActionInput, ReviseNoteInput, SetCoreQuestionInput,
    SetFrontmatterInput, SetQuestionStatusInput, StartActionInput, StartUnplannedActionInput,
    SwitchActionInput, SwitchUnplannedActionInput, UpdateProjectStateInput,
    UpsertDailySectionInput, UpsertMonthlySectionInput, UpsertWeeklySectionInput,
};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorCode, RawContent};

fn moment(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

fn vp(p: &str) -> VaultPath {
    VaultPath::new(p).unwrap()
}

/// Build a `CuadernoServer` from a populated vault — the seed
/// closure runs against the `Vault` before the server wraps it, so
/// tests can use any domain method to set up state.
fn server_with<F: FnOnce(&Vault, Arc<dyn VaultStore>)>(
    seed: F,
) -> (CuadernoServer, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(Arc::clone(&store), index, VaultConfig::default()).unwrap();
    seed(&vault, Arc::clone(&store));
    (CuadernoServer::new(Arc::new(vault), scratch_root()), store)
}

/// Like [`server_with`] but with a caller-supplied `VaultConfig`, so tests
/// can exercise templating that depends on `[variables.prompt]` config.
fn server_with_config<F: FnOnce(&Vault, Arc<dyn VaultStore>)>(
    config: VaultConfig,
    seed: F,
) -> (CuadernoServer, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(Arc::clone(&store), index, config).unwrap();
    seed(&vault, Arc::clone(&store));
    (CuadernoServer::new(Arc::new(vault), scratch_root()), store)
}

/// A `VaultConfig` whose `[variables.prompt]` map has a single `name → message`
/// entry — enough to make a custom template's placeholder prompt-defined.
fn config_with_prompt(name: &str, message: &str) -> VaultConfig {
    let mut config = VaultConfig::default();
    config
        .variables
        .prompt
        .insert(name.to_owned(), message.to_owned());
    config
}

/// A custom project template referencing a `{{ticket}}` placeholder, used by
/// the `vars` tests to prove prompted values flow through the MCP handler.
const PROJECT_WITH_TICKET: &str = "---\ntype: project\ncontext: {{context}}\nstatus: {{status}}\ncreated: {{created}}\nticket: {{ticket}}\n---\n# {{title}}\n";

fn decode_json(result: &CallToolResult) -> serde_json::Value {
    assert_eq!(
        result.is_error,
        Some(false),
        "tool returned an error result: {result:?}"
    );
    assert_eq!(result.content.len(), 1, "expected exactly one content item");
    match &result.content[0].raw {
        RawContent::Text(t) => serde_json::from_str(&t.text).expect("valid JSON payload"),
        other => panic!("expected text content carrying JSON, got {other:?}"),
    }
}

/// Pre-seed today's daily note with a `## Logs` section so handlers
/// that stage_daily_log don't fail on the missing-section path. The
/// real binary always uses `chrono::Local::now()`, which we can't
/// pin per-test; tests instead seed for *whatever today happens to
/// be when the test runs*. The handler then appends into that file.
fn seed_today_daily(store: &Arc<dyn VaultStore>) {
    let today = chrono::Local::now().date_naive();
    let path = vp(&cdno_core::paths::daily_note_relpath(today));
    let body = format!(
        "---\ndate: {date}\ntype: daily\n---\n\n# {date}\n\n## Logs\n",
        date = today.format("%Y-%m-%d"),
    );
    store.write_file(&path, &body).unwrap();
}

// ---------------------------------------------------------------------
// append_to_log
// ---------------------------------------------------------------------

#[tokio::test]
async fn append_to_log_writes_into_today_daily() {
    let (server, store) = server_with(|_v, store| seed_today_daily(&store));

    let result = server
        .append_to_log(Parameters(AppendToLogInput {
            text: "captured from MCP".to_owned(),
        }))
        .await
        .expect("append_to_log");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert!(path.ends_with(".md"), "path: {path}");

    let body = store.read_file(&vp(path)).unwrap();
    assert!(body.contains("captured from MCP"), "body:\n{body}");
}

// ---------------------------------------------------------------------
// file_to_portfolio
// ---------------------------------------------------------------------

#[tokio::test]
async fn file_to_portfolio_creates_evidence_note() {
    let (server, store) = server_with(|vault, _s| {
        vault
            .create_portfolio(
                moment(2026, 2, 1, 9, 0),
                "Does sparse beat dense?",
                Some("projects/surrogate"),
            )
            .unwrap();
    });

    let result = server
        .file_to_portfolio(Parameters(FileToPortfolioInput {
            portfolio: "does-sparse-beat-dense".to_owned(),
            source: "Chen 2025".to_owned(),
            origin: "projects/surrogate".to_owned(),
            content: "4x speedup at 95% accuracy.".to_owned(),
            attach: None,
            vars: None,
            format: None,
        }))
        .await
        .expect("file_to_portfolio");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert!(
        path.starts_with("portfolios/does-sparse-beat-dense/") && path.ends_with("chen-2025.md"),
        "path: {path}"
    );
    let body = store.read_file(&vp(path)).unwrap();
    assert!(body.contains("4x speedup at 95% accuracy."));
    assert!(body.contains("origin: \"[[projects/surrogate]]\""));
}

#[tokio::test]
async fn file_to_portfolio_errors_on_missing_portfolio() {
    let (server, _store) = server_with(|_v, _s| ());
    let err = server
        .file_to_portfolio(Parameters(FileToPortfolioInput {
            portfolio: "nonexistent".to_owned(),
            source: "x".to_owned(),
            origin: "projects/foo".to_owned(),
            content: String::new(),
            attach: None,
            vars: None,
            format: None,
        }))
        .await
        .expect_err("missing portfolio should error");
    assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
}

const TYPST_EVIDENCE: &str = "#import \"@preview/some-template:0.3.0\": report
#show: report.with(title: \"Error budget derivation\")

= Setting

The surrogate's error splits as in [[concepts/bias-variance-split]].
";

fn typst_filing(format: Option<&str>, content: &str) -> FileToPortfolioInput {
    FileToPortfolioInput {
        portfolio: "surrogate-error-budget".to_owned(),
        source: "Error budget derivation".to_owned(),
        origin: "projects/surrogate-model".to_owned(),
        content: content.to_owned(),
        attach: None,
        vars: None,
        format: format.map(str::to_owned),
    }
}

fn typst_filing_server() -> (CuadernoServer, Arc<dyn VaultStore>) {
    server_with(|vault, _s| {
        vault
            .create_portfolio(moment(2026, 2, 1, 9, 0), "Surrogate error budget", None)
            .unwrap();
    })
}

fn tree(store: &Arc<dyn VaultStore>) -> Vec<(String, String)> {
    let mut all: Vec<(String, String)> = store
        .walk_dir(&VaultPath::root())
        .unwrap()
        .into_iter()
        .map(|p| (p.to_string(), store.read_file(&p).unwrap_or_default()))
        .collect();
    all.sort();
    all
}

#[tokio::test]
async fn file_to_portfolio_with_format_typst_writes_a_typ_note() {
    let (server, store) = typst_filing_server();
    let result = server
        .file_to_portfolio(Parameters(typst_filing(Some("typst"), TYPST_EVIDENCE)))
        .await
        .expect("file_to_portfolio");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert!(
        path.starts_with("portfolios/surrogate-error-budget/")
            && path.ends_with("-error-budget-derivation.typ"),
        "path: {path}"
    );
    let note = store.read_file(&vp(path)).unwrap();
    assert!(note.contains("<cdno>"), "{note}");
    assert!(note.ends_with(&format!("\n\n{TYPST_EVIDENCE}")), "{note}");
    assert!(
        note.contains("origin: \"[[projects/surrogate-model]]\""),
        "{note}"
    );
}

#[tokio::test]
async fn file_to_portfolio_typst_refuses_vars_with_a_code() {
    let (server, store) = typst_filing_server();
    let before = tree(&store);
    let mut input = typst_filing(Some("typst"), TYPST_EVIDENCE);
    input.vars = Some(std::collections::HashMap::from([(
        "x".to_owned(),
        "y".to_owned(),
    )]));
    let err = server
        .file_to_portfolio(Parameters(input))
        .await
        .expect_err("vars with typst are refused");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "vars_unsupported_for_typst");
    assert!(
        !rejection["details"]["remedy"]
            .as_str()
            .unwrap_or("")
            .is_empty(),
        "{rejection}"
    );
    assert_eq!(tree(&store), before);
}

#[tokio::test]
async fn file_to_portfolio_typst_refuses_an_unescaped_link_with_line_and_remedy() {
    let (server, store) = typst_filing_server();
    let before = tree(&store);
    let body = "= Setting\n\nSee [[concepts/bias-variance-split#a#b]] now.\n";
    let err = server
        .file_to_portfolio(Parameters(typst_filing(Some("typst"), body)))
        .await
        .expect_err("refused");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "link_needs_escape");
    assert_eq!(rejection["details"]["line"], 3);
    assert_eq!(rejection["details"]["leaf"], "Hash");
    assert!(
        !rejection["details"]["remedy"]
            .as_str()
            .unwrap_or("")
            .is_empty(),
        "{rejection}"
    );
    assert_eq!(tree(&store), before);
}

#[tokio::test]
async fn file_to_portfolio_typst_refuses_a_shared_stem_naming_the_holder() {
    let (server, store) = typst_filing_server();
    let today = chrono::Local::now().format("%Y-%m-%d");
    let holder = format!("portfolios/surrogate-error-budget/{today}-error-budget-derivation.md");
    store.write_file(&vp(&holder), "# Held\n").unwrap();
    let err = server
        .file_to_portfolio(Parameters(typst_filing(Some("typst"), TYPST_EVIDENCE)))
        .await
        .expect_err("refused");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "stem_taken_by_other_format");
    assert_eq!(rejection["details"]["holder"], holder.as_str());
}

#[tokio::test]
async fn file_to_portfolio_typst_refuses_a_non_typ_attach_and_an_unknown_format_as_invalid_params()
{
    let (server, store) = typst_filing_server();
    let before = tree(&store);
    let mut input = typst_filing(Some("typst"), "");
    input.attach = Some("/nowhere/figure.pdf".to_owned());
    let err = server
        .file_to_portfolio(Parameters(input))
        .await
        .expect_err("attach with typst");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("'attach'"), "{err:?}");

    let err = server
        .file_to_portfolio(Parameters(typst_filing(Some("latex"), "x")))
        .await
        .expect_err("unknown format");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("'format'"), "{err:?}");
    assert_eq!(tree(&store), before);
}

fn adoption_source() -> tempfile::TempDir {
    let src = tempfile::tempdir().unwrap();
    std::fs::write(
        src.path().join("deck.typ"),
        "= Error budget\n#image(\"figs/a.png\")\n#bibliography(\"refs.bib\")\n",
    )
    .unwrap();
    std::fs::create_dir(src.path().join("figs")).unwrap();
    std::fs::write(src.path().join("figs/a.png"), b"png").unwrap();
    std::fs::write(src.path().join("refs.bib"), "@misc{x,}").unwrap();
    std::fs::write(src.path().join("out.pdf"), b"%PDF").unwrap();
    src
}

fn adopting(deck: &std::path::Path) -> FileToPortfolioInput {
    let mut input = typst_filing(Some("typst"), "");
    input.attach = Some(deck.to_string_lossy().into_owned());
    input
}

#[tokio::test]
async fn file_to_portfolio_typst_attach_adopts_and_lists_rewrites() {
    let (server, store) = typst_filing_server();
    let src = adoption_source();
    let result = server
        .file_to_portfolio(Parameters(adopting(&src.path().join("deck.typ"))))
        .await
        .expect("adopts");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert!(path.ends_with("-error-budget-derivation.typ"), "{path}");
    assert_eq!(value["verification"]["verified"], "content", "{value}");
    let rewrites = value["rewrites"].as_array().unwrap();
    assert_eq!(rewrites.len(), 2, "{value}");
    assert_eq!(rewrites[0]["was"], "figs/a.png");
    let stem = path.strip_suffix(".typ").unwrap();
    for rel in ["figs/a.png", "refs.bib", "out.pdf"] {
        assert!(
            store.exists(&vp(&format!("{stem}/{rel}"))).unwrap(),
            "{rel}"
        );
    }
    let folder = stem.rsplit('/').next().unwrap();
    let note = store.read_file(&vp(path)).unwrap();
    assert!(note.contains(&format!("{folder}/figs/a.png")), "{note}");
    assert!(src.path().join("figs/a.png").is_file(), "source untouched");
}

#[tokio::test]
async fn file_to_portfolio_typst_attach_refuses_an_escape_and_a_symlink_writing_nothing() {
    let (server, store) = typst_filing_server();
    let before = tree(&store);
    let src = adoption_source();
    std::fs::write(src.path().join("deck.typ"), "#image(\"../x.png\")\n").unwrap();
    let err = server
        .file_to_portfolio(Parameters(adopting(&src.path().join("deck.typ"))))
        .await
        .expect_err("escape");
    assert_eq!(rejection_of(&err)["code"], "asset_path_outside_folder");

    #[cfg(unix)]
    {
        let src = adoption_source();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("s.txt"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path().join("s.txt"), src.path().join("link.txt"))
            .unwrap();
        let err = server
            .file_to_portfolio(Parameters(adopting(&src.path().join("deck.typ"))))
            .await
            .expect_err("symlink");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("link.txt"), "{err:?}");
    }
    assert_eq!(tree(&store), before);
}

#[tokio::test]
async fn file_to_portfolio_typst_attach_refuses_content() {
    let (server, _store) = typst_filing_server();
    let src = adoption_source();
    let mut input = adopting(&src.path().join("deck.typ"));
    input.content = "extra".to_owned();
    let err = server
        .file_to_portfolio(Parameters(input))
        .await
        .expect_err("content with attach");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
}

#[tokio::test]
async fn file_to_portfolio_typst_refuses_a_blank_body() {
    let (server, store) = typst_filing_server();
    let before = tree(&store);
    let err = server
        .file_to_portfolio(Parameters(typst_filing(Some("typst"), "  \n")))
        .await
        .expect_err("refused");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "empty_field");
    assert_eq!(rejection["details"]["field"], "content");
    assert_eq!(tree(&store), before);
}

#[tokio::test]
async fn file_to_portfolio_without_format_is_markdown() {
    let (server, _store) = typst_filing_server();
    let result = server
        .file_to_portfolio(Parameters(typst_filing(None, "Plain finding.")))
        .await
        .expect("file_to_portfolio");
    let value = decode_json(&result);
    assert!(value["path"].as_str().unwrap().ends_with(".md"), "{value}");
    let mut second = typst_filing(Some("markdown"), "Another.");
    second.source = "Second derivation".to_owned();
    let result = server
        .file_to_portfolio(Parameters(second))
        .await
        .expect("file_to_portfolio");
    let value = decode_json(&result);
    assert!(value["path"].as_str().unwrap().ends_with(".md"), "{value}");
}

// ---------------------------------------------------------------------
// link_portfolio_to_question (#200 retrofit verb)
// ---------------------------------------------------------------------

#[tokio::test]
async fn link_portfolio_to_question_backlinks_the_question_note() {
    let (server, store) = server_with(|vault, _s| {
        vault
            .create_question(
                moment(2026, 2, 1, 9, 0),
                QuestionDomain::Research,
                "Where does the budget go",
            )
            .unwrap();
        vault
            .create_portfolio(moment(2026, 2, 1, 9, 0), "Sparse vs dense OOD", None)
            .unwrap();
    });

    let result = server
        .link_portfolio_to_question(Parameters(LinkPortfolioToQuestionInput {
            portfolio: "sparse-vs-dense-ood".to_owned(),
            question: "where-does-the-budget-go".to_owned(),
        }))
        .await
        .expect("link_portfolio_to_question");

    let value = decode_json(&result);
    assert_eq!(
        value["path"].as_str().unwrap(),
        "questions/research/where-does-the-budget-go.md"
    );
    let question_body = store
        .read_file(&vp("questions/research/where-does-the-budget-go.md"))
        .unwrap();
    assert!(
        question_body
            .contains("## Related Portfolios\n- [[portfolios/sparse-vs-dense-ood/_index]]"),
        "question note should backlink the portfolio:\n{question_body}"
    );
    let portfolio_body = store
        .read_file(&vp("portfolios/sparse-vs-dense-ood/_index.md"))
        .unwrap();
    assert!(
        portfolio_body
            .contains("## Related Questions\n- [[questions/research/where-does-the-budget-go]]"),
        "portfolio should link to the question:\n{portfolio_body}"
    );
}

#[tokio::test]
async fn link_portfolio_to_question_errors_on_missing_question() {
    let (server, _store) = server_with(|vault, _s| {
        vault
            .create_portfolio(moment(2026, 2, 1, 9, 0), "Sparse vs dense OOD", None)
            .unwrap();
    });
    let err = server
        .link_portfolio_to_question(Parameters(LinkPortfolioToQuestionInput {
            portfolio: "sparse-vs-dense-ood".to_owned(),
            question: "no-such-question".to_owned(),
        }))
        .await
        .expect_err("missing question should error");
    assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
}

#[tokio::test]
async fn link_portfolio_to_project_backfills_the_project_map() {
    let (server, store) = server_with(|vault, _s| {
        seed_active_project(vault); // projects/surrogate-model.md
        vault
            .create_portfolio(moment(2026, 2, 1, 9, 0), "Sparse vs dense OOD", None)
            .unwrap();
    });

    let result = server
        .link_portfolio_to_project(Parameters(LinkPortfolioToProjectInput {
            portfolio: "sparse-vs-dense-ood".to_owned(),
            project: "projects/surrogate-model".to_owned(),
        }))
        .await
        .expect("link_portfolio_to_project");

    let value = decode_json(&result);
    assert_eq!(
        value["path"].as_str().unwrap(),
        "projects/surrogate-model.md"
    );
    let project_body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(
        project_body.contains("- Portfolio: [[portfolios/sparse-vs-dense-ood/_index]]"),
        "project ## Links should list the portfolio:\n{project_body}"
    );
    let portfolio_body = store
        .read_file(&vp("portfolios/sparse-vs-dense-ood/_index.md"))
        .unwrap();
    assert!(
        portfolio_body.contains("project: \"[[projects/surrogate-model]]\""),
        "portfolio frontmatter should record the project:\n{portfolio_body}"
    );
}

#[tokio::test]
async fn link_portfolio_to_project_errors_on_missing_project() {
    let (server, _store) = server_with(|vault, _s| {
        vault
            .create_portfolio(moment(2026, 2, 1, 9, 0), "Sparse vs dense OOD", None)
            .unwrap();
    });
    let err = server
        .link_portfolio_to_project(Parameters(LinkPortfolioToProjectInput {
            portfolio: "sparse-vs-dense-ood".to_owned(),
            project: "projects/ghost".to_owned(),
        }))
        .await
        .expect_err("missing project should error");
    assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
}

#[tokio::test]
async fn link_portfolio_to_project_errors_on_missing_portfolio() {
    let (server, _store) = server_with(|vault, _s| {
        seed_active_project(vault); // projects/surrogate-model.md
    });
    let err = server
        .link_portfolio_to_project(Parameters(LinkPortfolioToProjectInput {
            portfolio: "no-such-portfolio".to_owned(),
            project: "projects/surrogate-model".to_owned(),
        }))
        .await
        .expect_err("missing portfolio should error");
    assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
}

// ---------------------------------------------------------------------
// update_project_state
// ---------------------------------------------------------------------

#[tokio::test]
async fn update_project_state_rewrites_section_and_logs() {
    let (server, store) = server_with(|vault, store| {
        seed_today_daily(&store);
        vault
            .create_project(
                moment(2026, 5, 1, 9, 0),
                "Surrogate model",
                Context::Work,
                None,
            )
            .unwrap();
    });

    let result = server
        .update_project_state(Parameters(UpdateProjectStateInput {
            project: "surrogate-model".to_owned(),
            new_state: "Sweep B underway, results by Friday.".to_owned(),
        }))
        .await
        .expect("update_project_state");
    let value = decode_json(&result);
    assert!(
        value["path"]
            .as_str()
            .unwrap()
            .ends_with("surrogate-model.md")
    );
    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(body.contains("Sweep B underway, results by Friday."));
}

#[tokio::test]
async fn update_project_state_reject_mode_surfaces_an_error_over_the_limit() {
    // Reject is the default policy; a low cap makes the limit easy to hit.
    let mut config = VaultConfig::default();
    config.vault.max_state_chars = 20;
    let (server, store) = server_with_config(config, |vault, store| {
        seed_today_daily(&store);
        vault
            .create_project(
                moment(2026, 5, 1, 9, 0),
                "Surrogate model",
                Context::Work,
                None,
            )
            .unwrap();
    });

    let err = server
        .update_project_state(Parameters(UpdateProjectStateInput {
            project: "surrogate-model".to_owned(),
            new_state: "x".repeat(50),
        }))
        .await
        .expect_err("over-limit state is rejected");
    // The message must carry both the guidance and the numbers — assert
    // them separately so dropping either fails the test (an `||` over two
    // substrings the message always contains can't catch a half-break).
    assert!(
        err.message.to_lowercase().contains("summarise"),
        "actionable guidance: {}",
        err.message
    );
    assert!(
        err.message.contains("50") && err.message.contains("20"),
        "names the count and the limit: {}",
        err.message
    );

    // Nothing was written — the create's initial state stands.
    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(
        !body.contains(&"x".repeat(50)),
        "rejected write must not land"
    );
}

#[tokio::test]
async fn update_project_state_warn_mode_folds_the_advisory_into_the_message() {
    let mut config = VaultConfig::default();
    config.vault.max_state_chars = 20;
    config.vault.state_overflow = StateOverflow::Warn;
    let (server, _store) = server_with_config(config, |vault, store| {
        seed_today_daily(&store);
        vault
            .create_project(
                moment(2026, 5, 1, 9, 0),
                "Surrogate model",
                Context::Work,
                None,
            )
            .unwrap();
    });

    let result = server
        .update_project_state(Parameters(UpdateProjectStateInput {
            project: "surrogate-model".to_owned(),
            new_state: "y".repeat(50),
        }))
        .await
        .expect("warn mode still writes");
    let value = decode_json(&result);
    let message = value["message"].as_str().unwrap();
    // The success line stays, with the advisory appended so the calling
    // agent sees it and can self-correct on the next write.
    assert!(
        message.contains("Updated state on"),
        "keeps the success line: {message}"
    );
    assert!(
        message.contains("50") && message.contains("20"),
        "folds in the length advisory: {message}"
    );
}

// ---------------------------------------------------------------------
// add_action / promote_action / complete_action
// ---------------------------------------------------------------------

fn seed_active_project(vault: &Vault) {
    vault
        .create_project(
            moment(2026, 5, 1, 9, 0),
            "Surrogate model",
            Context::Work,
            None,
        )
        .unwrap();
}

#[tokio::test]
async fn add_action_bullet_appends_to_next_actions() {
    let (server, store) = server_with(|vault, store| {
        seed_today_daily(&store);
        seed_active_project(vault);
    });

    server
        .add_action(Parameters(AddActionInput {
            slug: None,
            project: "surrogate-model".to_owned(),
            title: "Run sweep B".to_owned(),
            energy: "deep".to_owned(),
            with_note: false,
            vars: None,
        }))
        .await
        .expect("add_action");
    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(body.contains("- [ ] Run sweep B (deep)"), "body:\n{body}");
}

#[tokio::test]
async fn add_action_with_note_creates_action_note_and_wikilinks_bullet() {
    let (server, store) = server_with(|vault, store| {
        seed_today_daily(&store);
        seed_active_project(vault);
    });

    let result = server
        .add_action(Parameters(AddActionInput {
            slug: None,
            project: "surrogate-model".to_owned(),
            title: "Investigate basis stability".to_owned(),
            energy: "deep".to_owned(),
            with_note: true,
            vars: None,
        }))
        .await
        .expect("add_action with_note");
    let value = decode_json(&result);
    // Returned path is the new action note, not the project.
    let action_path = value["path"].as_str().unwrap();
    assert!(
        action_path.starts_with("actions/") && action_path.ends_with(".md"),
        "path: {action_path}"
    );
    // Project bullet got rewritten to a wikilink.
    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(body.contains("[[actions/"), "body:\n{body}");
}

#[tokio::test]
async fn add_action_rejects_unknown_energy_with_invalid_params() {
    let (server, _store) = server_with(|vault, store| {
        seed_today_daily(&store);
        seed_active_project(vault);
    });
    let err = server
        .add_action(Parameters(AddActionInput {
            slug: None,
            project: "surrogate-model".to_owned(),
            title: "x".to_owned(),
            energy: "intense".to_owned(),
            with_note: false,
            vars: None,
        }))
        .await
        .expect_err("unknown energy should error");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("energy"));
}

#[tokio::test]
async fn promote_action_creates_action_note_from_existing_bullet() {
    let (server, store) = server_with(|vault, store| {
        seed_today_daily(&store);
        seed_active_project(vault);
        vault
            .add_action(
                moment(2026, 5, 2, 9, 0),
                "surrogate-model",
                "Run sweep B",
                EnergyLevel::Deep,
            )
            .unwrap();
    });

    let result = server
        .promote_action(Parameters(PromoteActionInput {
            slug: None,
            project: "surrogate-model".to_owned(),
            query: "sweep B".to_owned(),
            vars: None,
        }))
        .await
        .expect("promote_action");
    let value = decode_json(&result);
    let action_path = value["path"].as_str().unwrap();
    assert!(action_path.starts_with("actions/"));
    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(
        body.contains("[[actions/"),
        "bullet should now wikilink the note:\n{body}"
    );
}

#[tokio::test]
async fn complete_action_removes_bullet_and_logs() {
    let (server, store) = server_with(|vault, store| {
        seed_today_daily(&store);
        seed_active_project(vault);
        vault
            .add_action(
                moment(2026, 5, 2, 9, 0),
                "surrogate-model",
                "Run sweep B",
                EnergyLevel::Deep,
            )
            .unwrap();
    });

    server
        .complete_action(Parameters(ActionQueryInput {
            project: "surrogate-model".to_owned(),
            query: "sweep B".to_owned(),
        }))
        .await
        .expect("complete_action");
    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(
        !body.contains("Run sweep B"),
        "completed bullet should be removed:\n{body}"
    );
}

// ---------------------------------------------------------------------
// create_commitment / complete_commitment
// ---------------------------------------------------------------------

#[tokio::test]
async fn create_commitment_writes_commitment_note() {
    let (server, store) = server_with(|_v, store| seed_today_daily(&store));

    let result = server
        .create_commitment(Parameters(CreateCommitmentInput {
            slug: None,
            title: "Renew passport".to_owned(),
            due: NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
            context: "personal".to_owned(),
            project: None,
            stewardship: None,
            vars: None,
        }))
        .await
        .expect("create_commitment");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert_eq!(path, "commitments/renew-passport.md");
    let body = store.read_file(&vp(path)).unwrap();
    assert!(body.contains("due: 2026-08-01"));
    assert!(body.contains("context: personal"));
}

#[tokio::test]
async fn create_commitment_persists_stewardship_origin_link() {
    let (server, store) = server_with(|_v, store| seed_today_daily(&store));

    let result = server
        .create_commitment(Parameters(CreateCommitmentInput {
            slug: None,
            title: "Email ophthalmologist".to_owned(),
            due: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
            context: "personal".to_owned(),
            project: None,
            stewardship: Some("health".to_owned()),
            vars: None,
        }))
        .await
        .expect("create_commitment");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    let body = store.read_file(&vp(path)).unwrap();
    assert!(
        body.contains("stewardship: \"health\""),
        "frontmatter:\n{body}"
    );
    assert!(body.contains("project: null"), "frontmatter:\n{body}");
}

#[tokio::test]
async fn create_commitment_rejects_unknown_context_with_invalid_params() {
    let (server, _store) = server_with(|_v, store| seed_today_daily(&store));
    let err = server
        .create_commitment(Parameters(CreateCommitmentInput {
            slug: None,
            title: "x".to_owned(),
            due: NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
            context: "fortnightly".to_owned(),
            project: None,
            stewardship: None,
            vars: None,
        }))
        .await
        .expect_err("unknown context should error");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("context"));
}

#[tokio::test]
async fn complete_commitment_moves_to_done_folder() {
    let (server, store) = server_with(|vault, store| {
        seed_today_daily(&store);
        vault
            .create_commitment(
                moment(2026, 5, 1, 9, 0),
                "Renew passport",
                NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
                Context::Personal,
                None,
                None,
            )
            .unwrap();
    });

    let result = server
        .complete_commitment(Parameters(CompleteCommitmentInput {
            commitment: "renew-passport".to_owned(),
        }))
        .await
        .expect("complete_commitment");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert!(path.starts_with("commitments/_done/"), "path: {path}");
    // Active file removed; done file present.
    assert!(!store.exists(&vp("commitments/renew-passport.md")).unwrap());
    assert!(store.exists(&vp(path)).unwrap());
}

// ---------------------------------------------------------------------
// create_tracking_entry
// ---------------------------------------------------------------------

#[tokio::test]
async fn create_tracking_entry_writes_under_expanded_stewardship() {
    let (server, store) = server_with(|vault, _s| {
        vault
            .create_stewardship_expanded(moment(2026, 1, 10, 9, 0), "Health", Context::Personal)
            .unwrap();
    });

    let result = server
        .create_tracking_entry(Parameters(CreateTrackingEntryInput {
            metrics: None,
            date: None,
            stewardship: "health".to_owned(),
            activity: "gym".to_owned(),
            routine: Some("upper-body-a".to_owned()),
            content: "Felt strong.".to_owned(),
            vars: None,
        }))
        .await
        .expect("create_tracking_entry");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert!(
        path.starts_with("stewardships/health/tracking/") && path.ends_with("-gym.md"),
        "path: {path}"
    );
    let body = store.read_file(&vp(path)).unwrap();
    // Generic tracking template (no variant ships built-in): `routine` has no
    // field to land in and no-ops; the Notes body carries the content.
    assert!(body.contains("Felt strong."));
}

#[tokio::test]
async fn create_tracking_entry_errors_on_flat_stewardship() {
    let (server, _store) = server_with(|vault, _s| {
        vault
            .create_stewardship_flat(moment(2026, 1, 10, 9, 0), "Finances", Context::Household)
            .unwrap();
    });
    let err = server
        .create_tracking_entry(Parameters(CreateTrackingEntryInput {
            metrics: None,
            date: None,
            stewardship: "finances".to_owned(),
            activity: "gym".to_owned(),
            routine: None,
            content: String::new(),
            vars: None,
        }))
        .await
        .expect_err("flat stewardship has no tracking subdir");
    assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
    let msg = err.message.to_lowercase();
    assert!(
        msg.contains("flat") || msg.contains("tracking"),
        "msg: {msg}"
    );
}

// ---------------------------------------------------------------------
// read_daily_note (GH #158)
// ---------------------------------------------------------------------

#[tokio::test]
async fn read_daily_note_reports_absence_for_a_fresh_vault() {
    let (server, _store) = server_with(|_v, _s| {});

    let result = server
        .read_daily_note(Parameters(ReadDailyNoteInput { date: None }))
        .await
        .expect("read_daily_note");
    let value = decode_json(&result);

    assert_eq!(value["exists"].as_bool(), Some(false));
    assert_eq!(value["markdown"].as_str(), Some(""));
    assert!(value["path"].as_str().unwrap().ends_with(".md"));
}

#[tokio::test]
async fn read_daily_note_returns_markdown_when_present() {
    let (server, _store) = server_with(|_v, store| seed_today_daily(&store));

    let result = server
        .read_daily_note(Parameters(ReadDailyNoteInput { date: None }))
        .await
        .expect("read_daily_note");
    let value = decode_json(&result);

    assert_eq!(value["exists"].as_bool(), Some(true));
    assert!(value["markdown"].as_str().unwrap().contains("## Logs"));
}

// ---------------------------------------------------------------------
// read_weekly_note / upsert_weekly_section
// ---------------------------------------------------------------------

/// A Wednesday in ISO week 2026-W18.
fn week_day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 4, 29).unwrap()
}

#[tokio::test]
async fn upsert_weekly_section_writes_a_review_section() {
    let (server, store) = server_with(|_v, _s| {});

    let result = server
        .upsert_weekly_section(Parameters(UpsertWeeklySectionInput {
            section: "This Week's Goal".to_owned(),
            content: "Draft the methods section.".to_owned(),
            date: Some(week_day()),
            append: false,
        }))
        .await
        .expect("upsert_weekly_section");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert!(path.ends_with("2026-W18.md"), "weekly path: {path}");

    let body = store.read_file(&vp(path)).unwrap();
    assert!(body.contains("week: 2026-W18"), "frontmatter:\n{body}");
    assert!(
        body.contains("## This Week's Goal\nDraft the methods section."),
        "body:\n{body}"
    );
}

#[tokio::test]
async fn upsert_weekly_section_accepts_the_deprecated_next_weeks_focus_alias() {
    // Back-compat at the real caller boundary: a pre-rename caller passing
    // the old section name must land content under the new
    // `## This Week's Goal` heading and must NOT create a stray
    // `## Next Week's Focus`. The `from_str` unit test alone doesn't prove
    // the heading routing through the MCP operation.
    let (server, store) = server_with(|_v, _s| {});

    let result = server
        .upsert_weekly_section(Parameters(UpsertWeeklySectionInput {
            section: "Next Week's Focus".to_owned(),
            content: "Book the vacation.".to_owned(),
            date: Some(week_day()),
            append: false,
        }))
        .await
        .expect("upsert via deprecated alias");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    let body = store.read_file(&vp(path)).unwrap();
    assert!(
        body.contains("## This Week's Goal\nBook the vacation."),
        "alias should route to the renamed heading:\n{body}"
    );
    assert!(
        !body.contains("## Next Week's Focus"),
        "alias must not write the old heading:\n{body}"
    );
}

#[tokio::test]
async fn upsert_weekly_section_rejects_an_unknown_section() {
    let (server, _store) = server_with(|_v, _s| {});

    let err = server
        .upsert_weekly_section(Parameters(UpsertWeeklySectionInput {
            section: "Retrospective".to_owned(),
            content: "x".to_owned(),
            date: Some(week_day()),
            append: false,
        }))
        .await
        .expect_err("unknown weekly section should be rejected");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("section"));
}

#[tokio::test]
async fn read_weekly_note_reports_absence_then_presence() {
    let (server, _store) = server_with(|_v, _s| {});

    let before = decode_json(
        &server
            .read_weekly_note(Parameters(ReadWeeklyNoteInput {
                date: Some(week_day()),
            }))
            .await
            .expect("read_weekly_note (absent)"),
    );
    assert_eq!(before["exists"].as_bool(), Some(false));

    server
        .upsert_weekly_section(Parameters(UpsertWeeklySectionInput {
            section: "Wins".to_owned(),
            content: "- Shipped it.".to_owned(),
            date: Some(week_day()),
            append: false,
        }))
        .await
        .expect("seed weekly note");

    let after = decode_json(
        &server
            .read_weekly_note(Parameters(ReadWeeklyNoteInput {
                date: Some(week_day()),
            }))
            .await
            .expect("read_weekly_note (present)"),
    );
    assert_eq!(after["exists"].as_bool(), Some(true));
    assert!(
        after["markdown"]
            .as_str()
            .unwrap()
            .contains("- Shipped it.")
    );
}

// ---------------------------------------------------------------------
// read_monthly_note / upsert_monthly_section (GH #228)
// ---------------------------------------------------------------------

/// A day in July 2026.
fn month_day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()
}

#[tokio::test]
async fn upsert_monthly_section_writes_a_review_section() {
    let (server, store) = server_with(|_v, _s| {});

    let result = server
        .upsert_monthly_section(Parameters(UpsertMonthlySectionInput {
            section: "Next Month's Focus".to_owned(),
            content: "Draft the discussion section.".to_owned(),
            date: Some(month_day()),
            append: false,
        }))
        .await
        .expect("upsert_monthly_section");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert!(path.ends_with("2026-07.md"), "monthly path: {path}");

    let body = store.read_file(&vp(path)).unwrap();
    assert!(body.contains("month: 2026-07"), "frontmatter:\n{body}");
    assert!(
        body.contains("## Next Month's Focus\nDraft the discussion section."),
        "body:\n{body}"
    );
    // The month's weeks are linked, not copied.
    assert!(
        body.contains("- [[journal/2026/weekly/2026-W28]]"),
        "weeks block:\n{body}"
    );
}

#[tokio::test]
async fn upsert_monthly_section_rejects_an_unknown_section() {
    let (server, _store) = server_with(|_v, _s| {});

    let err = server
        .upsert_monthly_section(Parameters(UpsertMonthlySectionInput {
            section: "Metrics".to_owned(),
            content: "x".to_owned(),
            date: Some(month_day()),
            append: false,
        }))
        .await
        .expect_err("unknown monthly section should be rejected");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("section"));
}

#[tokio::test]
async fn read_monthly_note_reports_absence_then_presence() {
    let (server, _store) = server_with(|_v, _s| {});

    let before = decode_json(
        &server
            .read_monthly_note(Parameters(ReadMonthlyNoteInput {
                date: Some(month_day()),
            }))
            .await
            .expect("read_monthly_note (absent)"),
    );
    assert_eq!(before["exists"].as_bool(), Some(false));

    server
        .upsert_monthly_section(Parameters(UpsertMonthlySectionInput {
            section: "Wins".to_owned(),
            content: "- Shipped it.".to_owned(),
            date: Some(month_day()),
            append: false,
        }))
        .await
        .expect("seed monthly note");

    let after = decode_json(
        &server
            .read_monthly_note(Parameters(ReadMonthlyNoteInput {
                date: Some(month_day()),
            }))
            .await
            .expect("read_monthly_note (present)"),
    );
    assert_eq!(after["exists"].as_bool(), Some(true));
    assert!(
        after["markdown"]
            .as_str()
            .unwrap()
            .contains("- Shipped it.")
    );
}

// ---------------------------------------------------------------------
// upsert_daily_section (GH #158)
// ---------------------------------------------------------------------

#[tokio::test]
async fn upsert_daily_section_writes_a_planning_section() {
    let (server, store) = server_with(|_v, _s| {});

    let result = server
        .upsert_daily_section(Parameters(UpsertDailySectionInput {
            section: "intention".to_owned(),
            content: "Ship #158".to_owned(),
            date: None,
            append: false,
        }))
        .await
        .expect("upsert_daily_section");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();

    let body = store.read_file(&vp(path)).unwrap();
    assert!(body.contains("## Intention"), "body:\n{body}");
    assert!(body.contains("Ship #158"), "body:\n{body}");
}

#[tokio::test]
async fn upsert_daily_section_rejects_history_section_with_invalid_params() {
    let (server, _store) = server_with(|_v, _s| {});

    let err = server
        .upsert_daily_section(Parameters(UpsertDailySectionInput {
            section: "Logs".to_owned(),
            content: "sneaky".to_owned(),
            date: None,
            append: false,
        }))
        .await
        .expect_err("Logs is append-only and not on the allowlist");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("section"));
}

#[tokio::test]
async fn upsert_daily_section_appends_meeting_notes() {
    let (server, store) = server_with(|_v, _s| {});

    for line in ["### NFM sync", "- decided X", "- next: Y"] {
        server
            .upsert_daily_section(Parameters(UpsertDailySectionInput {
                section: "Meeting".to_owned(),
                content: line.to_owned(),
                date: None,
                append: true,
            }))
            .await
            .expect("append meeting note");
    }
    // Read back today's daily and confirm all three accrued.
    let today = chrono::Local::now().date_naive();
    let path = vp(&cdno_core::paths::daily_note_relpath(today));
    let body = store.read_file(&path).unwrap();
    assert!(body.contains("## Meeting"), "body:\n{body}");
    assert!(
        body.contains("### NFM sync") && body.contains("- decided X") && body.contains("- next: Y")
    );
}

// ---------------------------------------------------------------------
// Structural creation (GH #162)
// ---------------------------------------------------------------------

#[tokio::test]
async fn create_project_creates_a_project_map() {
    let (server, store) = server_with(|_v, _s| {});

    let result = server
        .create_project(Parameters(CreateProjectInput {
            slug: None,
            title: "Widget Redesign".to_owned(),
            context: "work".to_owned(),
            core_question: None,
            vars: None,
        }))
        .await
        .expect("create_project");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert!(path.starts_with("projects/"), "path: {path}");

    let body = store.read_file(&vp(path)).unwrap();
    assert!(body.contains("type: project"), "body:\n{body}");
}

#[tokio::test]
async fn create_project_rejects_unknown_context_with_invalid_params() {
    let (server, _store) = server_with(|_v, _s| {});

    let err = server
        .create_project(Parameters(CreateProjectInput {
            slug: None,
            title: "X".to_owned(),
            context: "nonsense".to_owned(),
            core_question: None,
            vars: None,
        }))
        .await
        .expect_err("unknown context should error");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("context"));
}

#[tokio::test]
async fn create_project_at_the_cap_is_seeded_parked() {
    // Seed the default cap (5) of active projects. The 6th isn't
    // rejected — it's created parked, since the cap is enforced on
    // activation, not creation.
    let (server, store) = server_with(|vault, _s| {
        let today = moment(2026, 1, 1, 9, 0);
        for i in 1..=5 {
            vault
                .create_project(today, &format!("Project {i}"), Context::Work, None)
                .expect("seed project");
        }
    });

    let result = server
        .create_project(Parameters(CreateProjectInput {
            slug: None,
            title: "Sixth".to_owned(),
            context: "work".to_owned(),
            core_question: None,
            vars: None,
        }))
        .await
        .expect("sixth project is created, just parked");
    let path = decode_json(&result)["path"].as_str().unwrap().to_owned();
    assert!(
        path.starts_with("projects/_parked/"),
        "at the cap the new project is parked, got {path}"
    );
    assert!(
        store
            .read_file(&vp(&path))
            .unwrap()
            .contains("status: parked")
    );
}

#[tokio::test]
async fn create_project_with_vars_renders_a_prompted_variable() {
    // A custom template with a `[variables.prompt]` placeholder resolves
    // from the handler's `vars` map — the MCP analogue of the CLI's `--var`.
    let (server, store) =
        server_with_config(config_with_prompt("ticket", "Ticket?"), |_v, store| {
            store
                .write_file(&vp(".cuaderno/templates/project.md"), PROJECT_WITH_TICKET)
                .unwrap();
        });

    let mut vars = std::collections::HashMap::new();
    vars.insert("ticket".to_owned(), "ABC-1".to_owned());

    let result = server
        .create_project(Parameters(CreateProjectInput {
            slug: None,
            title: "Widget".to_owned(),
            context: "work".to_owned(),
            core_question: None,
            vars: Some(vars),
        }))
        .await
        .expect("create_project with vars");
    let path = decode_json(&result)["path"].as_str().unwrap().to_owned();
    let body = store.read_file(&vp(&path)).unwrap();
    assert!(body.contains("ticket: ABC-1"), "body:\n{body}");
    assert!(
        !body.contains("{{ticket}}"),
        "placeholder unresolved:\n{body}"
    );
}

#[tokio::test]
async fn create_project_without_vars_surfaces_unresolved_prompts() {
    // Omitting a required prompted variable must fail with a clear error
    // (the domain's `UnresolvedPrompts`, mapped to INTERNAL_ERROR) rather
    // than leaving a literal `{{ticket}}` in the note.
    let (server, _store) =
        server_with_config(config_with_prompt("ticket", "Ticket?"), |_v, store| {
            store
                .write_file(&vp(".cuaderno/templates/project.md"), PROJECT_WITH_TICKET)
                .unwrap();
        });

    let err = server
        .create_project(Parameters(CreateProjectInput {
            slug: None,
            title: "Widget".to_owned(),
            context: "work".to_owned(),
            core_question: None,
            vars: None,
        }))
        .await
        .expect_err("missing prompted var should error");
    assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
    assert!(err.message.contains("ticket"), "message: {}", err.message);
}

#[tokio::test]
async fn create_question_creates_a_note_and_rejects_unknown_domain() {
    let (server, store) = server_with(|_v, _s| {});

    let result = server
        .create_question(Parameters(CreateQuestionInput {
            slug: None,
            domain: "research".to_owned(),
            text: "What is the best benchmark?".to_owned(),
            vars: None,
        }))
        .await
        .expect("create_question");
    let path = decode_json(&result)["path"].as_str().unwrap().to_owned();
    assert!(path.starts_with("questions/research/"), "path: {path}");
    assert!(
        store
            .read_file(&vp(&path))
            .unwrap()
            .contains("type: question")
    );

    let err = server
        .create_question(Parameters(CreateQuestionInput {
            slug: None,
            domain: "philosophy".to_owned(),
            text: "?".to_owned(),
            vars: None,
        }))
        .await
        .expect_err("unknown domain should error");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("domain"));
}

#[tokio::test]
async fn create_portfolio_creates_an_index() {
    let (server, store) = server_with(|_v, _s| {});

    let result = server
        .create_portfolio(Parameters(CreatePortfolioInput {
            slug: None,
            question: "Reference material".to_owned(),
            project: None,
            vars: None,
        }))
        .await
        .expect("create_portfolio");
    let path = decode_json(&result)["path"].as_str().unwrap().to_owned();
    assert!(path.starts_with("portfolios/"), "path: {path}");
    assert!(
        store
            .read_file(&vp(&path))
            .unwrap()
            .contains("type: portfolio")
    );
}

#[tokio::test]
async fn create_stewardship_honours_the_expanded_flag() {
    let (server, store) = server_with(|_v, _s| {});

    let flat = server
        .create_stewardship(Parameters(CreateStewardshipInput {
            slug: None,
            name: "Finances".to_owned(),
            context: "personal".to_owned(),
            expanded: false,
            vars: None,
        }))
        .await
        .expect("flat stewardship");
    let flat_path = decode_json(&flat)["path"].as_str().unwrap().to_owned();
    assert!(flat_path.ends_with("finances.md"), "flat path: {flat_path}");

    let expanded = server
        .create_stewardship(Parameters(CreateStewardshipInput {
            slug: None,
            name: "Health".to_owned(),
            context: "personal".to_owned(),
            expanded: true,
            vars: None,
        }))
        .await
        .expect("expanded stewardship");
    let exp_path = decode_json(&expanded)["path"].as_str().unwrap().to_owned();
    assert!(
        exp_path.ends_with("health/_index.md"),
        "expanded path: {exp_path}"
    );
    assert!(
        store
            .read_file(&vp(&exp_path))
            .unwrap()
            .contains("type: stewardship")
    );
}

// ---------------------------------------------------------------------
// Lifecycle (GH #166)
// ---------------------------------------------------------------------

#[tokio::test]
async fn park_project_moves_it_to_parked() {
    let (server, store) = server_with(|vault, _s| {
        vault
            .create_project(moment(2026, 1, 1, 9, 0), "Widget", Context::Work, None)
            .unwrap();
    });

    let result = server
        .park_project(Parameters(ProjectSlugInput {
            project: "widget".to_owned(),
        }))
        .await
        .expect("park_project");
    let path = decode_json(&result)["path"].as_str().unwrap().to_owned();
    assert!(path.starts_with("projects/_parked/"), "path: {path}");
    assert!(
        store
            .read_file(&vp(&path))
            .unwrap()
            .contains("status: parked")
    );
}

#[tokio::test]
async fn activate_project_brings_it_back() {
    let (server, store) = server_with(|vault, _s| {
        let today = moment(2026, 1, 1, 9, 0);
        vault
            .create_project(today, "Widget", Context::Work, None)
            .unwrap();
        vault
            .park_project(moment(2026, 1, 1, 9, 0), "widget")
            .unwrap();
    });

    let result = server
        .activate_project(Parameters(ProjectSlugInput {
            project: "widget".to_owned(),
        }))
        .await
        .expect("activate_project");
    let path = decode_json(&result)["path"].as_str().unwrap().to_owned();
    assert!(path.starts_with("projects/widget"), "path: {path}");
    assert!(
        store
            .read_file(&vp(&path))
            .unwrap()
            .contains("status: active")
    );
}

#[tokio::test]
async fn activate_project_at_the_cap_errors() {
    // 5 active (the cap) + 1 auto-parked; activating the parked one fails.
    let (server, _store) = server_with(|vault, _s| {
        let today = moment(2026, 1, 1, 9, 0);
        for i in 1..=5 {
            vault
                .create_project(today, &format!("P{i}"), Context::Work, None)
                .unwrap();
        }
        vault
            .create_project(today, "Parked One", Context::Work, None)
            .unwrap();
    });

    let err = server
        .activate_project(Parameters(ProjectSlugInput {
            project: "parked-one".to_owned(),
        }))
        .await
        .expect_err("activating past the cap should error");
    assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
    assert!(err.message.contains("cap"), "msg: {}", err.message);
}

#[tokio::test]
async fn set_question_status_updates_and_rejects_unknown() {
    let (server, store) = server_with(|vault, _s| {
        vault
            .create_question(
                moment(2026, 1, 1, 9, 0),
                QuestionDomain::Research,
                "is it fast",
            )
            .unwrap();
    });

    let result = server
        .set_question_status(Parameters(SetQuestionStatusInput {
            question: "is-it-fast".to_owned(),
            status: "answered".to_owned(),
        }))
        .await
        .expect("set_question_status");
    let path = decode_json(&result)["path"].as_str().unwrap().to_owned();
    assert!(
        store
            .read_file(&vp(&path))
            .unwrap()
            .contains("status: answered")
    );

    let err = server
        .set_question_status(Parameters(SetQuestionStatusInput {
            question: "is-it-fast".to_owned(),
            status: "ponder".to_owned(),
        }))
        .await
        .expect_err("unknown status should error");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("status"));
}

#[tokio::test]
async fn add_periodic_commitment_appends_and_rejects_unknown_recurrence() {
    let (server, store) = server_with(|vault, _s| {
        vault
            .create_stewardship_expanded(moment(2026, 1, 1, 9, 0), "Gym", Context::Personal)
            .unwrap();
    });

    let result = server
        .add_periodic_commitment(Parameters(AddPeriodicCommitmentInput {
            stewardship: "gym".to_owned(),
            title: "Pay membership".to_owned(),
            recurrence: "monthly".to_owned(),
            next_date: NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
        }))
        .await
        .expect("add_periodic_commitment");
    let path = decode_json(&result)["path"].as_str().unwrap().to_owned();
    assert!(
        store
            .read_file(&vp(&path))
            .unwrap()
            .contains("Pay membership")
    );

    let err = server
        .add_periodic_commitment(Parameters(AddPeriodicCommitmentInput {
            stewardship: "gym".to_owned(),
            title: "x".to_owned(),
            recurrence: "fortnightly".to_owned(),
            next_date: NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
        }))
        .await
        .expect_err("unknown recurrence should error");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("recurrence"));
}

#[tokio::test]
async fn file_to_portfolio_attaches_a_non_markdown_artefact() {
    let dir = tempfile::tempdir().unwrap();
    let artefact = dir.path().join("figure.png");
    std::fs::write(&artefact, b"\x89PNG fake").unwrap();

    let (server, store) = server_with(|vault, _s| {
        vault
            .create_portfolio(moment(2026, 2, 1, 9, 0), "Does sparse beat dense?", None)
            .unwrap();
    });

    let result = server
        .file_to_portfolio(Parameters(FileToPortfolioInput {
            portfolio: "does-sparse-beat-dense".to_owned(),
            source: "Whiteboard".to_owned(),
            origin: "projects/surrogate".to_owned(),
            content: "Sketch of the attention sparsity pattern.".to_owned(),
            attach: Some(artefact.to_string_lossy().into_owned()),
            vars: None,
            format: None,
        }))
        .await
        .expect("attach via MCP");

    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert!(path.ends_with("-whiteboard.md"), "stub path: {path}");
    let body = store.read_file(&vp(path)).unwrap();
    assert!(body.contains("kind: image"), "{body}");
    assert!(body.contains("Sketch of the attention"), "{body}");
    // The artefact landed in the stub's sibling folder, filename preserved.
    let stem = path.strip_suffix(".md").unwrap();
    assert!(
        store.exists(&vp(&format!("{stem}/figure.png"))).unwrap(),
        "artefact imported beside the stub"
    );
}

// ---------------------------------------------------------------------
// milestone + waiting-on MCP parity (#213)
// ---------------------------------------------------------------------

fn server_with_project() -> (CuadernoServer, Arc<dyn VaultStore>) {
    server_with(|vault, _s| {
        vault
            .create_project(
                moment(2026, 5, 1, 9, 0),
                "Surrogate model",
                Context::Work,
                None,
            )
            .unwrap();
    })
}

#[tokio::test]
async fn drop_action_closes_the_bullet_without_claiming_it_was_done() {
    let (server, store) = server_with_project();
    server
        .add_action(Parameters(AddActionInput {
            slug: None,
            project: "surrogate-model".to_owned(),
            title: "Prepare the demo proposal".to_owned(),
            energy: "deep".to_owned(),
            with_note: false,
            vars: None,
        }))
        .await
        .expect("add_action");

    server
        .drop_action(Parameters(DropActionInput {
            project: "surrogate-model".to_owned(),
            query: "demo proposal".to_owned(),
            reason: Some("superseded by the demo-planning action".to_owned()),
        }))
        .await
        .expect("drop_action");

    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(!body.contains("Prepare the demo proposal"), "{body}");

    // The handler stamps the real clock, so build the path the same way
    // rather than hardcoding a date that would never match and leave the
    // assertion silently unreached.
    let today = chrono::Local::now().naive_local().date();
    let daily = vp(&format!(
        "journal/{}/daily/{}.md",
        today.format("%Y"),
        today.format("%Y-%m-%d")
    ));
    let daily = store.read_file(&daily).expect("the drop wrote a daily log");
    assert!(
        daily.contains("action dropped on [[surrogate-model]]"),
        "drop entry missing:\n{daily}"
    );
    assert!(
        daily.contains("reason: superseded by the demo-planning action"),
        "reason missing:\n{daily}"
    );
    assert!(
        !daily.contains("action done on"),
        "a drop must never be logged as a completion:\n{daily}"
    );
}

#[tokio::test]
async fn add_milestone_without_a_target_date_records_tbd() {
    let (server, store) = server_with_project();

    server
        .add_milestone(Parameters(AddMilestoneInput {
            project: "surrogate-model".to_owned(),
            title: "All Round-1 replies received".to_owned(),
            target_date: None,
            hard: false,
        }))
        .await
        .expect("add_milestone without a date");

    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(
        body.contains("- [ ] All Round-1 replies received \u{2014} target: TBD"),
        "{body}"
    );
}

#[tokio::test]
async fn add_milestone_rejects_hard_without_a_target_date() {
    let (server, _store) = server_with_project();

    let err = server
        .add_milestone(Parameters(AddMilestoneInput {
            project: "surrogate-model".to_owned(),
            title: "Ship v1".to_owned(),
            target_date: None,
            hard: true,
        }))
        .await
        .expect_err("a hard deadline with no date is rejected");
    assert!(
        err.message.contains("hard"),
        "the client is told which half is wrong: {}",
        err.message
    );
}

#[tokio::test]
async fn set_core_question_wraps_a_bare_target() {
    let (server, store) = server_with_project();

    server
        .set_core_question(Parameters(SetCoreQuestionInput {
            project: "surrogate-model".to_owned(),
            core_question: Some("questions/research/foo".to_owned()),
            clear: false,
        }))
        .await
        .expect("set_core_question");

    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(
        body.contains("core_question: \"[[questions/research/foo]]\""),
        "{body}"
    );
}

#[tokio::test]
async fn set_core_question_rejects_an_already_wrapped_target() {
    let (server, _store) = server_with_project();

    let err = server
        .set_core_question(Parameters(SetCoreQuestionInput {
            project: "surrogate-model".to_owned(),
            core_question: Some("[[questions/research/foo]]".to_owned()),
            clear: false,
        }))
        .await
        .expect_err("the wrapped form is refused");
    assert!(
        err.message.contains("bare path"),
        "the message teaches the convention: {}",
        err.message
    );
}

/// The omitted-field slip is the likeliest one an agent makes, and it
/// must not be the one that silently unlinks a project from its
/// question. The CLI refuses it; so does the tool.
#[tokio::test]
async fn set_core_question_refuses_to_detach_by_omission() {
    let (server, _store) = server_with_project();

    let err = server
        .set_core_question(Parameters(SetCoreQuestionInput {
            project: "surrogate-model".to_owned(),
            core_question: None,
            clear: false,
        }))
        .await
        .expect_err("neither field set is an error, not a detach");
    assert!(
        err.message.contains("clear"),
        "the message names the way to actually detach: {}",
        err.message
    );
}

#[tokio::test]
async fn set_core_question_refuses_a_target_and_clear_together() {
    let (server, _store) = server_with_project();

    let err = server
        .set_core_question(Parameters(SetCoreQuestionInput {
            project: "surrogate-model".to_owned(),
            core_question: Some("questions/research/foo".to_owned()),
            clear: true,
        }))
        .await
        .expect_err("the two are mutually exclusive");
    assert!(err.message.contains("not both"), "{}", err.message);
}

#[tokio::test]
async fn set_core_question_with_clear_detaches_the_question() {
    let (server, store) = server_with_project();
    server
        .set_core_question(Parameters(SetCoreQuestionInput {
            project: "surrogate-model".to_owned(),
            core_question: Some("questions/research/foo".to_owned()),
            clear: false,
        }))
        .await
        .expect("set");

    server
        .set_core_question(Parameters(SetCoreQuestionInput {
            project: "surrogate-model".to_owned(),
            core_question: None,
            clear: true,
        }))
        .await
        .expect("detach");

    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(body.contains("core_question: null"), "{body}");
}

#[tokio::test]
async fn add_milestone_appends_a_hard_deadline() {
    let (server, store) = server_with_project();

    let result = server
        .add_milestone(Parameters(AddMilestoneInput {
            project: "surrogate-model".to_owned(),
            title: "Ship v1".to_owned(),
            target_date: Some(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()),
            hard: true,
        }))
        .await
        .expect("add_milestone");
    let value = decode_json(&result);
    assert_eq!(
        value["path"].as_str().unwrap(),
        "projects/surrogate-model.md"
    );
    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    // Pin the full open-bullet form, not just the loose substrings.
    assert!(body.contains("- [ ] Ship v1"), "{body}");
    assert!(body.contains("hard: 2026-07-01"), "{body}");
}

#[tokio::test]
async fn complete_milestone_ticks_the_bullet() {
    let (server, store) = server_with_project();
    server
        .add_milestone(Parameters(AddMilestoneInput {
            project: "surrogate-model".to_owned(),
            title: "Ship v1".to_owned(),
            target_date: Some(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()),
            hard: false,
        }))
        .await
        .expect("add_milestone");

    server
        .complete_milestone(Parameters(CompleteMilestoneInput {
            project: "surrogate-model".to_owned(),
            query: "ship".to_owned(),
        }))
        .await
        .expect("complete_milestone");

    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(body.contains("- [x] Ship v1"), "{body}");
}

#[tokio::test]
async fn add_then_resolve_waiting_on_round_trips() {
    let (server, store) = server_with_project();

    server
        .add_waiting_on(Parameters(AddWaitingOnInput {
            project: "surrogate-model".to_owned(),
            description: "Compute allocation".to_owned(),
        }))
        .await
        .expect("add_waiting_on");
    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(body.contains("- Compute allocation"), "{body}");

    server
        .resolve_waiting_on(Parameters(ResolveWaitingOnInput {
            project: "surrogate-model".to_owned(),
            query: "compute".to_owned(),
        }))
        .await
        .expect("resolve_waiting_on");
    let body = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(!body.contains("- Compute allocation"), "{body}");
    // Removing the last item restores the placeholder.
    assert!(body.contains("(nothing yet)"), "{body}");
}

#[tokio::test]
async fn add_milestone_errors_on_unknown_project() {
    let (server, _store) = server_with(|_v, _s| ());
    let err = server
        .add_milestone(Parameters(AddMilestoneInput {
            project: "ghost".to_owned(),
            title: "X".to_owned(),
            target_date: Some(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()),
            hard: false,
        }))
        .await
        .expect_err("unknown project should error");
    assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
}

// ---------------------------------------------------------------------
// capture (#204)
// ---------------------------------------------------------------------

#[tokio::test]
async fn capture_writes_a_note_under_inbox() {
    let (server, store) = server_with(|_v, _s| ());

    let result = server
        .capture(Parameters(CaptureInput {
            text: "buy more index cards".to_owned(),
        }))
        .await
        .expect("capture");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert!(path.starts_with("inbox/"), "path: {path}");

    let body = store.read_file(&vp(path)).unwrap();
    assert!(body.contains("buy more index cards"), "body:\n{body}");
}

#[tokio::test]
async fn discard_inbox_item_removes_the_capture() {
    let (server, store) = server_with(|vault, _s| {
        vault
            .capture_to_inbox(moment(2026, 4, 26, 9, 0), "ephemeral")
            .unwrap();
    });
    assert!(store.exists(&vp("inbox/2026-04-26-ephemeral.md")).unwrap());

    server
        .discard_inbox_item(Parameters(DiscardInboxItemInput {
            slug: "2026-04-26-ephemeral".to_owned(),
        }))
        .await
        .expect("discard_inbox_item");

    assert!(
        !store.exists(&vp("inbox/2026-04-26-ephemeral.md")).unwrap(),
        "the inbox note is deleted"
    );
}

// ---------------------------------------------------------------------
// create_custom_note (config-defined custom types)
// ---------------------------------------------------------------------

fn config_with_person() -> VaultConfig {
    use cdno_core::config::CustomNoteType;
    let mut config = VaultConfig::default();
    config.note_types.insert(
        "person".to_owned(),
        CustomNoteType {
            folder: "people".to_owned(),
            required: vec!["name".to_owned()],
            optional: vec!["role".to_owned()],
            template: None,
            append_only: false,
            title_field: None,
            date_field: None,
        },
    );
    config
}

#[tokio::test]
async fn create_custom_note_creates_a_custom_type_note() {
    let (server, store) = server_with_config(config_with_person(), |_v, _s| {});
    let result = server
        .create_custom_note(Parameters(CreateCustomNoteInput {
            format: None,
            slug: None,
            type_name: "person".to_owned(),
            title: "Ada Lovelace".to_owned(),
            fields: std::collections::HashMap::from([
                ("name".to_owned(), "Ada".to_owned()),
                ("role".to_owned(), "advisor".to_owned()),
            ]),
            vars: None,
            body: None,
            origin: None,
        }))
        .await
        .expect("create_custom_note");

    let payload = decode_json(&result);
    assert!(
        payload["path"]
            .as_str()
            .unwrap()
            .contains("people/ada-lovelace.md"),
        "payload: {payload}"
    );
    let content = store.read_file(&vp("people/ada-lovelace.md")).unwrap();
    assert!(content.contains("type: person"), "{content}");
    assert!(content.contains("name: Ada"), "{content}");
}

#[tokio::test]
async fn create_custom_note_rejects_an_unknown_type() {
    let (server, _store) = server_with_config(config_with_person(), |_v, _s| {});
    let result = server
        .create_custom_note(Parameters(CreateCustomNoteInput {
            format: None,
            slug: None,
            type_name: "gadget".to_owned(),
            title: "Widget".to_owned(),
            fields: std::collections::HashMap::new(),
            vars: None,
            body: None,
            origin: None,
        }))
        .await;
    // A domain error maps to an MCP error (Err), not an error-result payload.
    let err = result.expect_err("unknown type should error");
    // Not bundled, so no install command is offered.
    assert!(
        !err.message.contains("note-type install"),
        "{}",
        err.message
    );
}

#[tokio::test]
async fn create_custom_note_names_the_install_command_for_a_bundled_type() {
    // RFC 0003 §4.5: `concept` ships with cdno, so a vault without it gets
    // told the command that installs it rather than a bare "unknown type".
    let (server, _store) = server_with_config(config_with_person(), |_v, _s| {});
    let err = server
        .create_custom_note(Parameters(CreateCustomNoteInput {
            format: None,
            slug: None,
            type_name: "concept".to_owned(),
            title: "Woodbury identity".to_owned(),
            fields: std::collections::HashMap::new(),
            vars: None,
            body: None,
            origin: None,
        }))
        .await
        .expect_err("an undeclared type is refused");
    assert!(
        err.message
            .contains("run `cdno config note-type install --name concept`"),
        "{}",
        err.message
    );
    let payload = &err.data.as_ref().expect("a classified rejection")["cdno_rejection"];
    assert_eq!(payload["code"], "unknown_note_type", "{payload}");
    assert_eq!(
        payload["details"]["install_command"],
        "cdno config note-type install --name concept"
    );
}

#[tokio::test]
async fn create_custom_note_rejects_a_missing_required_field() {
    let (server, _store) = server_with_config(config_with_person(), |_v, _s| {});
    let result = server
        .create_custom_note(Parameters(CreateCustomNoteInput {
            format: None,
            slug: None,
            type_name: "person".to_owned(),
            title: "Nameless".to_owned(),
            fields: std::collections::HashMap::new(),
            vars: None,
            body: None,
            origin: None,
        }))
        .await;
    assert!(result.is_err(), "missing required field should error");
}

/// A `concept` type (RFC 0002 §6.1) declaring `origin` optional.
fn config_with_person_and_concept() -> VaultConfig {
    use cdno_core::config::CustomNoteType;
    let mut config = config_with_person();
    config.note_types.insert(
        "concept".to_owned(),
        CustomNoteType {
            folder: "concepts".to_owned(),
            required: vec!["created".to_owned()],
            optional: vec!["tags".to_owned(), "origin".to_owned()],
            template: None,
            append_only: false,
            title_field: None,
            date_field: None,
        },
    );
    config
}

#[tokio::test]
async fn create_custom_note_writes_body_and_origin() {
    let (server, store) = server_with_config(config_with_person_and_concept(), |_v, s| {
        seed_today_daily(&s)
    });
    let origin = "[[journal/2026/daily/2026-09-02#Woodbury identity]]";
    let result = server
        .create_custom_note(Parameters(CreateCustomNoteInput {
            format: None,
            slug: None,
            type_name: "concept".to_owned(),
            title: "Woodbury identity".to_owned(),
            fields: std::collections::HashMap::new(),
            vars: None,
            body: Some("The inverse of a low-rank update.".to_owned()),
            origin: Some(origin.to_owned()),
        }))
        .await
        .expect("create_custom_note");
    let payload = decode_json(&result);
    assert_eq!(
        payload["path"].as_str(),
        Some("concepts/woodbury-identity.md"),
        "payload: {payload}"
    );

    let content = store
        .read_file(&vp("concepts/woodbury-identity.md"))
        .unwrap();
    let (fm, body) = cdno_core::frontmatter::Frontmatter::parse(&content).unwrap();
    assert_eq!(
        fm.optional_field::<String>("origin").unwrap().as_deref(),
        Some(origin),
        "{content}"
    );
    assert!(
        body.ends_with("# Woodbury identity\n\nThe inverse of a low-rank update.\n"),
        "{content}"
    );
}

#[tokio::test]
async fn create_custom_note_rejects_origin_on_a_type_that_does_not_declare_it() {
    let (server, store) = server_with_config(config_with_person_and_concept(), |_v, _s| {});
    let err = server
        .create_custom_note(Parameters(CreateCustomNoteInput {
            format: None,
            slug: None,
            type_name: "person".to_owned(),
            title: "Ada".to_owned(),
            fields: std::collections::HashMap::from([("name".to_owned(), "Ada".to_owned())]),
            vars: None,
            body: None,
            origin: Some("[[journal/2026/daily/2026-09-02]]".to_owned()),
        }))
        .await
        .expect_err("person does not declare origin");
    // Marked as a caller-actionable rejection, which `call_tool` turns into
    // an `isError` tool result (#560).
    let rejection = err
        .data
        .as_ref()
        .and_then(|d| d.get("cdno_rejection"))
        .unwrap_or_else(|| panic!("not marked as a rejection: {err:?}"));
    assert_eq!(rejection["code"], "unknown_field", "{rejection}");
    assert_eq!(rejection["details"]["field"], "origin", "{rejection}");
    assert!(!store.exists(&vp("people/ada.md")).unwrap());
}

// ---------------------------------------------------------------------
// set_frontmatter (#301)
// ---------------------------------------------------------------------

/// A config declaring a settable `meds` bool on daily notes.
fn config_with_settable_meds() -> VaultConfig {
    use cdno_core::config::{FieldSpec, FieldType, SchemaExtension};
    let mut schema = SchemaExtension::default();
    schema.fields.insert(
        "meds".to_owned(),
        FieldSpec {
            ty: FieldType::Bool,
            default: None,
            required: false,
            values: None,
            list: None,
            settable: Some(true),
            log_on_change: None,
        },
    );
    let mut config = VaultConfig::default();
    config.schemas.insert("daily".to_owned(), schema);
    config
}

/// Seed today's daily note carrying `meds: false`.
fn seed_today_daily_with_meds(store: &Arc<dyn VaultStore>) {
    let today = chrono::Local::now().date_naive();
    let path = vp(&cdno_core::paths::daily_note_relpath(today));
    let body = format!(
        "---\ndate: {date}\ntype: daily\nmeds: false\n---\n\n# {date}\n\n## Logs\n",
        date = today.format("%Y-%m-%d"),
    );
    store.write_file(&path, &body).unwrap();
}

#[tokio::test]
async fn set_frontmatter_flips_a_daily_flag() {
    let (server, store) = server_with_config(config_with_settable_meds(), |_v, store| {
        seed_today_daily_with_meds(&store)
    });

    let result = server
        .set_frontmatter(Parameters(SetFrontmatterInput {
            note: "today".to_owned(),
            key: "meds".to_owned(),
            value: "true".to_owned(),
        }))
        .await
        .expect("set_frontmatter");
    let value = decode_json(&result);
    let path = value["path"].as_str().unwrap();
    assert!(path.ends_with(".md"), "path: {path}");

    let body = store.read_file(&vp(path)).unwrap();
    assert!(body.contains("meds: true"), "flag flipped:\n{body}");
}

#[tokio::test]
async fn set_frontmatter_errors_on_a_reserved_key() {
    let (server, _store) = server_with_config(config_with_settable_meds(), |_v, store| {
        seed_today_daily_with_meds(&store)
    });

    // `date` is the engine-owned period key — rejected as a domain error.
    let result = server
        .set_frontmatter(Parameters(SetFrontmatterInput {
            note: "today".to_owned(),
            key: "date".to_owned(),
            value: "2026-01-01".to_owned(),
        }))
        .await;
    assert!(result.is_err(), "a reserved/undeclared key must error");
}

#[tokio::test]
async fn create_tracking_entry_writes_structured_metrics_at_an_explicit_date() {
    // The round trip an agent depends on: a record sequence plus a past date
    // reach the note as nested frontmatter, filed on the day it describes.
    let (server, store) = server_with(|vault, _s| {
        vault
            .create_stewardship_expanded(moment(2026, 1, 10, 9, 0), "Study", Context::Personal)
            .unwrap();
    });

    let result = server
        .create_tracking_entry(Parameters(CreateTrackingEntryInput {
            metrics: Some(serde_json::json!({
                "detail": [
                    {"subject": "harmony", "minutes": 25},
                    {"subject": "sight-reading", "minutes": 15},
                ]
            })),
            date: Some(chrono::NaiveDate::from_ymd_opt(2026, 4, 6).unwrap()),
            stewardship: "study".to_owned(),
            activity: "practice".to_owned(),
            routine: None,
            content: String::new(),
            vars: None,
        }))
        .await
        .expect("create_tracking_entry");

    let path = decode_json(&result)["path"].as_str().unwrap().to_owned();
    assert_eq!(path, "stewardships/study/tracking/2026-04-06-practice.md");
    let raw = store.read_file(&vp(&path)).unwrap();
    let (fm, _body) = cdno_core::frontmatter::Frontmatter::parse(&raw).unwrap();
    let detail = fm.as_json();
    let detail = detail
        .get("detail")
        .expect("detail key")
        .as_array()
        .unwrap();
    assert_eq!(detail.len(), 2);
    assert_eq!(detail[0]["subject"], serde_json::json!("harmony"));
}

#[tokio::test]
async fn create_tracking_entry_rejects_a_non_object_metrics_payload() {
    // Each key becomes a frontmatter key, so a bare array or scalar has no key
    // to be written under — say so rather than failing deeper in the merge.
    let (server, _store) = server_with(|vault, _s| {
        vault
            .create_stewardship_expanded(moment(2026, 1, 10, 9, 0), "Health", Context::Personal)
            .unwrap();
    });

    let err = server
        .create_tracking_entry(Parameters(CreateTrackingEntryInput {
            metrics: Some(serde_json::json!([1, 2, 3])),
            date: None,
            stewardship: "health".to_owned(),
            activity: "gym".to_owned(),
            routine: None,
            content: String::new(),
            vars: None,
        }))
        .await
        .expect_err("a non-object metrics payload must be rejected");
    assert!(
        format!("{err:?}").contains("metrics"),
        "the error must name the parameter: {err:?}"
    );
}

// --- start / unplanned start (#568) ------------------------------------

#[tokio::test]
async fn start_action_logs_the_resolved_bullet_not_the_query() {
    // The query may be energy-stripped; what lands in the log must be
    // the whole bullet, because that is what `complete_action` writes
    // back and `current_focus` pairs them by exact text.
    let (server, store) = server_with_project();
    server
        .add_action(Parameters(AddActionInput {
            slug: None,
            project: "surrogate-model".to_owned(),
            title: "Prepare the demo proposal".to_owned(),
            energy: "deep".to_owned(),
            with_note: false,
            vars: None,
        }))
        .await
        .expect("add_action");

    server
        .start_action(Parameters(StartActionInput {
            project: "surrogate-model".to_owned(),
            query: "demo proposal".to_owned(),
        }))
        .await
        .expect("start_action");

    let today = chrono::Local::now().naive_local().date();
    let daily = vp(&format!(
        "journal/{}/daily/{}.md",
        today.format("%Y"),
        today.format("%Y-%m-%d")
    ));
    let body = store.read_file(&daily).expect("daily written");
    assert!(
        body.contains("started [[surrogate-model]] \u{2014} Prepare the demo proposal (deep)"),
        "resolved bullet, energy and all:\n{body}"
    );
}

#[tokio::test]
async fn start_action_refuses_work_that_is_not_on_the_map() {
    // It will not create the bullet — that is `start_unplanned_action`.
    // A fallback here would turn a typo into a new action silently.
    let (server, store) = server_with_project();
    let err = server
        .start_action(Parameters(StartActionInput {
            project: "surrogate-model".to_owned(),
            query: "Buy milk".to_owned(),
        }))
        .await
        .expect_err("a start naming nothing must error");
    assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);

    let map = store
        .read_file(&vp("projects/surrogate-model.md"))
        .expect("map readable");
    assert!(!map.contains("Buy milk"), "and nothing was written:\n{map}");
}

#[tokio::test]
async fn start_unplanned_action_adds_the_bullet_and_starts_it() {
    let (server, store) = server_with_project();
    server
        .start_unplanned_action(Parameters(StartUnplannedActionInput {
            project: "surrogate-model".to_owned(),
            title: "Fix the CI badge".to_owned(),
            energy: "light".to_owned(),
        }))
        .await
        .expect("start_unplanned_action");

    let map = store
        .read_file(&vp("projects/surrogate-model.md"))
        .expect("map readable");
    assert!(
        map.contains("- [ ] Fix the CI badge (light)"),
        "bullet on the map:\n{map}"
    );

    let today = chrono::Local::now().naive_local().date();
    let daily = vp(&format!(
        "journal/{}/daily/{}.md",
        today.format("%Y"),
        today.format("%Y-%m-%d")
    ));
    let body = store.read_file(&daily).expect("daily written");
    assert!(
        body.contains("action added to [[surrogate-model]] \u{2014} Fix the CI badge (light)"),
        "origin logged:\n{body}"
    );
    assert!(
        body.contains("started [[surrogate-model]] \u{2014} Fix the CI badge (light)"),
        "start logged:\n{body}"
    );
}

#[tokio::test]
async fn start_unplanned_action_rejects_an_unknown_energy() {
    let (server, store) = server_with_project();
    let err = server
        .start_unplanned_action(Parameters(StartUnplannedActionInput {
            project: "surrogate-model".to_owned(),
            title: "Fix the CI badge".to_owned(),
            energy: "frantic".to_owned(),
        }))
        .await
        .expect_err("unknown energy");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);

    let map = store
        .read_file(&vp("projects/surrogate-model.md"))
        .expect("map readable");
    assert!(
        !map.contains("Fix the CI badge"),
        "and nothing was written:\n{map}"
    );
}

/// An unquoted `origin: {{origin}}` in the type's template, a two-link
/// origin, and a body that repeats the title heading: the origin still
/// round-trips as one string, and the note keeps a single H1.
#[tokio::test]
async fn create_custom_note_reconciles_origin_and_drops_a_repeated_title_heading() {
    let (server, store) = server_with_config(config_with_person_and_concept(), |_v, s| {
        seed_today_daily(&s);
        s.write_file(
            &vp(".cuaderno/templates/concept.md"),
            "---\ntype: concept\ncreated: {{created}}\norigin: {{origin}}\n---\n\n# {{title}}\n\n{{body}}\n",
        )
        .unwrap();
    });
    let origin = "[[journal/2026/daily/2026-09-02#Woodbury identity]] [[journal/2026/daily/2026-09-24#Low-rank refit]]";
    server
        .create_custom_note(Parameters(CreateCustomNoteInput {
            format: None,
            slug: None,
            type_name: "concept".to_owned(),
            title: "Woodbury identity".to_owned(),
            fields: std::collections::HashMap::new(),
            vars: None,
            body: Some("# Woodbury identity\n\nThe inverse of a low-rank update.".to_owned()),
            origin: Some(origin.to_owned()),
        }))
        .await
        .expect("create_custom_note");

    let content = store
        .read_file(&vp("concepts/woodbury-identity.md"))
        .unwrap();
    let (fm, body) = cdno_core::frontmatter::Frontmatter::parse(&content).unwrap();
    assert_eq!(
        fm.optional_field::<String>("origin").unwrap().as_deref(),
        Some(origin),
        "{content}"
    );
    assert_eq!(
        body, "\n# Woodbury identity\n\nThe inverse of a low-rank update.\n",
        "{content}"
    );
}

// ---------------------------------------------------------------------
// revise_note (RFC 0002 T9, #622)
// ---------------------------------------------------------------------

const REVISE_PATH: &str = "concepts/woodbury-identity.md";
const REVISE_NOTE: &str = "---\ntype: concept\ncreated: 2026-09-01\n---\n\n# Woodbury identity\n\nInverting a low-rank update.\n\n## Statement\n\n(A + UCV)^-1 = ...\n";

/// A concept vault holding [`REVISE_NOTE`] and a project, both indexed.
fn revise_server() -> (CuadernoServer, Arc<dyn VaultStore>) {
    server_with_config(config_with_person_and_concept(), |v, s| {
        s.write_file(&vp(REVISE_PATH), REVISE_NOTE).unwrap();
        s.write_file(
            &vp("projects/demo.md"),
            "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Demo\n",
        )
        .unwrap();
        v.reconcile().unwrap();
    })
}

fn revise_input(note: &str) -> ReviseNoteInput {
    ReviseNoteInput {
        note: note.to_owned(),
        expected_hash: None,
        body: None,
        section: None,
        content: None,
        reason: "tightened the statement".to_owned(),
    }
}

async fn revise(
    server: &CuadernoServer,
    input: ReviseNoteInput,
) -> Result<CallToolResult, rmcp::ErrorData> {
    server.revise_note(Parameters(input)).await
}

/// The `content_hash` a fresh `read_note` reports for `note`.
async fn read_hash(server: &CuadernoServer, note: &str) -> String {
    let json = decode_json(
        &server
            .read_note(Parameters(ReadNoteInput {
                note: note.to_owned(),
            }))
            .await
            .expect("read_note"),
    );
    json["content_hash"].as_str().expect("hash").to_owned()
}

/// Today's daily note, or empty when none has been written.
fn today_daily(store: &Arc<dyn VaultStore>) -> String {
    let today = chrono::Local::now().date_naive();
    store
        .read_file(&vp(&cdno_core::paths::daily_note_relpath(today)))
        .unwrap_or_default()
}

fn rejection_code(err: &rmcp::ErrorData) -> String {
    err.data
        .as_ref()
        .and_then(|d| d.get("cdno_rejection"))
        .and_then(|r| r["code"].as_str())
        .unwrap_or_else(|| panic!("not marked as a rejection: {err:?}"))
        .to_owned()
}

/// Assert an `INVALID_PARAMS` naming `field`, and that nothing was written.
async fn assert_invalid(input: ReviseNoteInput, field: &str) {
    let (server, store) = revise_server();
    let err = revise(&server, input).await.expect_err("refused");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS, "{err:?}");
    assert!(err.message.contains(&format!("'{field}'")), "{err:?}");
    assert_eq!(store.read_file(&vp(REVISE_PATH)).unwrap(), REVISE_NOTE);
    assert!(today_daily(&store).is_empty());
}

#[tokio::test]
async fn revise_note_refuses_a_body_revision_without_a_hash() {
    let mut input = revise_input(REVISE_PATH);
    input.body = Some("\nNew body.\n".to_owned());
    assert_invalid(input, "expected_hash").await;
}

#[tokio::test]
async fn revise_note_refuses_both_body_and_section() {
    let mut input = revise_input(REVISE_PATH);
    input.expected_hash = Some("any".to_owned());
    input.body = Some("\nNew body.\n".to_owned());
    input.section = Some("Statement".to_owned());
    input.content = Some("x".to_owned());
    assert_invalid(input, "body").await;
}

#[tokio::test]
async fn revise_note_refuses_neither_body_nor_section() {
    assert_invalid(revise_input(REVISE_PATH), "body").await;
}

#[tokio::test]
async fn revise_note_refuses_a_section_without_content_and_content_without_a_section() {
    let mut input = revise_input(REVISE_PATH);
    input.section = Some("Statement".to_owned());
    assert_invalid(input, "content").await;

    let mut input = revise_input(REVISE_PATH);
    input.content = Some("x".to_owned());
    assert_invalid(input, "content").await;
}

#[tokio::test]
async fn revise_note_refuses_a_blank_reason_in_its_own_words() {
    let mut input = revise_input(REVISE_PATH);
    input.section = Some("Statement".to_owned());
    input.content = Some("x".to_owned());
    input.reason = "  \n".to_owned();
    assert_invalid(input, "reason").await;
}

#[tokio::test]
async fn revise_note_refuses_a_stale_hash_and_leaves_the_note_untouched() {
    let (server, store) = revise_server();
    let mut input = revise_input(REVISE_PATH);
    input.expected_hash = Some("not-the-hash".to_owned());
    input.body = Some("\nNew body.\n".to_owned());

    let err = revise(&server, input).await.expect_err("stale");
    assert_eq!(rejection_code(&err), "stale_revision");
    assert_eq!(store.read_file(&vp(REVISE_PATH)).unwrap(), REVISE_NOTE);
    assert!(!today_daily(&store).contains("revised"));
}

#[tokio::test]
async fn revise_note_rewrites_the_body_with_the_read_hash_and_logs_the_reason() {
    let (server, store) = revise_server();
    let hash = read_hash(&server, "concept:woodbury-identity").await;
    let mut input = revise_input("concept:woodbury-identity");
    input.expected_hash = Some(hash.clone());
    input.body = Some("\n# Woodbury identity\n\nA sharper account.\n".to_owned());

    let json = decode_json(&revise(&server, input).await.expect("revised"));
    assert_eq!(json["path"], REVISE_PATH);
    assert_eq!(json["changed"], true);
    assert!(json["section_target"].is_null(), "{json}");
    assert!(json["verification"]["content_hash"].is_string(), "{json}");
    let new_hash = json["new_hash"].as_str().expect("new_hash");
    assert_ne!(new_hash, hash);
    assert_eq!(new_hash, read_hash(&server, REVISE_PATH).await);

    assert!(
        store
            .read_file(&vp(REVISE_PATH))
            .unwrap()
            .ends_with("---\n\n# Woodbury identity\n\nA sharper account.\n")
    );
    let daily = today_daily(&store);
    assert!(
        daily.contains("revised [[concepts/woodbury-identity]] \u{2014} tightened the statement"),
        "{daily}"
    );
}

#[tokio::test]
async fn revise_note_upserts_a_section_without_a_hash_and_anchors_the_log_line() {
    let (server, store) = revise_server();
    let mut input = revise_input("concepts/woodbury-identity");
    input.section = Some("Statement".to_owned());
    input.content = Some("(A + UCV)^-1 = A^-1 - ...".to_owned());

    let json = decode_json(&revise(&server, input).await.expect("revised"));
    assert_eq!(json["changed"], true);
    assert_eq!(
        json["section_target"],
        "concepts/woodbury-identity#Statement"
    );
    assert!(
        store
            .read_file(&vp(REVISE_PATH))
            .unwrap()
            .contains("## Statement\n\n(A + UCV)^-1 = A^-1 - ...\n")
    );
    let daily = today_daily(&store);
    assert!(
        daily.contains(
            "revised [[concepts/woodbury-identity#Statement]] \u{2014} tightened the statement"
        ),
        "{daily}"
    );
}

#[tokio::test]
async fn revise_note_with_identical_text_reports_no_change_and_logs_nothing() {
    let (server, store) = revise_server();
    let mut input = revise_input(REVISE_PATH);
    input.section = Some("Statement".to_owned());
    input.content = Some("(A + UCV)^-1 = ...".to_owned());

    let json = decode_json(&revise(&server, input).await.expect("no-op"));
    assert_eq!(json["changed"], false, "{json}");
    assert!(json["verification"].is_null(), "no write, no verification");
    assert!(json["log_line"].is_null());
    assert_eq!(json["new_hash"], read_hash(&server, REVISE_PATH).await);
    assert_eq!(store.read_file(&vp(REVISE_PATH)).unwrap(), REVISE_NOTE);
    assert!(!today_daily(&store).contains("revised"));
}

#[tokio::test]
async fn revise_note_replaces_a_typst_body_and_refuses_a_section() {
    let path = "concepts/bias-variance-split.typ";
    let block = "#metadata((type: \"concept\", title: \"Split\", created: \"2026-09-01\")) <cdno>";
    let original = format!("{block}\n\n= Old\n");
    let (server, store) = server_with_config(config_with_person_and_concept(), |v, s| {
        s.write_file(&vp(path), &original).unwrap();
        v.reconcile().unwrap();
    });

    let err = revise(
        &server,
        ReviseNoteInput {
            section: Some("Old".to_owned()),
            content: Some("New.".to_owned()),
            ..revise_input(path)
        },
    )
    .await
    .expect_err("refused");
    assert_eq!(
        rejection_code(&err),
        "section_revision_unavailable_for_typst"
    );
    assert_eq!(store.read_file(&vp(path)).unwrap(), original);

    let hash = read_hash(&server, path).await;
    let json = decode_json(
        &revise(
            &server,
            ReviseNoteInput {
                body: Some("= New\n".to_owned()),
                expected_hash: Some(hash),
                ..revise_input(path)
            },
        )
        .await
        .expect("revise"),
    );
    assert_eq!(json["changed"], true);
    assert_eq!(
        store.read_file(&vp(path)).unwrap(),
        format!("{block}\n\n= New\n")
    );
}

#[tokio::test]
async fn revise_note_refuses_a_built_in_type() {
    let (server, store) = revise_server();
    let mut input = revise_input("projects/demo.md");
    input.section = Some("Notes".to_owned());
    input.content = Some("x".to_owned());

    let err = revise(&server, input).await.expect_err("a project");
    assert_eq!(rejection_code(&err), "note_not_revisable");
    assert!(!today_daily(&store).contains("revised"));
}

#[tokio::test]
async fn revise_note_refuses_an_unknown_reference_as_not_found() {
    let (server, _store) = revise_server();
    let mut input = revise_input("no-such-note");
    input.section = Some("Statement".to_owned());
    input.content = Some("x".to_owned());

    let err = revise(&server, input).await.expect_err("unknown");
    assert_eq!(rejection_code(&err), "not_found");
}

#[tokio::test]
async fn revise_note_treats_a_blank_hash_as_missing() {
    let mut input = revise_input(REVISE_PATH);
    input.expected_hash = Some("  ".to_owned());
    input.body = Some("\nNew body.\n".to_owned());
    assert_invalid(input, "expected_hash").await;
}

#[tokio::test]
async fn revise_note_trims_a_padded_section_heading_onto_the_existing_section() {
    let (server, store) = revise_server();
    let mut input = revise_input(REVISE_PATH);
    input.section = Some(" Statement ".to_owned());
    input.content = Some("A padded heading still finds its section.".to_owned());

    let json = decode_json(&revise(&server, input).await.expect("revised"));
    assert_eq!(
        json["section_target"],
        "concepts/woodbury-identity#Statement"
    );
    let written = store.read_file(&vp(REVISE_PATH)).unwrap();
    assert_eq!(written.matches("Statement").count(), 1, "{written}");
    assert!(written.ends_with("## Statement\n\nA padded heading still finds its section.\n"));
}

/// Refuse a section revision of `REVISE_PATH` as `revision_invalid`,
/// leaving the note and the daily log untouched.
async fn assert_revision_invalid(section: &str, content: &str) {
    let (server, store) = revise_server();
    let mut input = revise_input(REVISE_PATH);
    input.section = Some(section.to_owned());
    input.content = Some(content.to_owned());

    let err = revise(&server, input).await.expect_err("invalid");
    assert_eq!(rejection_code(&err), "revision_invalid");
    assert_eq!(store.read_file(&vp(REVISE_PATH)).unwrap(), REVISE_NOTE);
    assert!(!today_daily(&store).contains("revised"));
}

#[tokio::test]
async fn revise_note_refuses_a_heading_containing_a_pipe() {
    assert_revision_invalid("Bad|Heading", "x").await;
}

#[tokio::test]
async fn revise_note_refuses_content_that_adds_a_same_level_heading() {
    assert_revision_invalid("Statement", "x\n\n## Smuggled\n\ny").await;
}

#[tokio::test]
async fn revise_note_refuses_a_daily_note() {
    let (server, store) = revise_server();
    seed_today_daily(&store);
    let before = today_daily(&store);
    let mut input = revise_input("today");
    input.section = Some("Logs".to_owned());
    input.content = Some("x".to_owned());

    let err = revise(&server, input).await.expect_err("a daily note");
    assert_eq!(rejection_code(&err), "note_not_revisable");
    assert_eq!(today_daily(&store), before);
}

#[tokio::test]
async fn revise_note_refuses_an_uppercase_copy_of_the_right_hash_as_stale() {
    let (server, store) = revise_server();
    let hash = read_hash(&server, REVISE_PATH).await;
    assert_ne!(hash, hash.to_uppercase(), "the hash has hex letters");
    let mut input = revise_input(REVISE_PATH);
    input.expected_hash = Some(hash.to_uppercase());
    input.body = Some("\nNew body.\n".to_owned());

    let err = revise(&server, input).await.expect_err("case-sensitive");
    assert_eq!(rejection_code(&err), "stale_revision");
    assert_eq!(store.read_file(&vp(REVISE_PATH)).unwrap(), REVISE_NOTE);
}

#[tokio::test]
async fn revise_note_refuses_a_slug_two_notes_share_as_ambiguous() {
    let (server, store) = server_with_config(config_with_person_and_concept(), |v, s| {
        s.write_file(&vp(REVISE_PATH), REVISE_NOTE).unwrap();
        s.write_file(
            &vp("projects/woodbury-identity.md"),
            "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Woodbury project\n",
        )
        .unwrap();
        v.reconcile().unwrap();
    });
    let mut input = revise_input("woodbury-identity");
    input.section = Some("Statement".to_owned());
    input.content = Some("x".to_owned());

    let err = revise(&server, input)
        .await
        .expect_err("two notes share the slug");
    assert_eq!(rejection_code(&err), "ambiguous_slug");
    assert_eq!(store.read_file(&vp(REVISE_PATH)).unwrap(), REVISE_NOTE);
    assert!(!today_daily(&store).contains("revised"));
}

// ---------------------------------------------------------------------
// note_to_daily (RFC 0002 T10, #623)
// ---------------------------------------------------------------------

fn note_input(heading: &str, body: &str) -> NoteToDailyInput {
    NoteToDailyInput {
        date: None,
        heading: heading.to_owned(),
        body: body.to_owned(),
    }
}

/// The index of the line equal to `line` in `body`, panicking with the
/// note when it is absent.
fn line_index(body: &str, line: &str) -> usize {
    body.lines()
        .position(|l| l == line)
        .unwrap_or_else(|| panic!("no line {line:?} in:\n{body}"))
}

/// The one `## Logs` line ending in `: <log_line>`, checked to be exactly
/// `- **HH:MM**: <log_line>` and to sit below `## Logs`.
fn assert_pointer_line(body: &str, log_line: &str) {
    let suffix = format!("**: {log_line}");
    let found: Vec<(usize, &str)> = body
        .lines()
        .enumerate()
        .filter(|(_, l)| l.ends_with(&suffix))
        .collect();
    assert_eq!(found.len(), 1, "exactly one pointer line in:\n{body}");
    let (index, line) = found[0];
    let stamp = &line[..line.len() - suffix.len()];
    assert!(
        stamp.len() == 9
            && stamp.starts_with("- **")
            && stamp[4..6].chars().all(|c| c.is_ascii_digit())
            && &stamp[6..7] == ":"
            && stamp[7..9].chars().all(|c| c.is_ascii_digit()),
        "pointer line not `- **HH:MM**: …`: {line:?}"
    );
    assert!(
        index > line_index(body, "## Logs"),
        "pointer outside ## Logs:\n{body}"
    );
}

#[tokio::test]
async fn note_to_daily_keeps_the_bodys_leading_indentation() {
    let (server, store) = server_with(|_v, store| seed_today_daily(&store));
    let today = chrono::Local::now().date_naive();
    let relpath = cdno_core::paths::daily_note_relpath(today);

    // An entry that opens with an indented code block: only the leading
    // blank lines and the trailing whitespace go; the first line's
    // indentation is part of the substance.
    server
        .note_to_daily(Parameters(note_input(
            "Indented",
            "\n\n    x = A^-1 u\n    y = x + 1\n\n",
        )))
        .await
        .expect("note_to_daily");

    let body = store.read_file(&vp(&relpath)).unwrap();
    assert!(
        body.contains("### Indented\n    x = A^-1 u\n    y = x + 1\n"),
        "body:\n{body}"
    );
}

#[tokio::test]
async fn note_to_daily_writes_the_entry_under_notes_and_the_pointer_under_logs() {
    let (server, store) = server_with(|_v, store| seed_today_daily(&store));
    let today = chrono::Local::now().date_naive();
    let relpath = cdno_core::paths::daily_note_relpath(today);
    let target = format!("{}#Woodbury identity", relpath.trim_end_matches(".md"));

    let result = server
        .note_to_daily(Parameters(note_input(
            "  Woodbury identity  ",
            "(A + UCV)^-1 = A^-1 - ...\n",
        )))
        .await
        .expect("note_to_daily");
    let value = decode_json(&result);
    assert_eq!(value["path"], relpath.as_str());
    assert_eq!(value["target"], target.as_str());
    assert_eq!(value["log_line"], format!("noted [[{target}]]").as_str());
    let tail = value["verification"]["appended_tail"].as_str().unwrap();
    assert!(tail.contains("### Woodbury identity"), "tail: {tail}");

    let body = store.read_file(&vp(&relpath)).unwrap();
    let notes = line_index(&body, "## Notes");
    let entry = line_index(&body, "### Woodbury identity");
    let logs = line_index(&body, "## Logs");
    assert!(
        notes < entry && entry < logs,
        "entry not under ## Notes:\n{body}"
    );
    assert!(body.contains("(A + UCV)^-1 = A^-1 - ..."), "body:\n{body}");
    assert_pointer_line(&body, &format!("noted [[{target}]]"));
}

#[tokio::test]
async fn note_to_daily_lists_the_body_links_after_the_pointer() {
    let (server, store) = server_with(|_v, store| seed_today_daily(&store));

    let result = server
        .note_to_daily(Parameters(note_input(
            "Refit",
            "Used on [[projects/surrogate-model]] with [[notes/kernels|kernels]] and again [[projects/surrogate-model]].\n#concept",
        )))
        .await
        .expect("note_to_daily");
    let value = decode_json(&result);
    let target = value["target"].as_str().unwrap().to_owned();
    let expected =
        format!("noted [[{target}]] ([[projects/surrogate-model]] [[notes/kernels|kernels]])");
    assert_eq!(value["log_line"], expected.as_str());

    let body = store
        .read_file(&vp(value["path"].as_str().unwrap()))
        .unwrap();
    assert_pointer_line(&body, &expected);
}

#[tokio::test]
async fn note_to_daily_refuses_a_duplicate_heading_and_leaves_the_file_untouched() {
    let (server, store) = server_with(|_v, store| seed_today_daily(&store));
    server
        .note_to_daily(Parameters(note_input("Woodbury identity", "first")))
        .await
        .expect("first entry");
    let today = chrono::Local::now().date_naive();
    let path = vp(&cdno_core::paths::daily_note_relpath(today));
    let before = store.read_file(&path).unwrap();

    let err = server
        .note_to_daily(Parameters(note_input("woodbury identity", "second")))
        .await
        .expect_err("the heading is already in today's note");
    assert_eq!(rejection_code(&err), "history_entry_heading_invalid");
    assert_eq!(store.read_file(&path).unwrap(), before);
}

#[tokio::test]
async fn note_to_daily_refuses_a_heading_that_reuses_a_section_name() {
    let (server, store) = server_with(|_v, store| seed_today_daily(&store));
    let today = chrono::Local::now().date_naive();
    let path = vp(&cdno_core::paths::daily_note_relpath(today));
    let before = store.read_file(&path).unwrap();

    let err = server
        .note_to_daily(Parameters(note_input("Logs", "not a section")))
        .await
        .expect_err("`Logs` is a daily section name");
    assert_eq!(rejection_code(&err), "history_entry_heading_invalid");
    assert_eq!(store.read_file(&path).unwrap(), before);
}

#[tokio::test]
async fn note_to_daily_refuses_a_blank_heading_or_body_as_invalid_params() {
    let (server, store) = server_with(|_v, store| seed_today_daily(&store));
    let today = chrono::Local::now().date_naive();
    let path = vp(&cdno_core::paths::daily_note_relpath(today));
    let before = store.read_file(&path).unwrap();

    for (input, field) in [
        (note_input("  ", "substance"), "heading"),
        (note_input("Heading", " \n\t "), "body"),
    ] {
        let err = server
            .note_to_daily(Parameters(input))
            .await
            .expect_err("blank field");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS, "{err:?}");
        assert!(err.message.contains(field), "{field}: {}", err.message);
    }
    assert_eq!(store.read_file(&path).unwrap(), before);
}

#[tokio::test]
async fn note_to_daily_with_a_past_date_lands_in_that_days_note() {
    let (server, store) = server_with(|_v, _s| {});
    let day = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
    let mut input = note_input("Back-filled", "worked out yesterday");
    input.date = Some(day);

    let value = decode_json(&server.note_to_daily(Parameters(input)).await.expect("note"));
    assert_eq!(value["path"], "journal/2026/daily/2026-09-01.md");
    assert_eq!(value["target"], "journal/2026/daily/2026-09-01#Back-filled");

    let body = store
        .read_file(&vp("journal/2026/daily/2026-09-01.md"))
        .unwrap();
    assert!(
        line_index(&body, "## Notes") < line_index(&body, "### Back-filled"),
        "body:\n{body}"
    );
    assert_pointer_line(&body, "noted [[journal/2026/daily/2026-09-01#Back-filled]]");
}

#[tokio::test]
async fn upsert_daily_section_refuses_replacing_notes_as_invalid_params() {
    let (server, store) = server_with(|_v, store| seed_today_daily(&store));
    let today = chrono::Local::now().date_naive();
    let path = vp(&cdno_core::paths::daily_note_relpath(today));
    let before = store.read_file(&path).unwrap();

    let err = server
        .upsert_daily_section(Parameters(UpsertDailySectionInput {
            section: "notes".to_owned(),
            content: "### Replaced\nnothing".to_owned(),
            date: None,
            append: false,
        }))
        .await
        .expect_err("Notes is append-only");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("append"), "{}", err.message);
    assert!(err.message.contains("append-only"), "{}", err.message);
    assert!(err.message.contains("note_to_daily"), "{}", err.message);
    assert_eq!(store.read_file(&path).unwrap(), before);
}

#[tokio::test]
async fn upsert_daily_section_appends_to_notes() {
    let (server, store) = server_with(|_v, store| seed_today_daily(&store));

    let value = decode_json(
        &server
            .upsert_daily_section(Parameters(UpsertDailySectionInput {
                section: "notes".to_owned(),
                content: "### Appended entry\nsubstance".to_owned(),
                date: None,
                append: true,
            }))
            .await
            .expect("append to Notes"),
    );
    let body = store
        .read_file(&vp(value["path"].as_str().unwrap()))
        .unwrap();
    let notes = line_index(&body, "## Notes");
    let entry = line_index(&body, "### Appended entry");
    assert!(
        notes < entry && entry < line_index(&body, "## Logs"),
        "body:\n{body}"
    );
    assert!(body.contains("substance"), "body:\n{body}");
}

// ---------------------------------------------------------------------
// complete_project / drop_project (RFC 0004)
// ---------------------------------------------------------------------

/// The `cdno_rejection` payload a handler error carries.
fn rejection_of(err: &rmcp::model::ErrorData) -> serde_json::Value {
    err.data
        .as_ref()
        .and_then(|d| d.get("cdno_rejection"))
        .cloned()
        .unwrap_or_else(|| panic!("not marked as a rejection: {err:?}"))
}

fn this_year() -> String {
    chrono::Local::now().format("%Y").to_string()
}

/// A fresh project: the template seeds one open bullet and one open
/// milestone.
fn server_with_widget() -> (CuadernoServer, Arc<dyn VaultStore>) {
    server_with(|vault, _s| {
        vault
            .create_project(moment(2026, 1, 1, 9, 0), "Widget", Context::Work, None)
            .unwrap();
    })
}

#[tokio::test]
async fn complete_project_tool_refuses_with_open_items() {
    let (server, store) = server_with_widget();

    let err = server
        .complete_project(Parameters(ProjectSlugInput {
            project: "widget".to_owned(),
        }))
        .await
        .expect_err("open items refuse a completion");

    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "project_has_open_items", "{rejection}");
    let details = &rejection["details"];
    assert_eq!(details["slug"], "widget");
    assert_eq!(
        details["actions"][0]["text"],
        "Define first concrete step (light)"
    );
    assert_eq!(details["milestones"].as_array().unwrap().len(), 1);
    assert!(details["open_items_hash"].as_str().is_some());
    assert!(store.exists(&vp("projects/widget.md")).unwrap());
}

#[tokio::test]
async fn complete_project_tool_moves_a_map_with_nothing_open() {
    let (server, store) = server_with(|_vault, store| {
        store
            .write_file(
                &vp("projects/done.md"),
                "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-01-01\ncore_question: null\nclosed: null\n---\n\n# Done\n\n## Next Actions\n- [x] Ship (deep)\n",
            )
            .unwrap();
    });

    let result = server
        .complete_project(Parameters(ProjectSlugInput {
            project: "done".to_owned(),
        }))
        .await
        .expect("complete_project");

    let body = decode_json(&result);
    let path = format!("projects/_done/{}/done.md", this_year());
    assert_eq!(body["path"], path);
    assert_eq!(body["dropped_actions"], serde_json::json!([]));
    assert!(body["verification"].is_object(), "{body}");
    assert!(
        store
            .read_file(&vp(&path))
            .unwrap()
            .contains("status: completed")
    );
}

#[tokio::test]
async fn drop_project_with_open_items_drop_cascades() {
    let (server, store) = server_with_widget();
    let refusal = server
        .drop_project(Parameters(DropProjectInput {
            project: "widget".to_owned(),
            reason: Some("superseded".to_owned()),
            open_items: OpenItemsChoice::Refuse,
            expected_open_items: None,
        }))
        .await
        .expect_err("refused by default");
    let hash = rejection_of(&refusal)["details"]["open_items_hash"]
        .as_str()
        .unwrap()
        .to_owned();

    let result = server
        .drop_project(Parameters(DropProjectInput {
            project: "widget".to_owned(),
            reason: Some("superseded".to_owned()),
            open_items: OpenItemsChoice::Drop,
            expected_open_items: Some(hash),
        }))
        .await
        .expect("the confirmed cascade drops");

    let body = decode_json(&result);
    let path = format!("projects/_done/{}/widget.md", this_year());
    assert_eq!(body["path"], path);
    assert_eq!(
        body["dropped_actions"],
        serde_json::json!(["Define first concrete step (light)"])
    );
    assert_eq!(body["dropped_milestones"].as_array().unwrap().len(), 1);
    let raw = store.read_file(&vp(&path)).unwrap();
    assert!(raw.contains("status: dropped"), "{raw}");
    assert!(!raw.contains("- [ ]"), "{raw}");
}

#[tokio::test]
async fn drop_project_with_stale_hash_is_re_refused() {
    let (server, store) = server_with_widget();
    let refusal = server
        .drop_project(Parameters(DropProjectInput {
            project: "widget".to_owned(),
            reason: None,
            open_items: OpenItemsChoice::Refuse,
            expected_open_items: None,
        }))
        .await
        .expect_err("refused by default");
    let old_hash = rejection_of(&refusal)["details"]["open_items_hash"]
        .as_str()
        .unwrap()
        .to_owned();
    server
        .add_action(Parameters(AddActionInput {
            slug: None,
            project: "widget".to_owned(),
            title: "Something new".to_owned(),
            energy: "light".to_owned(),
            with_note: false,
            vars: None,
        }))
        .await
        .expect("add_action");

    let err = server
        .drop_project(Parameters(DropProjectInput {
            project: "widget".to_owned(),
            reason: None,
            open_items: OpenItemsChoice::Drop,
            expected_open_items: Some(old_hash.clone()),
        }))
        .await
        .expect_err("a changed list is refused again");

    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "project_has_open_items");
    assert_ne!(rejection["details"]["open_items_hash"], old_hash);
    assert_eq!(rejection["details"]["actions"].as_array().unwrap().len(), 2);
    assert!(store.exists(&vp("projects/widget.md")).unwrap());
}

#[tokio::test]
async fn drop_project_drop_without_hash_is_refused_with_the_list() {
    let (server, store) = server_with_widget();

    let err = server
        .drop_project(Parameters(DropProjectInput {
            project: "widget".to_owned(),
            reason: None,
            open_items: OpenItemsChoice::Drop,
            expected_open_items: None,
        }))
        .await
        .expect_err("an agent must present the hash it was shown");

    assert_eq!(rejection_of(&err)["code"], "project_has_open_items");
    assert!(store.exists(&vp("projects/widget.md")).unwrap());
}

// --- pause / switch / resume (RFC 0005, #735) --------------------------

/// Today at `h:m`, the day the handlers (which read the real clock) stamp.
fn today_at(h: u32, m: u32) -> NaiveDateTime {
    chrono::Local::now()
        .date_naive()
        .and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap())
}

/// A project with two open bullets, the first already in focus since 09:10.
fn server_with_focus() -> (CuadernoServer, Arc<dyn VaultStore>) {
    server_with(|vault, _s| {
        vault
            .create_project(
                moment(2026, 5, 1, 9, 0),
                "Surrogate model",
                Context::Work,
                None,
            )
            .unwrap();
        for title in ["Draft methods section", "Run ablation"] {
            vault
                .add_action(
                    moment(2026, 5, 1, 9, 5),
                    "surrogate-model",
                    title,
                    EnergyLevel::Deep,
                )
                .unwrap();
        }
        vault
            .start_action(today_at(9, 10), "surrogate-model", "Draft methods")
            .unwrap();
    })
}

fn todays_daily(store: &Arc<dyn VaultStore>) -> String {
    let today = chrono::Local::now().date_naive();
    store
        .read_file(&vp(&format!(
            "journal/{}/daily/{}.md",
            today.format("%Y"),
            today.format("%Y-%m-%d")
        )))
        .expect("daily written")
}

#[tokio::test]
async fn pause_action_logs_the_pause_and_returns_the_paused_focus() {
    let (server, store) = server_with_focus();
    let result = server
        .pause_action(Parameters(PauseActionInput {
            next: Some("rerun with the new seed".to_owned()),
            reason: Some("meeting".to_owned()),
        }))
        .await
        .expect("pause_action");
    let payload = decode_json(&result);
    assert_eq!(payload["paused"]["project"], "surrogate-model");
    assert_eq!(payload["paused"]["action"], "Draft methods section (deep)");
    assert_eq!(payload["paused"]["started"], "09:10");
    assert_eq!(payload["paused"]["carried"], false);
    assert!(payload["paused"]["origin"].is_null());
    assert!(payload["verification"]["verified"].is_string());

    let body = todays_daily(&store);
    assert!(
        body.contains(
            "action paused on [[surrogate-model]] \u{2014} Draft methods section (deep)\n  next: rerun with the new seed\n  reason: meeting"
        ),
        "{body}"
    );
}

#[tokio::test]
async fn pause_action_with_nothing_in_focus_is_a_no_focus_rejection() {
    let (server, _store) = server_with_project();
    let err = server
        .pause_action(Parameters(PauseActionInput {
            next: None,
            reason: None,
        }))
        .await
        .expect_err("nothing to pause");
    assert_eq!(rejection_of(&err)["code"], "no_focus");
    assert_eq!(
        rejection_of(&err)["message"],
        cdno_domain::NO_FOCUS_TO_PAUSE_MESSAGE
    );
}

/// A second pause is refused, but a resumable pause exists, so the message
/// must not say there is none (review of #769).
#[tokio::test]
async fn a_second_pause_says_nothing_is_in_focus_not_that_nothing_is_paused() {
    let (server, _store) = server_with_focus();
    let pause = || PauseActionInput {
        next: None,
        reason: None,
    };
    server
        .pause_action(Parameters(pause()))
        .await
        .expect("first pause");
    let err = server
        .pause_action(Parameters(pause()))
        .await
        .expect_err("nothing left to pause");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "no_focus");
    assert_eq!(rejection["message"], "Nothing is in focus to pause.");

    server
        .resume_action(Parameters(ResumeActionInput { project: None }))
        .await
        .expect("the pause is still resumable");
}

#[tokio::test]
async fn resume_action_with_nothing_to_resume_is_a_no_focus_rejection_with_its_own_message() {
    let (server, _store) = server_with_project();
    let err = server
        .resume_action(Parameters(ResumeActionInput { project: None }))
        .await
        .expect_err("nothing to resume");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "no_focus");
    assert_eq!(
        rejection["message"],
        "Nothing to resume: no focus carried over and no pause."
    );
}

#[tokio::test]
async fn switch_action_pauses_the_focus_and_starts_the_target() {
    let (server, store) = server_with_focus();
    let result = server
        .switch_action(Parameters(SwitchActionInput {
            project: "surrogate-model".to_owned(),
            query: "Run ablation".to_owned(),
            next: Some("section 3 next".to_owned()),
            reason: None,
        }))
        .await
        .expect("switch_action");
    let payload = decode_json(&result);
    assert_eq!(payload["paused"]["action"], "Draft methods section (deep)");
    assert_eq!(payload["started"]["action"], "Run ablation (deep)");

    let body = todays_daily(&store);
    let paused = body
        .find("action paused on [[surrogate-model]] \u{2014} Draft methods section (deep)")
        .unwrap_or_else(|| panic!("{body}"));
    let started = body
        .find("started [[surrogate-model]] \u{2014} Run ablation (deep)")
        .unwrap_or_else(|| panic!("{body}"));
    assert!(paused < started, "pause, then start:\n{body}");
    assert!(body.contains("  next: section 3 next"), "{body}");
}

#[tokio::test]
async fn switch_unplanned_action_adds_the_bullet_pauses_and_starts_it() {
    let (server, store) = server_with_focus();
    let result = server
        .switch_unplanned_action(Parameters(SwitchUnplannedActionInput {
            project: "surrogate-model".to_owned(),
            title: "Fix the CI badge".to_owned(),
            energy: "light".to_owned(),
            next: None,
            reason: Some("build is red".to_owned()),
        }))
        .await
        .expect("switch_unplanned_action");
    let payload = decode_json(&result);
    assert_eq!(payload["paused"]["action"], "Draft methods section (deep)");
    assert_eq!(payload["started"]["action"], "Fix the CI badge (light)");

    let map = store
        .read_file(&vp("projects/surrogate-model.md"))
        .expect("map readable");
    assert!(map.contains("- [ ] Fix the CI badge (light)"), "{map}");
    let body = todays_daily(&store);
    assert!(body.contains("  reason: build is red"), "{body}");
    assert!(
        body.contains("started [[surrogate-model]] \u{2014} Fix the CI badge (light)"),
        "{body}"
    );
}

#[tokio::test]
async fn resume_action_reopens_a_pause_and_reports_where_it_came_from() {
    let (server, store) = server_with_focus();
    server
        .pause_action(Parameters(PauseActionInput {
            next: Some("rerun with the new seed".to_owned()),
            reason: None,
        }))
        .await
        .expect("pause_action");

    let result = server
        .resume_action(Parameters(ResumeActionInput {
            project: Some("surrogate-model".to_owned()),
        }))
        .await
        .expect("resume_action");
    let payload = decode_json(&result);
    assert_eq!(payload["resumed"]["action"], "Draft methods section (deep)");
    assert_eq!(payload["resumed_from"]["kind"], "paused");
    assert_eq!(
        payload["resumed_from"]["next"], "rerun with the new seed",
        "{payload}"
    );
    assert!(payload["resumed_from"]["reason"].is_null());
    assert_eq!(
        payload["resumed_from"]["date"],
        chrono::Local::now().date_naive().to_string()
    );

    let body = todays_daily(&store);
    assert!(
        body.contains("resumed [[surrogate-model]] \u{2014} Draft methods section (deep)"),
        "{body}"
    );
}

// ---------------------------------------------------------------------
// RFC 0006 T14 (#797): parked maps over MCP
// ---------------------------------------------------------------------

/// O7 (RFC 0006 §2): at the cap, the path an agent actually takes is
/// create, then add the first action. The map lands parked and takes the
/// action as it is; nothing in the chain calls `activate_project`.
#[tokio::test]
async fn o7_chain_create_at_cap_then_add_action_on_the_parked_map_succeeds() {
    let (server, store) = server_with(|vault, store| {
        seed_today_daily(&store);
        let today = moment(2026, 1, 1, 9, 0);
        for i in 1..=5 {
            vault
                .create_project(today, &format!("Project {i}"), Context::Work, None)
                .expect("seed project");
        }
    });

    let created = server
        .create_project(Parameters(CreateProjectInput {
            slug: None,
            title: "Glacier survey".to_owned(),
            context: "work".to_owned(),
            core_question: None,
            vars: None,
        }))
        .await
        .expect("creating at the cap is never refused");
    let path = decode_json(&created)["path"].as_str().unwrap().to_owned();
    assert_eq!(path, "projects/_parked/glacier-survey.md");

    server
        .add_action(Parameters(AddActionInput {
            slug: None,
            project: "glacier-survey".to_owned(),
            title: "Book the field hut".to_owned(),
            energy: "light".to_owned(),
            with_note: false,
            vars: None,
        }))
        .await
        .expect("a parked map takes its first action straight away");

    let map = store.read_file(&vp(&path)).unwrap();
    assert!(map.contains("- [ ] Book the field hut (light)"), "{map}");
    assert!(map.contains("status: parked"), "still parked:\n{map}");
    assert!(
        !store.exists(&vp("projects/glacier-survey.md")).unwrap(),
        "the map was never activated"
    );
}

/// A map you start work on is a map that counts (RFC 0006 §7.1): the
/// focus verbs keep refusing a parked map, with the status in `details`.
#[tokio::test]
async fn start_action_on_a_parked_project_is_project_not_active() {
    let (server, _store) = server_with(|vault, _s| {
        let at = moment(2026, 5, 1, 9, 0);
        vault
            .create_project(at, "Glacier survey", Context::Work, None)
            .unwrap();
        vault
            .add_action(
                at,
                "glacier-survey",
                "Book the field hut",
                EnergyLevel::Light,
            )
            .unwrap();
        vault.park_project(at, "glacier-survey").unwrap();
    });

    let err = server
        .start_action(Parameters(StartActionInput {
            project: "glacier-survey".to_owned(),
            query: "field hut".to_owned(),
        }))
        .await
        .expect_err("work cannot start on a parked map");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "project_not_active");
    assert_eq!(rejection["details"]["slug"], "glacier-survey");
    assert_eq!(rejection["details"]["status"], "parked");
    // O7: the message an agent reads on a parked map must not send it round
    // the activate-first loop; the focus verb is what was refused.
    let message = rejection["message"].as_str().expect("message");
    assert!(!message.contains("activate"), "{message}");
}

/// A pause on a map parked since is "picking up work you had set down"
/// (RFC 0006 §7.1): `resume_action` refuses it as `project_not_active`.
#[tokio::test]
async fn resume_action_on_a_since_parked_project_is_project_not_active() {
    let (server, store) = server_with_focus();
    server
        .pause_action(Parameters(PauseActionInput {
            next: None,
            reason: None,
        }))
        .await
        .expect("pause_action");
    server
        .park_project(Parameters(ProjectSlugInput {
            project: "surrogate-model".to_owned(),
        }))
        .await
        .expect("park_project");
    let before = todays_daily(&store);

    let err = server
        .resume_action(Parameters(ResumeActionInput { project: None }))
        .await
        .expect_err("the pause sits on a map parked since");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "project_not_active");
    assert_eq!(rejection["details"]["slug"], "surrogate-model");
    assert_eq!(rejection["details"]["status"], "parked");
    let message = rejection["message"].as_str().expect("message");
    assert!(!message.contains("activate"), "{message}");
    assert_eq!(todays_daily(&store), before, "nothing is logged");
}

/// Runtime strings carry what happened, never agent guidance: the FOCUS
/// pointers belong in tool descriptions (review of #769, where they had
/// landed in this message and in three error field names).
#[tokio::test]
async fn runtime_strings_do_not_carry_the_focus_pointers() {
    let (server, _store) = server_with_focus();
    server
        .pause_action(Parameters(PauseActionInput {
            next: None,
            reason: None,
        }))
        .await
        .expect("pause_action");
    let result = server
        .resume_action(Parameters(ResumeActionInput { project: None }))
        .await
        .expect("resume_action");
    let payload = decode_json(&result);
    let message = payload["message"].as_str().expect("message");
    assert_eq!(
        message,
        format!(
            "Resumed on surrogate-model, logged to {}",
            payload["path"].as_str().expect("path")
        )
    );
    assert!(!message.contains("FOCUS section"), "{message}");

    let err = server
        .set_core_question(Parameters(SetCoreQuestionInput {
            project: "surrogate-model".to_owned(),
            core_question: Some("questions/research/foo".to_owned()),
            clear: true,
        }))
        .await
        .expect_err("both given");
    assert_eq!(
        err.message,
        "invalid 'clear': pass either `core_question` or `clear: true`, not both"
    );

    let err = server
        .note_to_daily(Parameters(note_input("  ", "substance")))
        .await
        .expect_err("blank heading");
    assert!(
        err.message.starts_with("invalid 'heading': required:"),
        "{}",
        err.message
    );
}

#[tokio::test]
async fn start_action_rejection_carries_attempted() {
    let (server, _store) = server_with_focus();
    let err = server
        .start_action(Parameters(StartActionInput {
            project: "surrogate-model".to_owned(),
            query: "Run ablation".to_owned(),
        }))
        .await
        .expect_err("a second start must be refused");

    // The whole fixture the CLI's `--json` is compared against too. The
    // handlers read the real clock, so the focus seeded at 09:10 today is
    // not carried and only the fixture's date is today's rather than the
    // literal one. (The seed and this date each read the clock, so a run
    // straddling midnight could disagree; the window is milliseconds.)
    let mut fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/focus_open_rejection.json"))
            .expect("fixture parses");
    fixture["details"]["focus"]["date"] =
        serde_json::json!(chrono::Local::now().date_naive().to_string());
    assert_eq!(rejection_of(&err), fixture);
}

#[tokio::test]
async fn unplanned_and_switch_rejections_carry_attempted_in_the_cli_shape() {
    let (server, _store) = server_with_focus();
    let err = server
        .start_unplanned_action(Parameters(StartUnplannedActionInput {
            project: "surrogate-model".to_owned(),
            title: "Fix the CI badge".to_owned(),
            energy: "light".to_owned(),
        }))
        .await
        .expect_err("refused");
    assert_eq!(
        rejection_of(&err)["details"]["attempted"],
        serde_json::json!({ "project": "surrogate-model", "title": "Fix the CI badge" })
    );

    // Switching to the action already in focus is refused likewise.
    let err = server
        .switch_action(Parameters(SwitchActionInput {
            project: "surrogate-model".to_owned(),
            query: "Draft methods".to_owned(),
            next: None,
            reason: None,
        }))
        .await
        .expect_err("refused");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["details"]["remedy"], "already_focused");
    assert_eq!(
        rejection["details"]["attempted"],
        serde_json::json!({ "project": "surrogate-model", "query": "Draft methods" })
    );
}

#[tokio::test]
async fn switch_with_nothing_open_says_it_was_a_plain_start() {
    let (server, _store) = server_with_project();
    server
        .add_action(Parameters(AddActionInput {
            slug: None,
            project: "surrogate-model".to_owned(),
            title: "Run ablation".to_owned(),
            energy: "deep".to_owned(),
            with_note: false,
            vars: None,
        }))
        .await
        .expect("add_action");
    let result = server
        .switch_action(Parameters(SwitchActionInput {
            project: "surrogate-model".to_owned(),
            query: "Run ablation".to_owned(),
            next: Some("ignored".to_owned()),
            reason: None,
        }))
        .await
        .expect("switch_action");
    let payload = decode_json(&result);
    assert!(payload["paused"].is_null());
    assert_eq!(
        payload["message"],
        "Nothing was open \u{2014} started Run ablation. (next ignored: nothing to attach it to)"
    );
}

#[tokio::test]
async fn only_a_focus_open_rejection_gets_attempted() {
    let (server, _store) = server_with_focus();
    let err = server
        .start_action(Parameters(StartActionInput {
            project: "surrogate-model".to_owned(),
            query: "Buy milk".to_owned(),
        }))
        .await
        .expect_err("no such bullet");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "action_not_found");
    assert!(
        rejection["details"].get("attempted").is_none(),
        "{rejection}"
    );

    // Both seeded bullets contain "e": ambiguous, likewise without `attempted`.
    let err = server
        .start_action(Parameters(StartActionInput {
            project: "surrogate-model".to_owned(),
            query: "e".to_owned(),
        }))
        .await
        .expect_err("ambiguous");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "ambiguous_action");
    assert!(
        rejection["details"].get("attempted").is_none(),
        "{rejection}"
    );
}

// --- hand_off_project (RFC 0006 §8.2, T20) ------------------------------

/// The configured cap, read from the default config rather than written
/// out, so no test here restates a policy value.
fn default_cap() -> usize {
    usize::from(VaultConfig::default().vault.max_active_projects)
}

/// A vault whose counted maps fill the cap: `project-1` … `project-<cap>`,
/// all active and the person's own.
fn seed_full_cap(vault: &Vault) {
    let at = moment(2026, 1, 1, 9, 0);
    for i in 1..=default_cap() {
        vault
            .create_project(at, &format!("Project {i}"), Context::Work, None)
            .expect("seed project");
    }
}

/// Hand `slug` to a holder through the domain, as a seed step.
fn seed_held(vault: &Vault, slug: &str, holder: &str) {
    vault
        .hand_off(
            moment(2026, 1, 2, 9, 0),
            slug,
            cdno_domain::HolderChange::HandTo(holder.to_owned()),
        )
        .expect("seed hand-off");
}

fn hand_off(
    project: &str,
    holder: Option<&str>,
    take_back: bool,
) -> Parameters<HandOffProjectInput> {
    Parameters(HandOffProjectInput {
        project: project.to_owned(),
        holder: holder.map(str::to_owned),
        take_back,
    })
}

/// A quoted wikilink holder goes on the map as a quoted scalar, comes back
/// out of the reader as a name (so the map stops counting: the next project
/// created at what was the cap is active), and logs one `holder on` entry.
/// The payload carries `focus`, like every write that changes a map.
#[tokio::test]
async fn hand_off_project_round_trips_a_quoted_wikilink_and_the_map_stops_counting() {
    let (server, store) = server_with(|vault, store| {
        seed_today_daily(&store);
        seed_full_cap(vault);
    });

    let result = server
        .hand_off_project(hand_off("project-1", Some("[[people/ana]]"), false))
        .await
        .expect("hand_off_project");
    let payload = decode_json(&result);
    assert_eq!(payload["path"], "projects/project-1.md");
    assert!(
        payload.get("focus").is_some_and(|f| f.is_null()),
        "the payload carries `focus`: {payload}"
    );

    let map = store.read_file(&vp("projects/project-1.md")).unwrap();
    assert!(map.contains("held_by: \"[[people/ana]]\"\n"), "{map}");
    assert!(map.contains("status: active"), "still active:\n{map}");

    let today = chrono::Local::now().date_naive();
    let daily = store
        .read_file(&vp(&cdno_core::paths::daily_note_relpath(today)))
        .unwrap();
    assert_eq!(
        daily.matches("holder on [[project-1]]").count(),
        1,
        "{daily}"
    );
    assert!(daily.contains("now: [[people/ana]]"), "{daily}");

    // The map no longer holds a slot, so creating one more is not parked.
    let created = server
        .create_project(Parameters(CreateProjectInput {
            slug: None,
            title: "Glacier survey".to_owned(),
            context: "work".to_owned(),
            core_question: None,
            vars: None,
        }))
        .await
        .expect("create_project");
    assert_eq!(decode_json(&created)["path"], "projects/glacier-survey.md");

    // The same holder again changes nothing and logs nothing.
    let again = server
        .hand_off_project(hand_off("project-1", Some("[[people/ana]]"), false))
        .await
        .expect("same holder again");
    let message = decode_json(&again)["message"].as_str().unwrap().to_owned();
    assert!(message.contains("nothing changed"), "{message}");
    let daily = store
        .read_file(&vp(&cdno_core::paths::daily_note_relpath(today)))
        .unwrap();
    assert_eq!(
        daily.matches("holder on [[project-1]]").count(),
        1,
        "{daily}"
    );
}

/// Exactly one of `holder` and `take_back`, as `set_core_question` takes
/// exactly one of `core_question` and `clear`: an omitted holder must not
/// quietly take a project back.
#[tokio::test]
async fn hand_off_project_takes_exactly_one_of_holder_and_take_back() {
    let (server, store) = server_with(|vault, _s| {
        seed_full_cap(vault);
        seed_held(vault, "project-1", "the harbour office");
    });
    let before = store.read_file(&vp("projects/project-1.md")).unwrap();

    let err = server
        .hand_off_project(hand_off("project-1", Some("a colleague"), true))
        .await
        .expect_err("both given");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(
        err.message,
        "invalid 'take_back': pass either `holder` or `take_back: true`, not both"
    );

    let err = server
        .hand_off_project(hand_off("project-1", None, false))
        .await
        .expect_err("neither given is an error, not a take-back");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(
        err.message.starts_with("invalid 'holder':"),
        "{}",
        err.message
    );
    assert!(err.message.contains("`take_back: true`"), "{}", err.message);

    assert_eq!(
        store.read_file(&vp("projects/project-1.md")).unwrap(),
        before,
        "a refused call writes nothing"
    );
}

/// You do not hand off a map while you are in the middle of its work.
#[tokio::test]
async fn hand_off_project_in_focus_is_project_in_focus() {
    let (server, store) = server_with_focus();
    let before = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    let err = server
        .hand_off_project(hand_off("surrogate-model", Some("a colleague"), false))
        .await
        .expect_err("refused while in focus");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "project_in_focus", "{rejection}");
    assert_eq!(rejection["details"]["slug"], "surrogate-model");
    assert!(
        rejection["details"]["action"]
            .as_str()
            .is_some_and(|a| a.starts_with("Draft methods section")),
        "{rejection}"
    );
    assert_eq!(
        store.read_file(&vp("projects/surrogate-model.md")).unwrap(),
        before
    );
}

/// Taking back an active held map makes it count, so at the cap it is
/// refused, listing the maps that count.
#[tokio::test]
async fn take_back_at_the_cap_is_project_cap_reached() {
    let (server, store) = server_with(|vault, _s| {
        seed_full_cap(vault);
        seed_held(vault, "project-1", "the harbour office");
        // project-1 no longer counts, so one more is created active.
        vault
            .create_project(
                moment(2026, 1, 3, 9, 0),
                "Glacier survey",
                Context::Work,
                None,
            )
            .expect("created active");
    });
    let before = store.read_file(&vp("projects/project-1.md")).unwrap();

    let err = server
        .hand_off_project(hand_off("project-1", None, true))
        .await
        .expect_err("taking back at the cap is refused");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "project_cap_reached", "{rejection}");
    assert_eq!(rejection["details"]["current"], default_cap());
    assert_eq!(rejection["details"]["max"], default_cap());
    let listed = rejection["details"]["active_projects"].as_array().unwrap();
    assert!(
        !listed.iter().any(|s| s == "project-1"),
        "the held map is not a counted one: {rejection}"
    );
    assert!(listed.iter().any(|s| s == "glacier-survey"), "{rejection}");
    assert_eq!(
        store.read_file(&vp("projects/project-1.md")).unwrap(),
        before,
        "the holder stays"
    );
}

/// Taking back an active held map below the cap writes `held_by: null`,
/// logs the change, and says the map is the person's own again.
#[tokio::test]
async fn take_back_below_the_cap_clears_the_holder_and_logs_it() {
    let (server, store) = server_with(|vault, store| {
        seed_today_daily(&store);
        vault
            .create_project(
                moment(2026, 1, 1, 9, 0),
                "Glacier survey",
                Context::Work,
                None,
            )
            .expect("seed project");
        seed_held(vault, "glacier-survey", "the harbour office");
    });

    let result = server
        .hand_off_project(hand_off("glacier-survey", None, true))
        .await
        .expect("take back");
    let payload = decode_json(&result);
    assert_eq!(
        payload["message"],
        "Recorded projects/glacier-survey.md as the person's own"
    );
    assert!(payload.get("focus").is_some(), "{payload}");

    let map = store.read_file(&vp("projects/glacier-survey.md")).unwrap();
    assert!(map.contains("held_by: null\n"), "{map}");
    assert!(map.contains("status: active"), "{map}");

    let today = chrono::Local::now().date_naive();
    let daily = store
        .read_file(&vp(&cdno_core::paths::daily_note_relpath(today)))
        .unwrap();
    assert_eq!(
        daily.matches("holder on [[glacier-survey]]").count(),
        1,
        "{daily}"
    );
    assert!(daily.contains("was: the harbour office"), "{daily}");

    // Taking back again changes nothing.
    let again = server
        .hand_off_project(hand_off("glacier-survey", None, true))
        .await
        .expect("take back again");
    assert_eq!(
        decode_json(&again)["message"],
        "projects/glacier-survey.md is already the person's own; nothing changed"
    );
}

/// Taking back a parked map only clears the name: it needs no slot, so it
/// succeeds with the counted maps at the cap, and the map stays parked.
#[tokio::test]
async fn take_back_on_a_parked_map_clears_the_name_only() {
    let (server, store) = server_with(|vault, _s| {
        seed_full_cap(vault);
        vault
            .create_project(moment(2026, 1, 3, 9, 0), "Old idea", Context::Work, None)
            .expect("created parked at the cap");
        seed_held(vault, "old-idea", "a colleague");
    });

    server
        .hand_off_project(hand_off("old-idea", None, true))
        .await
        .expect("a parked take-back needs no slot");

    let map = store
        .read_file(&vp("projects/_parked/old-idea.md"))
        .unwrap();
    assert!(map.contains("held_by: null\n"), "{map}");
    assert!(map.contains("status: parked"), "still parked:\n{map}");
    assert!(
        !store.exists(&vp("projects/old-idea.md")).unwrap(),
        "the map was not activated"
    );
}

/// `project_cap_reached` lists the counted maps only: a held active map is
/// in no position to free a slot, so listing it would send an agent to park
/// something that frees nothing (RFC 0006 §8.2). And nothing an agent reads
/// at the cap names handing off as a way to make room.
#[tokio::test]
async fn cap_rejection_lists_counted_maps_only_and_never_names_handing_off() {
    let (server, _store) = server_with(|vault, _s| {
        seed_full_cap(vault);
        seed_held(vault, "project-1", "the harbour office");
        let at = moment(2026, 1, 3, 9, 0);
        vault
            .create_project(at, "Glacier survey", Context::Work, None)
            .expect("created active");
        vault
            .create_project(at, "Old idea", Context::Work, None)
            .expect("created parked at the cap");
    });

    let err = server
        .activate_project(Parameters(ProjectSlugInput {
            project: "old-idea".to_owned(),
        }))
        .await
        .expect_err("activating an unheld map at the cap is refused");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "project_cap_reached", "{rejection}");

    let mut listed: Vec<String> = rejection["details"]["active_projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect();
    listed.sort();
    let mut counted: Vec<String> = (2..=default_cap())
        .map(|i| format!("project-{i}"))
        .chain(["glacier-survey".to_owned()])
        .collect();
    counted.sort();
    assert_eq!(listed, counted, "exactly the counted maps: {rejection}");
    assert_eq!(rejection["details"]["current"], default_cap());

    let message = err.message.to_lowercase();
    let details = rejection["details"].to_string().to_lowercase();
    for text in [&message, &details] {
        for banned in ["hand", "holder", "held"] {
            assert!(
                !text.contains(banned),
                "the cap rejection must not name handing off ({banned:?}): {text}"
            );
        }
    }
}

// --- focus on every write payload (RFC 0005 §5.5, #736) -----------------

/// The eighteen tools whose success payload carries `focus`.
const FOCUSED_WRITES: [&str; 18] = [
    "start_action",
    "start_unplanned_action",
    "switch_action",
    "switch_unplanned_action",
    "pause_action",
    "resume_action",
    "complete_action",
    "drop_action",
    "promote_action",
    "add_action",
    "append_to_log",
    "capture",
    "note_to_daily",
    "park_project",
    "activate_project",
    "complete_project",
    "drop_project",
    "hand_off_project",
];

/// A vault for the focus table, before anything is in focus:
/// `surrogate-model` with three open bullets and an open pause on
/// `Run ablation` (started then paused this morning), `side-quest` (active,
/// nothing open, so it can be parked, completed or dropped) and `old-idea`
/// (parked, so it can be activated). Today's daily note is written as the
/// domain writes it.
fn server_for_focus_table() -> (CuadernoServer, Arc<dyn VaultStore>) {
    server_with(|vault, store| {
        vault
            .create_project(
                moment(2026, 5, 1, 9, 0),
                "Surrogate model",
                Context::Work,
                None,
            )
            .unwrap();
        for (title, energy) in [
            ("Draft methods section", EnergyLevel::Deep),
            ("Run ablation", EnergyLevel::Deep),
            ("Write abstract", EnergyLevel::Light),
        ] {
            vault
                .add_action(moment(2026, 5, 1, 9, 5), "surrogate-model", title, energy)
                .unwrap();
        }
        vault
            .start_action(today_at(8, 0), "surrogate-model", "Run ablation")
            .unwrap();
        vault.pause_action(today_at(8, 30), None, None).unwrap();
        store
            .write_file(
                &vp("projects/side-quest.md"),
                "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-01-01\ncore_question: null\nclosed: null\n---\n\n# Side quest\n\n## Next Actions\n- [x] Ship (deep)\n",
            )
            .unwrap();
        store
            .write_file(
                &vp("projects/_parked/old-idea.md"),
                "---\ntype: project\ncontext: work\nstatus: parked\ncreated: 2026-01-01\ncore_question: null\nclosed: null\n---\n\n# Old idea\n\n## Next Actions\n",
            )
            .unwrap();
    })
}

/// Call one of [`FOCUSED_WRITES`] with arguments valid on
/// [`server_for_focus_table`].
async fn call_focused_write(
    server: &CuadernoServer,
    tool: &str,
) -> Result<CallToolResult, rmcp::model::ErrorData> {
    let project = || "surrogate-model".to_owned();
    match tool {
        "start_action" => {
            server
                .start_action(Parameters(StartActionInput {
                    project: project(),
                    query: "Write abstract".to_owned(),
                }))
                .await
        }
        "start_unplanned_action" => {
            server
                .start_unplanned_action(Parameters(StartUnplannedActionInput {
                    project: project(),
                    title: "Fix the CI badge".to_owned(),
                    energy: "light".to_owned(),
                }))
                .await
        }
        "switch_action" => {
            server
                .switch_action(Parameters(SwitchActionInput {
                    project: project(),
                    query: "Write abstract".to_owned(),
                    next: None,
                    reason: None,
                }))
                .await
        }
        "switch_unplanned_action" => {
            server
                .switch_unplanned_action(Parameters(SwitchUnplannedActionInput {
                    project: project(),
                    title: "Fix the CI badge".to_owned(),
                    energy: "light".to_owned(),
                    next: None,
                    reason: None,
                }))
                .await
        }
        "pause_action" => {
            server
                .pause_action(Parameters(PauseActionInput {
                    next: None,
                    reason: None,
                }))
                .await
        }
        "resume_action" => {
            server
                .resume_action(Parameters(ResumeActionInput { project: None }))
                .await
        }
        "complete_action" => {
            server
                .complete_action(Parameters(ActionQueryInput {
                    project: project(),
                    query: "Write abstract".to_owned(),
                }))
                .await
        }
        "drop_action" => {
            server
                .drop_action(Parameters(DropActionInput {
                    project: project(),
                    query: "Write abstract".to_owned(),
                    reason: None,
                }))
                .await
        }
        "promote_action" => {
            server
                .promote_action(Parameters(PromoteActionInput {
                    slug: None,
                    project: project(),
                    query: "Write abstract".to_owned(),
                    vars: None,
                }))
                .await
        }
        "add_action" => {
            server
                .add_action(Parameters(AddActionInput {
                    slug: None,
                    project: project(),
                    title: "Plot the loss curves".to_owned(),
                    energy: "medium".to_owned(),
                    with_note: false,
                    vars: None,
                }))
                .await
        }
        "append_to_log" => {
            server
                .append_to_log(Parameters(AppendToLogInput {
                    text: "read a related paper".to_owned(),
                }))
                .await
        }
        "capture" => {
            server
                .capture(Parameters(CaptureInput {
                    text: "look into mixed precision".to_owned(),
                }))
                .await
        }
        "note_to_daily" => {
            server
                .note_to_daily(Parameters(NoteToDailyInput {
                    date: None,
                    heading: "Loss scaling".to_owned(),
                    body: "Scale the loss before the backward pass.".to_owned(),
                }))
                .await
        }
        "park_project" => {
            server
                .park_project(Parameters(ProjectSlugInput {
                    project: "side-quest".to_owned(),
                }))
                .await
        }
        "activate_project" => {
            server
                .activate_project(Parameters(ProjectSlugInput {
                    project: "old-idea".to_owned(),
                }))
                .await
        }
        "complete_project" => {
            server
                .complete_project(Parameters(ProjectSlugInput {
                    project: "side-quest".to_owned(),
                }))
                .await
        }
        "drop_project" => {
            server
                .drop_project(Parameters(DropProjectInput {
                    project: "side-quest".to_owned(),
                    reason: None,
                    open_items: OpenItemsChoice::Refuse,
                    expected_open_items: None,
                }))
                .await
        }
        "hand_off_project" => {
            server
                .hand_off_project(Parameters(HandOffProjectInput {
                    project: "side-quest".to_owned(),
                    holder: Some("the harbour office".to_owned()),
                    take_back: false,
                }))
                .await
        }
        other => panic!("not a focused write: {other}"),
    }
}

/// What `current_focus` returns on `server` right now, as JSON.
async fn current_focus_json(server: &CuadernoServer) -> serde_json::Value {
    let result = server
        .current_focus(Parameters(cdno_mcp::server::EmptyInput {}))
        .await
        .expect("current_focus");
    decode_json(&result)
}

/// What a row of the focus table expects of its tool's result.
#[derive(Debug, Clone, Copy)]
enum FocusAfter {
    /// Success, and `focus` is exactly the focus the phase began with.
    Same,
    /// Success, and `focus` is null.
    Null,
    /// Success, and `focus` is a new one on this action text: the tool
    /// opens or moves the focus by construction.
    Opened(&'static str),
    /// Refused with this rejection code: there is no success payload.
    Refused(&'static str),
}

/// Run one row against a fresh copy of the phase's vault.
async fn check_focus_row(
    phase: &str,
    tool: &str,
    expect: FocusAfter,
    begin: &serde_json::Value,
    server: &CuadernoServer,
) {
    let outcome = call_focused_write(server, tool).await;
    let payload = match (expect, outcome) {
        (FocusAfter::Refused(code), Err(err)) => {
            assert_eq!(rejection_of(&err)["code"], code, "{phase}: {tool}");
            return;
        }
        (FocusAfter::Refused(code), Ok(r)) => {
            panic!("{phase}: {tool} should be refused with {code}, got {r:?}")
        }
        (_, Err(err)) => panic!("{phase}: {tool} failed: {err:?}"),
        (_, Ok(r)) => decode_json(&r),
    };
    let focus = payload
        .get("focus")
        .unwrap_or_else(|| panic!("{phase}: {tool} payload has no `focus`: {payload}"));
    // The invariant every row shares: the payload's focus is what
    // `current_focus` returns after the write.
    assert_eq!(
        focus,
        &current_focus_json(server).await,
        "{phase}: {tool}'s focus is not current_focus"
    );
    match expect {
        FocusAfter::Same => assert_eq!(focus, begin, "{phase}: {tool}"),
        FocusAfter::Null => assert!(focus.is_null(), "{phase}: {tool}: {focus}"),
        FocusAfter::Opened(action) => {
            assert_eq!(focus["action"], action, "{phase}: {tool}: {focus}");
            assert_eq!(focus["carried"], false, "{phase}: {tool}: {focus}");
        }
        FocusAfter::Refused(_) => unreachable!(),
    }
}

/// Every listed write carries `focus` (RFC 0005 §5.5), and it is the focus
/// `current_focus` reads after the write. Two phases, each row on a fresh
/// vault: after a `start_action`, and after a `complete_action` of that
/// start. A tool that opens, moves or closes the focus by construction
/// (start, switch, pause, resume) cannot return the phase's focus unchanged,
/// so its row names the focus it must return instead, or the rejection it
/// gets in that phase.
#[tokio::test]
async fn every_listed_write_carries_focus() {
    use FocusAfter::*;
    let draft = "Draft methods section (deep)";

    // Phase A: `Draft methods section` started through the tool.
    let after_start: [(&str, FocusAfter); 18] = [
        // A second start is refused while a focus is open; the phase's own
        // start is checked below.
        ("start_action", Refused("focus_open")),
        ("start_unplanned_action", Refused("focus_open")),
        ("switch_action", Opened("Write abstract (light)")),
        (
            "switch_unplanned_action",
            Opened("Fix the CI badge (light)"),
        ),
        ("pause_action", Null),
        // The open pause is on another action than the focus.
        ("resume_action", Refused("focus_open")),
        ("complete_action", Same),
        ("drop_action", Same),
        ("promote_action", Same),
        ("add_action", Same),
        ("append_to_log", Same),
        ("capture", Same),
        ("note_to_daily", Same),
        ("park_project", Same),
        ("activate_project", Same),
        ("complete_project", Same),
        ("drop_project", Same),
        // The focus is on another project, so the hand-off is not refused.
        ("hand_off_project", Same),
    ];
    // Phase B: that start then completed through the tool.
    let after_complete: [(&str, FocusAfter); 18] = [
        ("start_action", Opened("Write abstract (light)")),
        ("start_unplanned_action", Opened("Fix the CI badge (light)")),
        // With nothing open a switch is a plain start.
        ("switch_action", Opened("Write abstract (light)")),
        (
            "switch_unplanned_action",
            Opened("Fix the CI badge (light)"),
        ),
        ("pause_action", Refused("no_focus")),
        // The morning's pause on `Run ablation` is still open.
        ("resume_action", Opened("Run ablation (deep)")),
        ("complete_action", Null),
        ("drop_action", Null),
        ("promote_action", Null),
        ("add_action", Null),
        ("append_to_log", Null),
        ("capture", Null),
        ("note_to_daily", Null),
        ("park_project", Null),
        ("activate_project", Null),
        ("complete_project", Null),
        ("drop_project", Null),
        ("hand_off_project", Null),
    ];
    for table in [&after_start, &after_complete] {
        let tools: Vec<&str> = table.iter().map(|(t, _)| *t).collect();
        assert_eq!(tools, FOCUSED_WRITES, "every listed tool, once");
    }

    let start = |server: CuadernoServer| async move {
        let result = server
            .start_action(Parameters(StartActionInput {
                project: "surrogate-model".to_owned(),
                query: "Draft methods".to_owned(),
            }))
            .await
            .expect("start_action");
        (server, decode_json(&result))
    };

    for (tool, expect) in after_start {
        let (server, payload) = start(server_for_focus_table().0).await;
        let begin = payload["focus"].clone();
        assert_eq!(begin["action"], draft, "{payload}");
        assert_eq!(begin, current_focus_json(&server).await);
        check_focus_row("after start_action", tool, expect, &begin, &server).await;
    }

    for (tool, expect) in after_complete {
        let (server, _) = start(server_for_focus_table().0).await;
        let result = server
            .complete_action(Parameters(ActionQueryInput {
                project: "surrogate-model".to_owned(),
                query: "Draft methods".to_owned(),
            }))
            .await
            .expect("complete_action");
        let begin = decode_json(&result)["focus"].clone();
        assert!(begin.is_null(), "complete_action clears the focus: {begin}");
        check_focus_row("after complete_action", tool, expect, &begin, &server).await;
    }
}

/// A focus that cannot be read after a committed write is `focus: null`,
/// never a tool error (RFC 0005 §5.5). Yesterday's daily note is inside the
/// default focus window and is replaced, through the store, by one the
/// markdown parser rejects (no frontmatter): `current_focus` itself fails,
/// while writes to today's notes still land and report success.
#[tokio::test]
async fn a_failing_focus_read_yields_null_not_an_error() {
    let (server, store) = server_with_focus();
    let yesterday = chrono::Local::now().date_naive().pred_opt().unwrap();
    store
        .write_file(
            &vp(&cdno_core::paths::daily_note_relpath(yesterday)),
            "no frontmatter here\n\n## Logs\n",
        )
        .unwrap();
    // The read genuinely fails: this is not "nothing open".
    server
        .current_focus(Parameters(cdno_mcp::server::EmptyInput {}))
        .await
        .expect_err("the focus window holds a note that does not parse");

    let logged = server
        .append_to_log(Parameters(AppendToLogInput {
            text: "kept going regardless".to_owned(),
        }))
        .await
        .expect("a write that landed is not an error");
    let payload = decode_json(&logged);
    assert!(payload["focus"].is_null(), "{payload}");
    assert_eq!(payload["verification"]["verified"], "content", "{payload}");
    assert!(todays_daily(&store).contains("kept going regardless"));

    let captured = server
        .capture(Parameters(CaptureInput {
            text: "an idea for later".to_owned(),
        }))
        .await
        .expect("a write that landed is not an error");
    let payload = decode_json(&captured);
    assert!(payload["focus"].is_null(), "{payload}");
    assert!(
        store
            .exists(&vp(payload["path"].as_str().unwrap()))
            .unwrap()
    );
}

/// `note_to_daily` with a back-dated `date` writes to that day but reads the
/// focus on today's clock, so its `focus` is what `current_focus` returns, not
/// the focus as of the earlier day (which, outside the window, would be null).
#[tokio::test]
async fn note_to_daily_back_dated_still_reads_todays_focus() {
    let (server, _store) = server_with_focus();
    let three_days_ago = chrono::Local::now()
        .date_naive()
        .checked_sub_days(chrono::Days::new(3))
        .unwrap();
    let result = server
        .note_to_daily(Parameters(NoteToDailyInput {
            date: Some(three_days_ago),
            heading: "Loss scaling".to_owned(),
            body: "Scale the loss before the backward pass.".to_owned(),
        }))
        .await
        .expect("note_to_daily");
    let payload = decode_json(&result);
    assert!(
        payload["path"]
            .as_str()
            .unwrap()
            .contains(&three_days_ago.to_string()),
        "written to the back-dated day: {payload}"
    );
    let focus = &payload["focus"];
    assert!(!focus.is_null(), "{payload}");
    assert_eq!(focus["action"], "Draft methods section (deep)", "{payload}");
    assert_eq!(focus, &current_focus_json(&server).await);
}

// ---------------------------------------------------------------------
// RFC 0007 T4 (#853): `slug` on the create tools, and its rejection codes
// ---------------------------------------------------------------------

#[tokio::test]
async fn create_project_with_slug_names_the_file() {
    let (server, store) = server_with(|_v, s| seed_today_daily(&s));
    let result = server
        .create_project(Parameters(CreateProjectInput {
            title: "Calibrating the surrogate model against the reference solver".to_owned(),
            context: "work".to_owned(),
            core_question: None,
            vars: None,
            slug: Some("surrogate-calibration".to_owned()),
        }))
        .await
        .expect("create_project");
    assert_eq!(
        decode_json(&result)["path"],
        "projects/surrogate-calibration.md"
    );
    assert!(
        store
            .exists(&vp("projects/surrogate-calibration.md"))
            .unwrap()
    );
}

#[tokio::test]
async fn a_taken_slug_rejects_with_slug_taken() {
    let (server, _store) = server_with(|vault, s| {
        seed_today_daily(&s);
        vault
            .create_project(
                moment(2026, 1, 1, 9, 0),
                "Surrogate model",
                Context::Work,
                None,
            )
            .unwrap();
    });
    let err = server
        .create_project(Parameters(CreateProjectInput {
            title: "Another effort".to_owned(),
            context: "work".to_owned(),
            core_question: None,
            vars: None,
            slug: Some("surrogate-model".to_owned()),
        }))
        .await
        .expect_err("a taken slug is refused");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "slug_taken", "{rejection}");
    assert_eq!(rejection["details"]["slug"], "surrogate-model");
    assert_eq!(rejection["details"]["kind"], "stem", "{rejection}");
    assert_eq!(
        rejection["details"]["holder"], "projects/surrogate-model.md",
        "{rejection}"
    );
    assert!(
        rejection["message"]
            .as_str()
            .unwrap()
            .contains("do not suffix it yourself"),
        "{rejection}"
    );
}

#[tokio::test]
async fn an_invalid_slug_rejects_with_invalid_slug() {
    let (server, _store) = server_with(|_v, s| seed_today_daily(&s));
    let err = server
        .create_project(Parameters(CreateProjectInput {
            title: "Surrogate model".to_owned(),
            context: "work".to_owned(),
            core_question: None,
            vars: None,
            slug: Some("Surrogate Model".to_owned()),
        }))
        .await
        .expect_err("an invalid slug is refused");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "invalid_slug", "{rejection}");
    assert_eq!(rejection["details"]["slug"], "Surrogate Model");
    assert!(
        rejection["details"]["why"]
            .as_str()
            .is_some_and(|w| !w.is_empty()),
        "{rejection}"
    );
}

#[tokio::test]
async fn a_mismatched_portfolio_slug_rejects_with_portfolio_slug_mismatch() {
    let (server, _store) = server_with(|vault, s| {
        seed_today_daily(&s);
        vault
            .create_question(
                moment(2026, 1, 1, 9, 0),
                QuestionDomain::Research,
                "Surrogate calibration",
            )
            .unwrap();
    });
    let err = server
        .create_portfolio(Parameters(CreatePortfolioInput {
            question: "surrogate-calibration".to_owned(),
            project: None,
            vars: None,
            slug: Some("surrogate-evidence".to_owned()),
        }))
        .await
        .expect_err("a slug that is not the question's is refused");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "portfolio_slug_mismatch", "{rejection}");
    assert_eq!(
        rejection["details"],
        serde_json::json!({
            "question_slug": "surrogate-calibration",
            "slug": "surrogate-evidence",
        })
    );
}

#[tokio::test]
async fn a_slug_beside_a_slug_field_rejects_with_slug_given_twice() {
    let (server, _store) = server_with_config(config_with_person(), |_v, _s| {});
    let err = server
        .create_custom_note(Parameters(CreateCustomNoteInput {
            format: None,
            type_name: "person".to_owned(),
            title: "Ada Lovelace".to_owned(),
            fields: std::collections::HashMap::from([
                ("name".to_owned(), "Ada".to_owned()),
                ("slug".to_owned(), "ada".to_owned()),
            ]),
            vars: None,
            body: None,
            origin: None,
            slug: Some("ada-l".to_owned()),
        }))
        .await
        .expect_err("two slugs are refused");
    let rejection = rejection_of(&err);
    assert_eq!(rejection["code"], "slug_given_twice", "{rejection}");
    assert_eq!(rejection["details"]["field"], "slug", "{rejection}");
}

#[tokio::test]
async fn add_action_without_note_ignores_slug() {
    let (server, store) = server_with(|vault, s| {
        seed_today_daily(&s);
        vault
            .create_project(
                moment(2026, 1, 1, 9, 0),
                "Surrogate model",
                Context::Work,
                None,
            )
            .unwrap();
    });
    server
        .add_action(Parameters(AddActionInput {
            project: "surrogate-model".to_owned(),
            title: "Book the field hut".to_owned(),
            energy: "light".to_owned(),
            with_note: false,
            vars: None,
            slug: Some("hut".to_owned()),
        }))
        .await
        .expect("an inline bullet ignores the slug");
    let map = store.read_file(&vp("projects/surrogate-model.md")).unwrap();
    assert!(map.contains("- [ ] Book the field hut (light)"), "{map}");
    assert!(!store.exists(&vp("actions/hut.md")).unwrap());
}

/// The name a created path carries: its file stem, or its folder for a
/// folder note (`<slug>/_index.md`).
fn created_name(result: &CallToolResult) -> String {
    let path = decode_json(result)["path"].as_str().unwrap().to_owned();
    let p = std::path::Path::new(&path);
    let named = if p.file_stem().and_then(|s| s.to_str()) == Some("_index") {
        p.parent().and_then(|d| d.file_name())
    } else {
        p.file_stem()
    };
    named.and_then(|s| s.to_str()).unwrap().to_owned()
}

/// Every create tool hands the slug to the domain: a `None` put back at any
/// pass-through would leave the call succeeding under the derived name.
/// Names are unique across note types, so each row has its own slug.
#[tokio::test]
async fn every_create_tool_passes_the_slug_through() {
    let (server, _store) = server_with_config(config_with_person(), |vault, s| {
        seed_today_daily(&s);
        let at = moment(2026, 1, 1, 9, 0);
        vault
            .create_project(at, "Surrogate model", Context::Work, None)
            .unwrap();
        vault
            .add_action(
                at,
                "surrogate-model",
                "Fit the calibration curve",
                EnergyLevel::Medium,
            )
            .unwrap();
    });
    let s = |name: &str| Some(format!("surrogate-calibration-{name}"));
    let mut rows: Vec<(String, CallToolResult)> = Vec::new();

    rows.push((
        "surrogate-calibration-project".into(),
        server
            .create_project(Parameters(CreateProjectInput {
                title: "Calibrating the surrogate against the solver".to_owned(),
                context: "work".to_owned(),
                core_question: None,
                vars: None,
                slug: s("project"),
            }))
            .await
            .expect("create_project"),
    ));
    rows.push((
        "surrogate-calibration-question".into(),
        server
            .create_question(Parameters(CreateQuestionInput {
                domain: "research".to_owned(),
                text: "How well does the surrogate extrapolate beyond its training range?"
                    .to_owned(),
                vars: None,
                slug: s("question"),
            }))
            .await
            .expect("create_question"),
    ));
    rows.push((
        "surrogate-calibration-portfolio".into(),
        server
            .create_portfolio(Parameters(CreatePortfolioInput {
                question: "Evidence on how the surrogate extrapolates".to_owned(),
                project: None,
                vars: None,
                slug: s("portfolio"),
            }))
            .await
            .expect("create_portfolio"),
    ));
    rows.push((
        "surrogate-calibration-note".into(),
        server
            .create_custom_note(Parameters(CreateCustomNoteInput {
                format: None,
                type_name: "person".to_owned(),
                title: "Ada Lovelace".to_owned(),
                fields: std::collections::HashMap::from([("name".to_owned(), "Ada".to_owned())]),
                vars: None,
                body: None,
                origin: None,
                slug: s("note"),
            }))
            .await
            .expect("create_custom_note"),
    ));
    for (expanded, name) in [(false, "flat"), (true, "expanded")] {
        rows.push((
            format!("surrogate-calibration-{name}"),
            server
                .create_stewardship(Parameters(CreateStewardshipInput {
                    name: "Keeping the surrogate honest".to_owned(),
                    context: "work".to_owned(),
                    expanded,
                    vars: None,
                    slug: s(name),
                }))
                .await
                .expect("create_stewardship"),
        ));
    }
    rows.push((
        "surrogate-calibration-action".into(),
        server
            .add_action(Parameters(AddActionInput {
                project: "surrogate-model".to_owned(),
                title: "Plot the residuals".to_owned(),
                energy: "light".to_owned(),
                with_note: true,
                vars: None,
                slug: s("action"),
            }))
            .await
            .expect("add_action"),
    ));
    rows.push((
        "surrogate-calibration-promoted".into(),
        server
            .promote_action(Parameters(PromoteActionInput {
                project: "surrogate-model".to_owned(),
                query: "Fit the calibration".to_owned(),
                vars: None,
                slug: s("promoted"),
            }))
            .await
            .expect("promote_action"),
    ));
    rows.push((
        "surrogate-calibration-commitment".into(),
        server
            .create_commitment(Parameters(CreateCommitmentInput {
                title: "Send the surrogate report".to_owned(),
                due: NaiveDate::from_ymd_opt(2026, 12, 1).unwrap(),
                context: "work".to_owned(),
                project: None,
                stewardship: None,
                vars: None,
                slug: s("commitment"),
            }))
            .await
            .expect("create_commitment"),
    ));

    for (want, result) in &rows {
        assert_eq!(&created_name(result), want, "{result:?}");
    }
}

// ---------------------------------------------------------------------
// The rename's freshness policy (RFC 0007 §5.8 (2), T25): the helper the
// `rename_note` tool (T26) calls, over constructed `GitState` values, and
// its guard on a real repository.
// ---------------------------------------------------------------------

mod freshness {
    use std::cell::RefCell;
    use std::path::Path;

    use cdno_core::git::{GitError, GitState, PausedGitOp, Upstream};
    use cdno_domain::error::DomainError;
    use cdno_mcp::freshness::{Assessment, assess, guard};

    fn repo(paused: Result<Option<PausedGitOp>, GitError>, upstream: Option<Upstream>) -> GitState {
        GitState::Repository { paused, upstream }
    }

    fn merge() -> PausedGitOp {
        PausedGitOp {
            reason: "a merge is in progress (.git/MERGE_HEAD present)".into(),
            marker: Some("MERGE_HEAD"),
        }
    }

    fn probe_failed() -> GitError {
        GitError::TimedOut {
            command: "git diff --name-only --diff-filter=U".into(),
        }
    }

    fn up(ahead: u32, behind: u32) -> Option<Upstream> {
        Some(Upstream { ahead, behind })
    }

    fn proceeds(a: &Assessment) -> bool {
        a.refusal.is_none()
    }

    #[test]
    fn not_a_repository_skips_the_check() {
        for skip in [false, true] {
            let a = assess(&GitState::NotARepository, skip);
            assert!(proceeds(&a) && a.warnings.is_empty(), "{a:?}");
        }
    }

    #[test]
    fn an_unknown_state_warns_and_proceeds() {
        let state = GitState::Unknown(GitError::Exit {
            command: "git rev-parse --is-inside-work-tree".into(),
            stderr: "fatal: detected dubious ownership\nTo add an exception ...".into(),
        });
        let a = assess(&state, false);
        assert!(proceeds(&a), "{a:?}");
        assert_eq!(a.warnings.len(), 1);
        assert!(
            a.warnings[0].contains("dubious ownership") && !a.warnings[0].contains('\n'),
            "{:?}",
            a.warnings
        );
    }

    #[test]
    fn up_to_date_and_ahead_proceed_quietly() {
        for upstream in [up(0, 0), up(3, 0)] {
            let a = assess(&repo(Ok(None), upstream), false);
            assert!(proceeds(&a) && a.warnings.is_empty(), "{a:?}");
        }
    }

    #[test]
    fn a_paused_operation_refuses_even_with_skip_sync_check() {
        for skip in [false, true] {
            for upstream in [up(0, 0), up(0, 2), None] {
                let a = assess(&repo(Ok(Some(merge())), upstream), skip);
                assert!(
                    matches!(&a.refusal, Some(DomainError::GitOperationInProgress { reason })
                        if reason.contains("MERGE_HEAD")),
                    "skip {skip}, {upstream:?}: {a:?}"
                );
            }
        }
    }

    #[test]
    fn a_failed_paused_probe_warns_and_proceeds() {
        let a = assess(&repo(Err(probe_failed()), up(0, 0)), false);
        assert!(proceeds(&a), "{a:?}");
        assert_eq!(a.warnings.len(), 1, "{:?}", a.warnings);
        assert!(a.warnings[0].contains("paused"));
    }

    #[test]
    fn a_failed_paused_probe_keeps_its_warning_beside_a_refusal() {
        let a = assess(&repo(Err(probe_failed()), up(0, 1)), false);
        assert!(matches!(
            a.refusal,
            Some(DomainError::BehindUpstream { behind: 1 })
        ));
        assert_eq!(a.warnings.len(), 1, "{:?}", a.warnings);
    }

    #[test]
    fn behind_refuses_unless_skipped() {
        let a = assess(&repo(Ok(None), up(1, 2)), false);
        let err = a.refusal.expect("refused");
        assert!(matches!(err, DomainError::BehindUpstream { behind: 2 }));
        assert_eq!(
            err.to_string(),
            "the vault is 2 commits behind its upstream"
        );

        let a = assess(&repo(Ok(None), up(1, 2)), true);
        assert!(proceeds(&a), "{a:?}");
        assert_eq!(a.warnings.len(), 1, "{:?}", a.warnings);
        assert!(a.warnings[0].contains("behind"));
    }

    #[test]
    fn no_upstream_warns_and_proceeds() {
        for skip in [false, true] {
            let a = assess(&repo(Ok(None), None), skip);
            assert!(proceeds(&a), "{a:?}");
            assert_eq!(a.warnings.len(), 1, "{:?}", a.warnings);
            assert!(a.warnings[0].contains("no upstream"));
        }
    }

    /// `git init` in `dir` with nothing from the developer's config.
    fn git_init(dir: &Path) -> bool {
        std::process::Command::new("git")
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["init", "-q"])
            .status()
            .is_ok_and(|s| s.success())
    }

    #[test]
    fn the_guard_refuses_a_paused_operation_under_the_lock() {
        // The guard's git inherits `HOME` / `XDG_CONFIG_HOME` (cdno's git
        // clears every other variable), so the body runs in a re-exec of
        // this test with both pointed at a temp dir: the developer's git
        // config never reaches it, and no other test's environment changes.
        const REPO: &str = "CDNO_FRESHNESS_GUARD_REPO";
        if let Some(repo) = std::env::var_os(REPO) {
            return guard_on_a_paused_repository(Path::new(&repo));
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("vault");
        std::fs::create_dir_all(&repo).unwrap();
        if !git_init(&repo) {
            eprintln!("skipped: `git` is not on PATH");
            return;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "freshness::the_guard_refuses_a_paused_operation_under_the_lock",
                "--exact",
                "--test-threads=1",
            ])
            .env(REPO, &repo)
            .env("HOME", tmp.path())
            .env("XDG_CONFIG_HOME", tmp.path())
            .output()
            .expect("re-run this test");
        assert!(
            out.status.success() && String::from_utf8_lossy(&out.stdout).contains("1 passed"),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn guard_on_a_paused_repository(dir: &Path) {
        // A clean repository with no upstream: the guard proceeds, and its
        // warning is reported once, not again when the first check had it.
        let fresh = RefCell::new(Vec::new());
        assert!(guard(dir, false, &[], &fresh)().is_ok());
        let first = fresh.into_inner();
        assert_eq!(first.len(), 1, "{first:?}");
        let fresh = RefCell::new(Vec::new());
        assert!(guard(dir, false, &first, &fresh)().is_ok());
        assert!(fresh.into_inner().is_empty());

        // A merge stopped mid-way: refused, with or without the override.
        std::fs::write(
            dir.join(".git/MERGE_HEAD"),
            "0123456789012345678901234567890123456789\n",
        )
        .unwrap();
        for skip in [false, true] {
            let fresh = RefCell::new(Vec::new());
            let got = guard(dir, skip, &[], &fresh)();
            assert!(
                matches!(&got, Err(DomainError::GitOperationInProgress { reason })
                    if reason.contains("MERGE_HEAD")),
                "skip {skip}: {got:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------
// rename_note (RFC 0007 §6.4, T26)
// ---------------------------------------------------------------------

#[path = "support/flaky_index.rs"]
mod flaky_index;

mod rename {
    use super::*;

    use std::sync::Arc;

    use cdno_core::index::MemoryIndex;
    use serde_json::json;

    use crate::flaky_index::FlakyIndex;

    const PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nCalibrating.\n\n## Next Actions\n- [ ] [[actions/fit-the-baseline]] (deep)\n\n## Milestones\n- [ ] Baseline fitted \u{2014} hard: 2026-05-20\n";
    const ACTION: &str = "---\ntype: action\nstatus: active\nproject: surrogate-model\nenergy: deep\ncreated: 2026-05-01\n---\n\n# Fit the baseline\n\nSee [[surrogate-model]].\n";
    const OLD: &str = "projects/surrogate-model.md";
    const NEW: &str = "projects/emulator-study.md";

    fn seeded_store() -> Arc<dyn VaultStore> {
        let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
        store.write_file(&vp(OLD), PROJECT).unwrap();
        store
            .write_file(&vp("actions/fit-the-baseline.md"), ACTION)
            .unwrap();
        store
    }

    fn server_at(
        store: &Arc<dyn VaultStore>,
        index: Arc<dyn VaultIndex>,
        root: std::path::PathBuf,
    ) -> CuadernoServer {
        let (vault, _r) =
            Vault::new(Arc::clone(store), index, VaultConfig::default()).expect("vault opens");
        CuadernoServer::new(Arc::new(vault), root)
    }

    fn server_over(store: &Arc<dyn VaultStore>, index: Arc<dyn VaultIndex>) -> CuadernoServer {
        server_at(store, index, scratch_root())
    }

    fn server() -> (CuadernoServer, Arc<dyn VaultStore>) {
        let store = seeded_store();
        let server = server_over(&store, Arc::new(MemoryIndex::new()));
        (server, store)
    }

    fn input(note: &str, to: &str) -> RenameNoteInput {
        RenameNoteInput {
            note: note.to_owned(),
            to: Some(to.to_owned()),
            relink: None,
            reason: Some("the model became an emulator study".to_owned()),
            dry_run: None,
            expected_plan: None,
            skip_sync_check: None,
        }
    }

    async fn preview(server: &CuadernoServer) -> serde_json::Value {
        let result = server
            .rename_note(Parameters(RenameNoteInput {
                dry_run: Some(true),
                ..input("surrogate-model", "emulator-study")
            }))
            .await
            .expect("a dry run");
        decode_json(&result)
    }

    #[tokio::test]
    async fn rename_note_dry_run_returns_the_plan_and_writes_nothing() {
        let (server, store) = server();
        let before = store.walk_dir(&VaultPath::root()).unwrap();
        let plan = preview(&server).await;

        assert_eq!(plan["dry_run"], json!(true));
        assert_eq!(plan["written"], json!(false));
        assert_eq!(plan["index_stale"], json!(false));
        assert_eq!(plan["target"], OLD);
        assert_eq!(plan["from"], "surrogate-model");
        assert_eq!(plan["new_slug"], "emulator-study");
        assert_eq!(plan["found_by_former_name"], json!(null));
        assert_eq!(plan["moves"], json!([{ "from": OLD, "to": NEW }]));
        assert!(plan["plan_hash"].as_str().is_some_and(|h| !h.is_empty()));
        assert!(
            plan["log_line"]
                .as_str()
                .is_some_and(|l| l.contains("renamed")),
            "{plan}"
        );
        assert_eq!(plan["verification"], json!(null));
        let edits = plan["edits"].as_array().unwrap();
        let action = edits
            .iter()
            .find(|e| e["path"] == "actions/fit-the-baseline.md")
            .unwrap_or_else(|| panic!("the referrer is planned: {plan}"));
        assert!(
            action["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["kind"] == "links" && c["count"] == 1),
            "{action}"
        );
        assert!(
            action["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["kind"] == "field" && c["key"] == "project"),
            "{action}"
        );
        // Nothing moved, nothing created.
        assert_eq!(store.walk_dir(&VaultPath::root()).unwrap(), before);
        assert!(store.exists(&vp(OLD)).unwrap());
        assert!(!store.exists(&vp(NEW)).unwrap());
    }

    #[tokio::test]
    async fn rename_note_with_the_previewed_hash_writes() {
        let (server, store) = server();
        let plan = preview(&server).await;
        let result = server
            .rename_note(Parameters(RenameNoteInput {
                expected_plan: Some(plan["plan_hash"].as_str().unwrap().to_owned()),
                ..input("surrogate-model", "emulator-study")
            }))
            .await
            .expect("the previewed plan is written");
        let written = decode_json(&result);

        assert_eq!(written["written"], json!(true));
        assert_eq!(written["dry_run"], json!(false));
        assert_eq!(written["plan_hash"], plan["plan_hash"]);
        assert_eq!(written["verification"]["verified"], "content");
        assert!(!store.exists(&vp(OLD)).unwrap());
        assert!(store.exists(&vp(NEW)).unwrap());
        let action = store.read_file(&vp("actions/fit-the-baseline.md")).unwrap();
        assert!(
            action.contains("project: emulator-study") && action.contains("[[emulator-study]]"),
            "{action}"
        );
        let daily = today_daily_text(&store);
        assert!(daily.contains("renamed"), "{daily}");
    }

    #[tokio::test]
    async fn rename_note_with_a_stale_hash_is_stale_plan() {
        let (server, store) = server();
        let plan = preview(&server).await;
        let err = server
            .rename_note(Parameters(RenameNoteInput {
                expected_plan: Some("0000stale".to_owned()),
                ..input("surrogate-model", "emulator-study")
            }))
            .await
            .expect_err("a plan that moved is refused");
        let rejection = rejection_of(&err);
        assert_eq!(rejection["code"], "stale_plan");
        assert_eq!(rejection["details"]["expected"], "0000stale");
        assert_eq!(rejection["details"]["actual"], plan["plan_hash"]);
        assert!(
            rejection["message"]
                .as_str()
                .unwrap()
                .contains("Do not retry"),
            "{rejection}"
        );
        assert!(store.exists(&vp(OLD)).unwrap(), "nothing was written");
        assert!(!store.exists(&vp(NEW)).unwrap());
    }

    #[tokio::test]
    async fn rename_note_index_stale_is_success() {
        let store = seeded_store();
        let flaky = Arc::new(FlakyIndex::new(Arc::new(MemoryIndex::new())));
        let index: Arc<dyn VaultIndex> = flaky.clone();
        let server = server_over(&store, index);
        let plan = preview(&server).await;
        flaky.arm();
        let result = server
            .rename_note(Parameters(RenameNoteInput {
                expected_plan: Some(plan["plan_hash"].as_str().unwrap().to_owned()),
                ..input("surrogate-model", "emulator-study")
            }))
            .await
            .expect("the files landed, so the rename succeeded");
        let written = decode_json(&result);
        assert_eq!(written["index_stale"], json!(true), "{written}");
        assert_eq!(written["written"], json!(true));
        assert!(store.exists(&vp(NEW)).unwrap());
        assert!(!store.exists(&vp(OLD)).unwrap());
    }

    #[tokio::test]
    async fn rename_note_reports_a_former_name_and_read_note_does_too() {
        let (server, _store) = server();
        let plan = preview(&server).await;
        server
            .rename_note(Parameters(RenameNoteInput {
                expected_plan: Some(plan["plan_hash"].as_str().unwrap().to_owned()),
                ..input("surrogate-model", "emulator-study")
            }))
            .await
            .expect("renamed");

        let read = server
            .read_note(Parameters(ReadNoteInput {
                note: "surrogate-model".to_owned(),
            }))
            .await
            .expect("the old name resolves");
        let read = decode_json(&read);
        assert_eq!(read["path"], NEW);
        assert_eq!(read["found_by_former_name"], "surrogate-model");
        let current = server
            .read_note(Parameters(ReadNoteInput {
                note: "emulator-study".to_owned(),
            }))
            .await
            .unwrap();
        assert!(decode_json(&current).get("found_by_former_name").is_none());

        // And the rename tool says so when it is given the former name.
        let again = server
            .rename_note(Parameters(RenameNoteInput {
                dry_run: Some(true),
                ..input("surrogate-model", "emulator-notes")
            }))
            .await
            .expect("a dry run by the former name");
        let again = decode_json(&again);
        assert_eq!(again["found_by_former_name"], "surrogate-model");
        assert_eq!(again["from"], "emulator-study");
    }

    #[tokio::test]
    async fn rename_note_of_an_unknown_note_is_not_found() {
        let (server, _store) = server();
        let err = server
            .rename_note(Parameters(input("no-such-note", "emulator-study")))
            .await
            .expect_err("nothing to rename");
        assert_eq!(rejection_code(&err), "not_found");
    }

    #[tokio::test]
    async fn rename_note_to_a_taken_slug_is_refused_not_suffixed() {
        let (server, store) = server();
        let err = server
            .rename_note(Parameters(input("surrogate-model", "fit-the-baseline")))
            .await
            .expect_err("the stem is taken");
        assert_eq!(rejection_code(&err), "slug_taken");
        assert!(store.exists(&vp(OLD)).unwrap());
    }

    #[tokio::test]
    async fn rename_note_relink_rewrites_stragglers_and_moves_nothing() {
        let (server, store) = server();
        let plan = preview(&server).await;
        server
            .rename_note(Parameters(RenameNoteInput {
                expected_plan: Some(plan["plan_hash"].as_str().unwrap().to_owned()),
                ..input("surrogate-model", "emulator-study")
            }))
            .await
            .expect("renamed");
        // A straggler written after the rename, as a synced clone brings
        // one back: a link and a field still under the old name.
        let straggler = "actions/wire-the-emulator.md";
        store
            .write_file(
                &vp(straggler),
                "---\ntype: action\nstatus: active\nproject: surrogate-model\nenergy: deep\ncreated: 2026-05-02\n---\n\n# Wire the emulator\n\nSee [[surrogate-model]].\n",
            )
            .unwrap();
        let relink = || RenameNoteInput {
            note: "emulator-study".to_owned(),
            to: None,
            relink: Some(true),
            reason: None,
            dry_run: Some(true),
            expected_plan: None,
            skip_sync_check: None,
        };

        let dry = decode_json(
            &server
                .rename_note(Parameters(relink()))
                .await
                .expect("a dry run"),
        );
        assert_eq!(dry["moves"], json!([]));
        assert_eq!(dry["from"], "emulator-study");
        assert_eq!(dry["new_slug"], "emulator-study");
        assert_eq!(dry["references"], 2);
        assert_eq!(dry["notes"], 1);
        assert!(
            dry["log_line"]
                .as_str()
                .is_some_and(|l| l.starts_with("relinked [[emulator-study]]")
                    && l.ends_with("\n  references: 2")),
            "{dry}"
        );
        assert!(
            store
                .read_file(&vp(straggler))
                .unwrap()
                .contains("[[surrogate-model]]")
        );

        let written = decode_json(
            &server
                .rename_note(Parameters(RenameNoteInput {
                    dry_run: None,
                    expected_plan: Some(dry["plan_hash"].as_str().unwrap().to_owned()),
                    ..relink()
                }))
                .await
                .expect("the previewed relink is written"),
        );
        assert_eq!(written["written"], json!(true));
        assert_eq!(written["verification"]["verified"], "content");
        let action = store.read_file(&vp(straggler)).unwrap();
        assert!(
            action.contains("project: emulator-study") && action.contains("[[emulator-study]]"),
            "{action}"
        );
        assert!(store.exists(&vp(NEW)).unwrap() && !store.exists(&vp(OLD)).unwrap());
        assert!(today_daily_text(&store).contains("relinked [[emulator-study]]"));

        // `relink` is exclusive with `to` (and takes no `reason`); one of
        // the two must be given.
        for (bad, field) in [
            (
                RenameNoteInput {
                    to: Some("emulator".to_owned()),
                    ..relink()
                },
                "to",
            ),
            (
                RenameNoteInput {
                    reason: Some("tidy".to_owned()),
                    ..relink()
                },
                "reason",
            ),
            (
                RenameNoteInput {
                    relink: None,
                    ..relink()
                },
                "to",
            ),
        ] {
            let err = server
                .rename_note(Parameters(bad))
                .await
                .expect_err("an argument error");
            assert_eq!(err.code, ErrorCode::INVALID_PARAMS, "{err:?}");
            assert!(
                err.message.contains(&format!("'{field}'")),
                "{}",
                err.message
            );
        }

        // A blank `to` beside `relink: true` reads as omitted.
        let blank = decode_json(
            &server
                .rename_note(Parameters(RenameNoteInput {
                    to: Some("  ".to_owned()),
                    ..relink()
                }))
                .await
                .expect("a blank `to` with `relink` is a relink"),
        );
        assert_eq!(blank["moves"], json!([]));
        assert_eq!(blank["new_slug"], "emulator-study");
    }

    #[tokio::test]
    async fn a_blank_expected_plan_is_refused_not_written_past() {
        let (server, store) = server();
        let err = server
            .rename_note(Parameters(RenameNoteInput {
                expected_plan: Some("  ".to_owned()),
                ..input("surrogate-model", "emulator-study")
            }))
            .await
            .expect_err("a blank hash is not an omitted one");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS, "{err:?}");
        assert!(err.message.contains("expected_plan"), "{}", err.message);
        assert!(store.exists(&vp(OLD)).unwrap(), "nothing was written");
    }

    #[tokio::test]
    async fn a_different_reason_makes_the_plan_stale() {
        let (server, store) = server();
        let plan = preview(&server).await;
        let err = server
            .rename_note(Parameters(RenameNoteInput {
                reason: None,
                expected_plan: Some(plan["plan_hash"].as_str().unwrap().to_owned()),
                ..input("surrogate-model", "emulator-study")
            }))
            .await
            .expect_err("the reason is part of the plan");
        let rejection = rejection_of(&err);
        assert_eq!(rejection["code"], "stale_plan");
        let message = rejection["message"].as_str().unwrap();
        assert!(message.contains("`reason`"), "{message}");
        assert_eq!(message.matches("preview it again").count(), 1, "{message}");
        assert!(store.exists(&vp(OLD)).unwrap());
    }

    /// A question and its same-slug portfolio share a stem; the portfolio is
    /// renamed through the question, so the bare slug is the question, as in
    /// the CLI, rather than an `ambiguous_slug`.
    #[tokio::test]
    async fn a_question_and_its_portfolio_resolve_to_the_question_over_mcp() {
        let (server, _store) = server();
        server
            .create_question(Parameters(CreateQuestionInput {
                domain: "research".to_owned(),
                text: "Does a surrogate beat the simulator?".to_owned(),
                vars: None,
                slug: Some("surrogate-accuracy".to_owned()),
            }))
            .await
            .expect("question");
        server
            .create_portfolio(Parameters(CreatePortfolioInput {
                question: "surrogate-accuracy".to_owned(),
                project: None,
                vars: None,
                slug: None,
            }))
            .await
            .expect("portfolio");
        let result = server
            .rename_note(Parameters(RenameNoteInput {
                dry_run: Some(true),
                ..input("surrogate-accuracy", "surrogate-error")
            }))
            .await
            .expect("the question is the only renamable hit");
        let plan = decode_json(&result);
        assert_eq!(plan["target"], "questions/research/surrogate-accuracy.md");
        assert!(
            plan["moves"][0]["to"] == "questions/research/surrogate-error.md",
            "{plan}"
        );
    }

    /// The freshness check's warnings are part of the tool's answer (an MCP
    /// client has no stderr), on a preview and a write alike. A git vault
    /// with no upstream warns and proceeds.
    #[tokio::test]
    async fn sync_check_warnings_reach_the_tool_result() {
        let dir = tempfile::TempDir::new().unwrap();
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["init", "-q"])
            .output()
            .expect("git");
        assert!(out.status.success(), "{out:?}");
        let store = seeded_store();
        let server = server_at(
            &store,
            Arc::new(MemoryIndex::new()),
            dir.path().to_path_buf(),
        );

        let plan = server
            .rename_note(Parameters(RenameNoteInput {
                dry_run: Some(true),
                ..input("surrogate-model", "emulator-study")
            }))
            .await
            .expect("a dry run");
        let plan = decode_json(&plan);
        let sync = |payload: &serde_json::Value| -> Vec<String> {
            payload["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|w| w["kind"] == "sync_check")
                .map(|w| w["message"].as_str().unwrap().to_owned())
                .collect()
        };
        let shown = sync(&plan);
        assert_eq!(shown.len(), 1, "{plan}");
        assert!(shown[0].starts_with("no upstream"), "{shown:?}");

        let written = server
            .rename_note(Parameters(RenameNoteInput {
                expected_plan: Some(plan["plan_hash"].as_str().unwrap().to_owned()),
                ..input("surrogate-model", "emulator-study")
            }))
            .await
            .expect("a write");
        let written = decode_json(&written);
        assert_eq!(written["written"], json!(true));
        // The first check's warning and the guard's are the same sentence:
        // reported once, not twice.
        assert_eq!(sync(&written), shown, "{written}");
    }

    fn today_daily_text(store: &Arc<dyn VaultStore>) -> String {
        let today = chrono::Local::now().date_naive();
        store
            .read_file(&vp(&cdno_core::paths::daily_note_relpath(today)))
            .unwrap_or_default()
    }

    /// A git repository whose merge is left paused: the refusal the tool
    /// makes before it plans, for a preview as for a write.
    #[tokio::test]
    async fn a_paused_git_operation_refuses_a_preview_and_a_write_alike() {
        let dir = tempfile::TempDir::new().unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .expect("git");
            assert!(out.status.success(), "{args:?}: {out:?}");
        };
        run(&["init", "-q"]);
        std::fs::write(dir.path().join(".git/MERGE_HEAD"), "0".repeat(40) + "\n").unwrap();
        let store = seeded_store();
        let server = server_at(
            &store,
            Arc::new(MemoryIndex::new()),
            dir.path().to_path_buf(),
        );

        for dry_run in [true, false] {
            for skip in [false, true] {
                let err = server
                    .rename_note(Parameters(RenameNoteInput {
                        dry_run: Some(dry_run),
                        skip_sync_check: Some(skip),
                        ..input("surrogate-model", "emulator-study")
                    }))
                    .await
                    .expect_err("a paused merge is never overridable");
                let rejection = rejection_of(&err);
                assert_eq!(
                    rejection["code"], "git_operation_in_progress",
                    "dry_run {dry_run}, skip {skip}: {rejection}"
                );
                assert!(
                    rejection["details"]["reason"]
                        .as_str()
                        .unwrap()
                        .contains("MERGE_HEAD"),
                    "{rejection}"
                );
            }
        }
        assert!(store.exists(&vp(OLD)).unwrap(), "nothing was written");
    }
}

/// A fresh empty directory as the server's vault root: the in-memory vault
/// has no folder of its own, and the root must not be a place git could find
/// a repository above (a developer's temp dir may sit inside one).
fn scratch_root() -> std::path::PathBuf {
    tempfile::TempDir::new().expect("tempdir").keep()
}

#[tokio::test]
async fn create_custom_note_format_typst() {
    use cdno_core::config::CustomNoteType;
    let mut config = VaultConfig::default();
    config.note_types.insert(
        "concept".to_owned(),
        CustomNoteType {
            folder: "concepts".to_owned(),
            required: vec!["created".to_owned()],
            optional: vec!["origin".to_owned()],
            template: None,
            append_only: false,
            title_field: None,
            date_field: None,
        },
    );
    let (server, store) = server_with_config(config, |_v, _s| {});
    let input = |format: Option<&str>, vars: Option<std::collections::HashMap<String, String>>| {
        CreateCustomNoteInput {
            format: format.map(str::to_owned),
            slug: None,
            type_name: "concept".to_owned(),
            title: "Bias-variance split".to_owned(),
            fields: std::collections::HashMap::new(),
            vars,
            body: Some("The error splits as $ sigma^2 $.".to_owned()),
            origin: Some("[[projects/surrogate-model]]".to_owned()),
        }
    };

    server
        .create_custom_note(Parameters(input(Some("typst"), None)))
        .await
        .expect("create_custom_note");
    let content = store
        .read_file(&vp("concepts/bias-variance-split.typ"))
        .unwrap();
    assert!(
        content.contains("origin: \"[[projects/surrogate-model]]\""),
        "{content}"
    );
    assert!(
        content.ends_with("<cdno>\n\nThe error splits as $ sigma^2 $.\n"),
        "{content}"
    );

    let vars = std::collections::HashMap::from([("x".to_owned(), "y".to_owned())]);
    server
        .create_custom_note(Parameters(input(Some("typst"), Some(vars))))
        .await
        .expect_err("vars with typst are refused");
    server
        .create_custom_note(Parameters(input(Some("latex"), None)))
        .await
        .expect_err("an unknown format is refused");
}
