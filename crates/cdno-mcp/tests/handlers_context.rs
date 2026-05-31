//! In-process tests for the three context-gathering handlers
//! implemented in #46.
//!
//! Tests call the handler methods directly on `CuadernoServer`
//! (they're `pub async fn` for exactly this purpose) rather than
//! going through `ServerHandler::call_tool`. Spinning up a real rmcp
//! request context for every test would test rmcp's dispatch
//! (already covered upstream) rather than our handler logic, and the
//! constructor surface for the test context isn't ergonomically
//! usable outside the rmcp crate. Decoding the `CallToolResult` JSON
//! payload still exercises the full DTO + serialisation path.
//!
//! The four stubbed handlers (`get_weekly_context`,
//! `get_monthly_context`, `get_project_context`,
//! `get_stewardship_tracking`) are deferred to follow-up issues;
//! their `not_yet_implemented` paths are covered by the catalogue
//! test in `tests/server.rs`.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::QuestionDomain;
use cdno_mcp::CuadernoServer;
use cdno_mcp::server::{
    EmptyInput, GetActiveQuestionsInput, GetOrientationInput, PortfolioSlugInput,
};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorCode, RawContent};

fn moment(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
}

fn empty_server() -> CuadernoServer {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(store, index, VaultConfig::default()).unwrap();
    CuadernoServer::new(Arc::new(vault))
}

fn server_with<F: FnOnce(&Vault)>(seed: F) -> CuadernoServer {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(store, index, VaultConfig::default()).unwrap();
    seed(&vault);
    CuadernoServer::new(Arc::new(vault))
}

/// Decode the single JSON content item of a successful tool result.
/// Asserts `is_error == false` so a Result-rewriting bug doesn't
/// pass silently.
fn decode_json(result: &CallToolResult) -> serde_json::Value {
    assert_eq!(
        result.is_error,
        Some(false),
        "tool returned an error result: {result:?}"
    );
    assert_eq!(
        result.content.len(),
        1,
        "expected exactly one content item, got {}",
        result.content.len()
    );
    match &result.content[0].raw {
        RawContent::Text(t) => {
            serde_json::from_str(&t.text).expect("content payload is valid JSON")
        }
        other => panic!("expected text content carrying JSON, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// get_orientation
// ---------------------------------------------------------------------

#[tokio::test]
async fn get_orientation_returns_empty_context_for_empty_vault() {
    let server = empty_server();
    let result = server
        .get_orientation(Parameters(GetOrientationInput { energy: None }))
        .await
        .expect("get_orientation");
    let value = decode_json(&result);
    assert!(value["commitments"].as_array().unwrap().is_empty());
    assert!(value["projects"].as_array().unwrap().is_empty());
    assert!(value["lapsed_habits"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_orientation_accepts_and_ignores_energy_field() {
    let server = empty_server();
    let result = server
        .get_orientation(Parameters(GetOrientationInput {
            energy: Some("deep".to_owned()),
        }))
        .await
        .expect("get_orientation with energy");
    // Energy is documented as reserved for client-side biasing; the
    // server returns the unfiltered context regardless.
    let value = decode_json(&result);
    assert!(value.get("commitments").is_some());
}

// ---------------------------------------------------------------------
// get_active_questions
// ---------------------------------------------------------------------

#[tokio::test]
async fn get_active_questions_returns_empty_for_empty_vault() {
    let server = empty_server();
    let result = server
        .get_active_questions(Parameters(GetActiveQuestionsInput { domain: None }))
        .await
        .expect("get_active_questions");
    let value = decode_json(&result);
    assert!(value.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_active_questions_returns_all_when_no_domain_filter() {
    let server = server_with(|vault| {
        vault
            .create_question(
                moment(2026, 1, 10, 9, 0),
                QuestionDomain::Research,
                "Does X beat Y?",
            )
            .unwrap();
        vault
            .create_question(
                moment(2026, 1, 11, 9, 0),
                QuestionDomain::Life,
                "Where to in 5y?",
            )
            .unwrap();
    });
    let result = server
        .get_active_questions(Parameters(GetActiveQuestionsInput { domain: None }))
        .await
        .unwrap();
    let value = decode_json(&result);
    let arr = value.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    let domains: Vec<&str> = arr.iter().map(|q| q["domain"].as_str().unwrap()).collect();
    assert!(domains.contains(&"research"));
    assert!(domains.contains(&"life"));
}

#[tokio::test]
async fn get_active_questions_filters_by_domain_when_supplied() {
    let server = server_with(|vault| {
        vault
            .create_question(
                moment(2026, 1, 10, 9, 0),
                QuestionDomain::Research,
                "Does X beat Y?",
            )
            .unwrap();
        vault
            .create_question(
                moment(2026, 1, 11, 9, 0),
                QuestionDomain::Life,
                "Where to in 5y?",
            )
            .unwrap();
    });
    let result = server
        .get_active_questions(Parameters(GetActiveQuestionsInput {
            domain: Some("life".to_owned()),
        }))
        .await
        .unwrap();
    let value = decode_json(&result);
    let arr = value.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["domain"], "life");
}

#[tokio::test]
async fn get_active_questions_rejects_unknown_domain_with_invalid_params() {
    let server = empty_server();
    let err = server
        .get_active_questions(Parameters(GetActiveQuestionsInput {
            domain: Some("fortnightly".to_owned()),
        }))
        .await
        .expect_err("unknown domain should error");
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("domain"), "msg: {}", err.message);
}

// ---------------------------------------------------------------------
// get_portfolio_contents
// ---------------------------------------------------------------------

#[tokio::test]
async fn get_portfolio_contents_returns_frontmatter_and_evidence() {
    let server = server_with(|vault| {
        vault
            .create_portfolio(
                moment(2026, 2, 1, 9, 0),
                "Does sparse beat dense on OOD?",
                Some("projects/surrogate"),
            )
            .unwrap();
        vault
            .file_evidence(
                moment(2026, 3, 15, 10, 0),
                "does-sparse-beat-dense-on-ood",
                "Chen 2025",
                "projects/surrogate",
                "They show 4x speedup at 95% accuracy.\n",
            )
            .unwrap();
    });
    let result = server
        .get_portfolio_contents(Parameters(PortfolioSlugInput {
            portfolio: "does-sparse-beat-dense-on-ood".to_owned(),
        }))
        .await
        .unwrap();
    let value = decode_json(&result);
    assert_eq!(value["slug"], "does-sparse-beat-dense-on-ood");
    assert_eq!(value["question"], "Does sparse beat dense on OOD?");
    assert_eq!(value["project"], "[[projects/surrogate]]");
    let evidence = value["evidence"].as_array().unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0]["source"], "Chen 2025");
    assert!(
        evidence[0]["path"]
            .as_str()
            .unwrap()
            .ends_with("chen-2025.md")
    );
}

#[tokio::test]
async fn get_portfolio_contents_errors_on_missing_portfolio() {
    let server = empty_server();
    let err = server
        .get_portfolio_contents(Parameters(PortfolioSlugInput {
            portfolio: "nonexistent".to_owned(),
        }))
        .await
        .expect_err("missing portfolio should error");
    assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
    assert!(
        err.message.contains("nonexistent") || err.message.to_lowercase().contains("not found"),
        "msg: {}",
        err.message
    );
}

// ---------------------------------------------------------------------
// get_weekly_context
// ---------------------------------------------------------------------
//
// These tests can only assert on shape and on data clearly inside or
// outside the current ISO week (which depends on when the test
// runs). Seed log entries on today and assert they appear; the
// week_of field is checked to equal today's Monday.

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

fn monday_of(date: NaiveDate) -> NaiveDate {
    use chrono::Datelike;
    let days = date.weekday().num_days_from_monday() as i64;
    date - chrono::Duration::days(days)
}

fn seed_daily_with_log(vault_root_store: &Arc<dyn VaultStore>, date: NaiveDate, log_text: &str) {
    let path = cdno_core::path::VaultPath::new(cdno_core::paths::daily_note_relpath(date)).unwrap();
    let body = format!(
        "---\ndate: {date}\ntype: daily\n---\n\n# {date}\n\n## Logs\n- **09:00**: {log_text}\n",
        date = date.format("%Y-%m-%d"),
    );
    vault_root_store.write_file(&path, &body).unwrap();
}

#[tokio::test]
async fn get_weekly_context_returns_shape_with_week_of_set_to_monday() {
    let server = empty_server();
    let result = server
        .get_weekly_context(Parameters(EmptyInput::default()))
        .await
        .expect("get_weekly_context");
    let value = decode_json(&result);
    assert_eq!(
        value["week_of"].as_str().unwrap(),
        monday_of(today()).format("%Y-%m-%d").to_string()
    );
    // Shape: four arrays + week_of. Order-independent presence check.
    for key in ["logs", "completed_actions", "state_changes", "commitments"] {
        assert!(
            value[key].is_array(),
            "expected `{key}` to be an array: {value}"
        );
    }
}

#[tokio::test]
async fn get_weekly_context_includes_a_log_from_today() {
    // Build a server over a hand-seeded store so we can write today's
    // daily entry into the same vault the server reads.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    seed_daily_with_log(&store, today(), "MCP weekly smoke");
    let (vault, _r) = Vault::new(Arc::clone(&store), index, VaultConfig::default()).unwrap();
    let server = CuadernoServer::new(Arc::new(vault));

    let result = server
        .get_weekly_context(Parameters(EmptyInput::default()))
        .await
        .unwrap();
    let value = decode_json(&result);
    let logs = value["logs"].as_array().unwrap();
    assert!(
        logs.iter()
            .any(|l| l["text"].as_str().unwrap().contains("MCP weekly smoke")),
        "today's seeded log line should appear: {logs:?}"
    );
}

// ---------------------------------------------------------------------
// get_monthly_context
// ---------------------------------------------------------------------

#[tokio::test]
async fn get_monthly_context_returns_shape_with_since_and_slots() {
    let server = empty_server();
    let result = server
        .get_monthly_context(Parameters(EmptyInput::default()))
        .await
        .expect("get_monthly_context");
    let value = decode_json(&result);
    // since = today - 30 days
    let expected = (today() - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    assert_eq!(value["since"].as_str().unwrap(), expected);
    // Seven slices + slots block.
    for key in [
        "completed_actions",
        "active_questions",
        "portfolios",
        "stuck_projects",
        "stewardships",
        "commitments",
    ] {
        assert!(
            value[key].is_array(),
            "expected `{key}` to be an array: {value}"
        );
    }
    assert_eq!(value["slots"]["active"].as_u64().unwrap(), 0);
    // Default cap from VaultConfig::default().
    assert_eq!(value["slots"]["cap"].as_u64().unwrap(), 5);
}

#[tokio::test]
async fn get_monthly_context_surfaces_active_questions_and_portfolios() {
    let server = server_with(|vault| {
        vault
            .create_question(
                moment(2026, 1, 10, 9, 0),
                QuestionDomain::Research,
                "What truly matters?",
            )
            .unwrap();
        vault
            .create_portfolio(moment(2026, 1, 12, 9, 0), "Does X beat Y?", None)
            .unwrap();
    });
    let result = server
        .get_monthly_context(Parameters(EmptyInput::default()))
        .await
        .unwrap();
    let value = decode_json(&result);
    assert_eq!(value["active_questions"].as_array().unwrap().len(), 1);
    assert_eq!(value["portfolios"].as_array().unwrap().len(), 1);
}
