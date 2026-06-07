//! In-process tests for the 9 operation handlers implemented in #47.
//!
//! Same pattern as `handlers_context.rs`: call the handler methods
//! directly on `CuadernoServer` (they're `pub async fn`) with
//! `Parameters(input)`, decode the JSON payload of the returned
//! `CallToolResult`, and assert on shape + side effects.
//!
//! Operation handlers all return a `WriteResultDto { path, message }`
//! and have a side effect on the vault — we assert both.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::{Context, EnergyLevel};
use cdno_mcp::CuadernoServer;
use cdno_mcp::server::{
    ActionQueryInput, AddActionInput, AppendToLogInput, CompleteCommitmentInput,
    CreateCommitmentInput, CreatePortfolioInput, CreateProjectInput, CreateQuestionInput,
    CreateStewardshipInput, CreateTrackingEntryInput, FileToPortfolioInput, ReadDailyNoteInput,
    UpdateProjectStateInput, UpsertDailySectionInput,
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
    (CuadernoServer::new(Arc::new(vault)), store)
}

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
                moment(2026, 5, 1, 9, 0).date(),
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

// ---------------------------------------------------------------------
// add_action / promote_action / complete_action
// ---------------------------------------------------------------------

fn seed_active_project(vault: &Vault) {
    vault
        .create_project(
            moment(2026, 5, 1, 9, 0).date(),
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
            project: "surrogate-model".to_owned(),
            title: "Run sweep B".to_owned(),
            energy: "deep".to_owned(),
            with_note: false,
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
            project: "surrogate-model".to_owned(),
            title: "Investigate basis stability".to_owned(),
            energy: "deep".to_owned(),
            with_note: true,
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
            project: "surrogate-model".to_owned(),
            title: "x".to_owned(),
            energy: "intense".to_owned(),
            with_note: false,
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
        .promote_action(Parameters(ActionQueryInput {
            project: "surrogate-model".to_owned(),
            query: "sweep B".to_owned(),
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
            title: "Renew passport".to_owned(),
            due: NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
            context: "personal".to_owned(),
            project: None,
            stewardship: None,
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
async fn create_commitment_rejects_unknown_context_with_invalid_params() {
    let (server, _store) = server_with(|_v, store| seed_today_daily(&store));
    let err = server
        .create_commitment(Parameters(CreateCommitmentInput {
            title: "x".to_owned(),
            due: NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
            context: "fortnightly".to_owned(),
            project: None,
            stewardship: None,
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
            stewardship: "health".to_owned(),
            activity: "gym".to_owned(),
            routine: Some("upper-body-a".to_owned()),
            content: "Felt strong.".to_owned(),
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
    assert!(body.contains("routine: \"[[stewardships/health/routines/upper-body-a]]\""));
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
            stewardship: "finances".to_owned(),
            activity: "gym".to_owned(),
            routine: None,
            content: String::new(),
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
        }))
        .await
        .expect_err("Logs is append-only and not on the allowlist");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("section"));
}

// ---------------------------------------------------------------------
// Structural creation (GH #162)
// ---------------------------------------------------------------------

#[tokio::test]
async fn create_project_creates_a_project_map() {
    let (server, store) = server_with(|_v, _s| {});

    let result = server
        .create_project(Parameters(CreateProjectInput {
            title: "Widget Redesign".to_owned(),
            context: "work".to_owned(),
            core_question: None,
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
            title: "X".to_owned(),
            context: "nonsense".to_owned(),
            core_question: None,
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
        let today = moment(2026, 1, 1, 9, 0).date();
        for i in 1..=5 {
            vault
                .create_project(today, &format!("Project {i}"), Context::Work, None)
                .expect("seed project");
        }
    });

    let result = server
        .create_project(Parameters(CreateProjectInput {
            title: "Sixth".to_owned(),
            context: "work".to_owned(),
            core_question: None,
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
async fn create_question_creates_a_note_and_rejects_unknown_domain() {
    let (server, store) = server_with(|_v, _s| {});

    let result = server
        .create_question(Parameters(CreateQuestionInput {
            domain: "research".to_owned(),
            text: "What is the best benchmark?".to_owned(),
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
            domain: "philosophy".to_owned(),
            text: "?".to_owned(),
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
            question: "Reference material".to_owned(),
            project: None,
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
            name: "Finances".to_owned(),
            context: "personal".to_owned(),
            expanded: false,
        }))
        .await
        .expect("flat stewardship");
    let flat_path = decode_json(&flat)["path"].as_str().unwrap().to_owned();
    assert!(flat_path.ends_with("finances.md"), "flat path: {flat_path}");

    let expanded = server
        .create_stewardship(Parameters(CreateStewardshipInput {
            name: "Health".to_owned(),
            context: "personal".to_owned(),
            expanded: true,
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
