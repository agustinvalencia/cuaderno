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
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::QuestionDomain;
use cdno_mcp::CuadernoServer;
use cdno_mcp::server::{
    EmptyInput, GetActiveQuestionsInput, GetCommitmentsInput, GetOrientationInput,
    PortfolioSlugInput, SearchNotesInput,
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

/// Build a server from raw seeded `(path, body)` notes — for tests
/// that need on-disk content (e.g. a note carrying a broken wikilink)
/// rather than domain-method-created state.
fn server_with_notes(notes: &[(&str, &str)]) -> CuadernoServer {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    for (path, body) in notes {
        store
            .write_file(&VaultPath::new(path).unwrap(), body)
            .unwrap();
    }
    let (vault, _r) = Vault::new(store, index, VaultConfig::default()).unwrap();
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
    // Plain prose evidence is not an attachment: `kind` is omitted
    // (skip_serializing_if), so the JSON has no such key.
    assert!(
        evidence[0].get("kind").is_none(),
        "plain evidence must not carry a kind: {}",
        evidence[0]
    );
}

#[tokio::test]
async fn get_portfolio_contents_surfaces_attachment_kind() {
    // An attachment stub must report its media `kind` distinctly so a
    // retrieving agent can tell it apart from prose and know to
    // dereference the linked artefact (#154).
    let dir = tempfile::tempdir().unwrap();
    let artefact = dir.path().join("figure.png");
    std::fs::write(&artefact, b"\x89PNG fake").unwrap();

    let server = server_with(|vault| {
        vault
            .create_portfolio(moment(2026, 2, 1, 9, 0), "Does sparse beat dense?", None)
            .unwrap();
        vault
            .file_attachment(
                moment(2026, 3, 15, 10, 0),
                "does-sparse-beat-dense",
                &artefact,
                "Whiteboard",
                "projects/surrogate",
                "Sketch of the attention sparsity pattern.",
            )
            .unwrap();
    });

    let result = server
        .get_portfolio_contents(Parameters(PortfolioSlugInput {
            portfolio: "does-sparse-beat-dense".to_owned(),
        }))
        .await
        .unwrap();
    let value = decode_json(&result);
    let evidence = value["evidence"].as_array().unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0]["kind"], "image");
    assert_eq!(evidence[0]["source"], "Whiteboard");
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

/// Seed a daily note whose `## Logs` section carries `log_lines`
/// plain checkpoint lines plus, when `state_change` is `Some`, one
/// canonical `state on [[slug]]` block with the given long
/// before/after bodies (the shape `update_project_state` writes and
/// `project_state_changes_between` parses).
fn seed_daily_week_fixture(
    store: &Arc<dyn VaultStore>,
    date: NaiveDate,
    log_lines: usize,
    state_change: Option<(&str, &str, &str)>,
) {
    let path = cdno_core::path::VaultPath::new(cdno_core::paths::daily_note_relpath(date)).unwrap();
    let mut body = format!(
        "---\ndate: {date}\ntype: daily\n---\n\n# {date}\n\n## Logs\n",
        date = date.format("%Y-%m-%d"),
    );
    if let Some((slug, was, now)) = state_change {
        body.push_str(&format!(
            "- **08:00**: state on [[{slug}]]\n  was: {was}\n  now: {now}\n"
        ));
    }
    for i in 0..log_lines {
        body.push_str(&format!(
            "- **09:{minute:02}**: checkpoint {i} on the day, a terse but realistic log line\n",
            minute = i % 60,
        ));
    }
    store.write_file(&path, &body).unwrap();
}

// A ~400-word Current State body — representative of an active
// project whose state has grown over the week. Two of these per state
// change (before + after) are exactly what blew the payload to 82k in
// GH #298.
const LONG_STATE: &str = "The nonlinear factor model refactor is mid-flight: the core solver now \
threads the sparse Jacobian through the block-elimination path, but the boundary handling for the \
periodic terms is still provisional and only exercised by the synthetic fixtures. We validated the \
forward pass against the reference implementation to within tolerance on the small grid, though the \
large grid diverges after roughly forty iterations, which points at an accumulation bug in the \
residual reduction rather than the factorisation itself. The next concrete step is to instrument \
the residual norm per block and compare against the dense baseline, then decide whether the \
preconditioner needs the extra Chebyshev smoothing pass or whether the divergence is purely a \
scheduling artefact of the parallel reduction. Documentation and the benchmark harness are \
lagging behind the code and will need a dedicated pass before this is shareable. Open questions \
remain about whether the memory budget holds on the target hardware once the halo exchange is \
enabled, and about the interaction between the adaptive time step and the factor caching.";

#[tokio::test]
async fn get_weekly_context_payload_is_bounded_for_a_heavy_week() {
    // A realistic heavy week: every day of the current ISO week carries
    // a long-bodied project state change plus a stack of log lines,
    // reproducing the GH #298 conditions (full before/after state bodies
    // multiplied across changes, and >100 verbatim log lines). Assert
    // the serialised payload comes out bounded.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());

    let monday = monday_of(today());
    // 7 days x 25 log lines = 175 lines (> WEEKLY_LOGS_MAX of 100), and
    // one long state change per day = 7 changes with full before/after
    // bodies.
    for offset in 0..7 {
        let date = monday + chrono::Duration::days(offset);
        seed_daily_week_fixture(&store, date, 25, Some(("nfm", LONG_STATE, LONG_STATE)));
    }

    let (vault, _r) = Vault::new(Arc::clone(&store), index, VaultConfig::default()).unwrap();
    let server = CuadernoServer::new(Arc::new(vault));

    let result = server
        .get_weekly_context(Parameters(EmptyInput::default()))
        .await
        .expect("get_weekly_context");

    // Measure the raw serialised text — the exact bytes the MCP client
    // receives and counts against its token cap.
    let raw = match &result.content[0].raw {
        RawContent::Text(t) => t.text.clone(),
        other => panic!("expected text content, got {other:?}"),
    };
    let value = decode_json(&result);

    // Ceiling well under the ~25k-char target from GH #298. The
    // un-truncated fixture would serialise to well over 80k.
    const CEILING: usize = 25_000;
    assert!(
        raw.len() < CEILING,
        "payload should be bounded under {CEILING} chars, got {}",
        raw.len()
    );

    // state_changes: the new_state body is truncated to the gist (old_state
    // is dropped entirely, GH #351).
    let state_changes = value["state_changes"].as_array().unwrap();
    assert!(!state_changes.is_empty(), "fixture should yield changes");
    for change in state_changes {
        assert!(
            change.get("old_state").is_none(),
            "old_state must not be shipped: {change}"
        );
        let body = change["new_state"].as_str().unwrap();
        // <= 200 content chars + one ellipsis marker.
        assert!(
            body.chars().count() <= 201,
            "new_state should be truncated, got {} chars",
            body.chars().count()
        );
        // The seeded body is far longer than the cap, so it must carry the
        // observable truncation marker.
        assert!(
            body.ends_with('…'),
            "new_state should end with the ellipsis marker: {body:?}"
        );
    }

    // logs: capped to the most-recent WEEKLY_LOGS_MAX lines.
    let logs = value["logs"].as_array().unwrap();
    assert_eq!(
        logs.len(),
        cdno_mcp::dto::WEEKLY_LOGS_MAX,
        "logs should be capped to the most-recent {} lines",
        cdno_mcp::dto::WEEKLY_LOGS_MAX
    );
}

/// Run `get_weekly_context` over a store seeded with a single state
/// change on today, returning the first state change's `new_state`
/// gist. A focused helper for the boundary tests below.
async fn first_new_state(was: &str, now: &str) -> String {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    seed_daily_week_fixture(&store, today(), 0, Some(("nfm", was, now)));
    let (vault, _r) = Vault::new(Arc::clone(&store), index, VaultConfig::default()).unwrap();
    let server = CuadernoServer::new(Arc::new(vault));
    let result = server
        .get_weekly_context(Parameters(EmptyInput::default()))
        .await
        .expect("get_weekly_context");
    let value = decode_json(&result);
    value["state_changes"][0]["new_state"]
        .as_str()
        .expect("state change should carry a new_state string")
        .to_owned()
}

#[tokio::test]
async fn get_weekly_context_truncates_on_a_multibyte_char_boundary() {
    // A state body of 250 multibyte (3-byte) codepoints: char index 200
    // sits mid-way through the string, and byte offset 200 is NOT a char
    // boundary. A naive `&s[..200]` byte-slice would panic here; the
    // char-based truncation must not. Guards against a regression to
    // byte-slicing that would otherwise leave the suite green (the
    // ASCII fixtures never straddle a codepoint boundary).
    let body = "\u{3042}".repeat(250); // 'あ' x250 = 750 bytes, 250 chars
    let new_state = first_new_state(&body, &body).await;

    // Did not panic, and the result is a valid string bounded to the cap
    // plus the one-char ellipsis marker.
    assert_eq!(
        new_state.chars().count(),
        201,
        "200 content chars + one ellipsis marker"
    );
    assert!(
        new_state.ends_with('\u{2026}'),
        "truncated multibyte snippet should end with the ellipsis marker: {new_state:?}"
    );
    // Every retained char is the seeded codepoint — no split/replacement.
    assert!(
        new_state.chars().take(200).all(|c| c == '\u{3042}'),
        "retained chars should be the intact seeded codepoint"
    );
}

#[tokio::test]
async fn get_weekly_context_passes_through_a_body_at_exactly_the_cap() {
    // Exactly STATE_SNIPPET_MAX_CHARS (200) chars: the `<=` early-return
    // branch must return it unchanged with NO ellipsis. Guards the
    // off-by-one at the truncation boundary.
    let body = "a".repeat(200);
    let new_state = first_new_state(&body, &body).await;
    assert_eq!(new_state, body, "a body at the cap passes through verbatim");
    assert!(
        !new_state.ends_with('\u{2026}'),
        "a body at the cap must not gain a truncation marker"
    );
}

#[tokio::test]
async fn get_weekly_context_leaves_a_normal_small_week_untouched() {
    // A light week: one short state change plus a handful of log lines,
    // all well under the caps. Nothing should be truncated — the short
    // state body round-trips verbatim and the logs vec is returned whole.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let short_state = "shipped the parser; next up is the boundary handling";
    // One day: 10 plain log lines + one state change. The state-change
    // header parses as a log line too (its `was:`/`now:` fold in as
    // continuations), so the expected count is 10 + 1 = 11.
    seed_daily_week_fixture(&store, today(), 10, Some(("nfm", short_state, short_state)));
    let (vault, _r) = Vault::new(Arc::clone(&store), index, VaultConfig::default()).unwrap();
    let server = CuadernoServer::new(Arc::new(vault));

    let result = server
        .get_weekly_context(Parameters(EmptyInput::default()))
        .await
        .expect("get_weekly_context");
    let value = decode_json(&result);

    // Short state body returned byte-for-byte, no truncation marker.
    let new_state = value["state_changes"][0]["new_state"].as_str().unwrap();
    assert_eq!(new_state, short_state);
    assert!(!new_state.ends_with('\u{2026}'));

    // Logs vec returned whole (cap is a no-op below WEEKLY_LOGS_MAX).
    let logs = value["logs"].as_array().unwrap();
    assert_eq!(logs.len(), 11, "10 plain lines + 1 state-change header");
    assert!(logs.len() < cdno_mcp::dto::WEEKLY_LOGS_MAX);
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

// ---------------------------------------------------------------------
// get_project_context
// ---------------------------------------------------------------------

use cdno_domain::frontmatter::Context;

#[tokio::test]
async fn get_project_context_returns_frontmatter_body_and_empty_collections() {
    use cdno_mcp::server::ProjectSlugInput;
    let server = server_with(|vault| {
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
        .get_project_context(Parameters(ProjectSlugInput {
            project: "surrogate-model".to_owned(),
        }))
        .await
        .expect("get_project_context");
    let value = decode_json(&result);

    assert_eq!(value["slug"], "surrogate-model");
    assert_eq!(value["frontmatter"]["context"], "work");
    assert_eq!(value["frontmatter"]["status"], "active");
    assert!(
        value["body_markdown"]
            .as_str()
            .unwrap()
            .contains("# Surrogate model")
    );
    assert!(value["recent_mentions"].as_array().unwrap().is_empty());
    assert!(
        value["backlinks"]["questions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(value["core_question"].is_null());
}

#[tokio::test]
async fn get_project_context_resolves_core_question_when_set() {
    use cdno_domain::frontmatter::QuestionDomain;
    let server = server_with(|vault| {
        // Create question first so the project's core_question
        // wikilink resolves to an existing note.
        vault
            .create_question(
                moment(2026, 1, 10, 9, 0),
                QuestionDomain::Research,
                "Surrogate cost",
            )
            .unwrap();
        vault
            .create_project(
                moment(2026, 5, 1, 9, 0).date(),
                "Surrogate model",
                Context::Work,
                Some("questions/research/surrogate-cost"),
            )
            .unwrap();
    });

    use cdno_mcp::server::ProjectSlugInput;
    let result = server
        .get_project_context(Parameters(ProjectSlugInput {
            project: "surrogate-model".to_owned(),
        }))
        .await
        .unwrap();
    let value = decode_json(&result);

    let core = &value["core_question"];
    assert!(!core.is_null(), "core_question should resolve: {value}");
    assert_eq!(core["slug"], "surrogate-cost");
    assert_eq!(core["domain"], "research");
}

#[tokio::test]
async fn get_project_context_returns_none_for_unresolved_core_question() {
    // Project sets core_question to a wikilink target that doesn't
    // resolve — the field is included as null rather than erroring.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let project_body = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\ncore_question: \"[[questions/research/missing]]\"\n---\n\n# Surrogate model\n\n## Current State\nN/A.\n\n## Next Actions\n";
    store
        .write_file(
            &cdno_core::path::VaultPath::new("projects/surrogate-model.md").unwrap(),
            project_body,
        )
        .unwrap();
    let (vault, _r) = Vault::new(Arc::clone(&store), index, VaultConfig::default()).unwrap();
    let server = CuadernoServer::new(Arc::new(vault));

    use cdno_mcp::server::ProjectSlugInput;
    let result = server
        .get_project_context(Parameters(ProjectSlugInput {
            project: "surrogate-model".to_owned(),
        }))
        .await
        .unwrap();
    let value = decode_json(&result);
    assert!(value["core_question"].is_null());
    assert_eq!(
        value["frontmatter"]["core_question"], "[[questions/research/missing]]",
        "raw wikilink still surfaced in frontmatter even when unresolved"
    );
}

#[tokio::test]
async fn get_project_context_errors_on_missing_project() {
    let server = empty_server();
    use cdno_mcp::server::ProjectSlugInput;
    let err = server
        .get_project_context(Parameters(ProjectSlugInput {
            project: "nonexistent".to_owned(),
        }))
        .await
        .expect_err("missing project should error");
    assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
}

#[tokio::test]
async fn get_project_context_caps_recent_mentions() {
    // A very active project: several recent daily notes each carrying a
    // stack of log lines that wikilink it — more than PROJECT_MENTIONS_MAX
    // in total, all inside the 30-day window. The payload must come back
    // capped to the most-recent PROJECT_MENTIONS_MAX lines (GH #352).
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());

    store
        .write_file(
            &cdno_core::path::VaultPath::new("projects/surrogate-model.md").unwrap(),
            "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Surrogate model\n\n## Current State\nN/A.\n\n## Next Actions\n",
        )
        .unwrap();

    // 3 recent days x 30 mention lines = 90 mentions (> the cap of 50).
    for day in 0..3 {
        let date = today() - chrono::Duration::days(day);
        let mut logs = String::new();
        for m in 0..30 {
            logs.push_str(&format!("- **09:{m:02}**: worked on [[surrogate-model]]\n"));
        }
        let body = format!(
            "---\ndate: {date}\ntype: daily\n---\n\n# {date}\n\n## Logs\n{logs}",
            date = date.format("%Y-%m-%d"),
        );
        store
            .write_file(
                &cdno_core::path::VaultPath::new(cdno_core::paths::daily_note_relpath(date))
                    .unwrap(),
                &body,
            )
            .unwrap();
    }

    let (vault, _r) = Vault::new(Arc::clone(&store), index, VaultConfig::default()).unwrap();
    let server = CuadernoServer::new(Arc::new(vault));

    use cdno_mcp::server::ProjectSlugInput;
    let result = server
        .get_project_context(Parameters(ProjectSlugInput {
            project: "surrogate-model".to_owned(),
        }))
        .await
        .expect("get_project_context");
    let value = decode_json(&result);

    let mentions = value["recent_mentions"].as_array().unwrap();
    assert_eq!(
        mentions.len(),
        cdno_mcp::dto::PROJECT_MENTIONS_MAX,
        "recent_mentions should be capped to the most-recent {} lines",
        cdno_mcp::dto::PROJECT_MENTIONS_MAX
    );
    // The kept slice is the most-recent: with 30 lines per day, the cap of
    // 50 keeps all of today plus part of yesterday, so every surviving
    // mention is dated today or yesterday — never the oldest day, whose
    // mentions are entirely dropped from the front.
    let oldest_dropped = today() - chrono::Duration::days(2);
    for m in mentions {
        assert_ne!(
            m["date"].as_str().unwrap(),
            oldest_dropped.format("%Y-%m-%d").to_string(),
            "the oldest day's mentions should have been dropped, not kept"
        );
    }
}

#[tokio::test]
async fn get_project_context_caps_a_pathologically_long_body() {
    // A project map whose body runs past the safety valve: the returned
    // body_markdown must be bounded and carry the observable truncation
    // marker, while the full body stays one read_note away (GH #388).
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let huge = "x".repeat(cdno_mcp::dto::PROJECT_BODY_MAX_CHARS + 500);
    let body = format!(
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-05-01\n---\n\n# Surrogate model\n\n## Current State\n{huge}\n\n## Next Actions\n"
    );
    store
        .write_file(
            &cdno_core::path::VaultPath::new("projects/surrogate-model.md").unwrap(),
            &body,
        )
        .unwrap();
    let (vault, _r) = Vault::new(Arc::clone(&store), index, VaultConfig::default()).unwrap();
    let server = CuadernoServer::new(Arc::new(vault));

    use cdno_mcp::server::ProjectSlugInput;
    let result = server
        .get_project_context(Parameters(ProjectSlugInput {
            project: "surrogate-model".to_owned(),
        }))
        .await
        .expect("get_project_context");
    let value = decode_json(&result);

    let body_md = value["body_markdown"].as_str().unwrap();
    // <= cap content chars + one ellipsis marker.
    assert!(
        body_md.chars().count() <= cdno_mcp::dto::PROJECT_BODY_MAX_CHARS + 1,
        "body should be capped, got {} chars",
        body_md.chars().count()
    );
    assert!(
        body_md.ends_with('…'),
        "a truncated body must carry the observable marker"
    );
}

#[test]
fn project_backlinks_dto_caps_each_group() {
    // A heavily-referenced project: one backlink group runs past the
    // per-group cap. The DTO must keep only PROJECT_BACKLINKS_PER_GROUP_MAX
    // (GH #388). Exercised at the From boundary directly — no need to seed
    // hundreds of backlinking notes.
    let evidence: Vec<cdno_core::path::VaultPath> = (0..150)
        .map(|i| cdno_core::path::VaultPath::new(format!("portfolios/x/ev-{i}.md")).unwrap())
        .collect();
    let domain = cdno_domain::ProjectBacklinks {
        evidence,
        ..Default::default()
    };
    let dto = cdno_mcp::dto::ProjectBacklinksDto::from(domain);
    assert_eq!(
        dto.evidence.len(),
        cdno_mcp::dto::PROJECT_BACKLINKS_PER_GROUP_MAX,
        "the over-cap group is trimmed to the per-group max"
    );
    // Under-cap groups pass through untouched.
    assert!(dto.questions.is_empty());
}

// ---------------------------------------------------------------------
// get_stewardship_tracking
// ---------------------------------------------------------------------

use cdno_mcp::server::GetStewardshipTrackingInput;

#[tokio::test]
async fn get_stewardship_tracking_returns_entries_in_default_window() {
    // Default period is 90d. Seed tracking notes inside and outside
    // that window; only the recent ones should surface.
    let server = server_with(|vault| {
        vault
            .create_stewardship_expanded(moment(2026, 1, 1, 9, 0), "Health", Context::Personal)
            .unwrap();
        // Within 90 days of today.
        let recent = chrono::Local::now().naive_local() - chrono::Duration::days(10);
        vault
            .add_tracking_entry(recent, "health", "gym", None, "Felt strong")
            .unwrap();
        // 200 days ago — outside the default window.
        let old = chrono::Local::now().naive_local() - chrono::Duration::days(200);
        vault
            .add_tracking_entry(old, "health", "gym", None, "Old session")
            .unwrap();
    });
    let result = server
        .get_stewardship_tracking(Parameters(GetStewardshipTrackingInput {
            stewardship: "health".to_owned(),
            activity: "gym".to_owned(),
            period: None,
        }))
        .await
        .expect("get_stewardship_tracking");
    let value = decode_json(&result);
    assert_eq!(value["stewardship"], "health");
    assert_eq!(value["activity"], "gym");
    let entries = value["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{value}");
    // body_excerpt is the first non-blank line after the H1; for the
    // gym template that's the table header rather than the rendered
    // {{content}} block. The Notes section content lives elsewhere
    // in the body — we just check the path lands under the right
    // tracking subdir.
    assert_eq!(entries[0]["activity"], "gym");
    assert!(
        entries[0]["path"]
            .as_str()
            .unwrap()
            .starts_with("stewardships/health/tracking/")
    );
}

#[tokio::test]
async fn get_stewardship_tracking_filters_by_activity() {
    let server = server_with(|vault| {
        vault
            .create_stewardship_expanded(moment(2026, 1, 1, 9, 0), "Health", Context::Personal)
            .unwrap();
        let now = chrono::Local::now().naive_local() - chrono::Duration::days(5);
        vault
            .add_tracking_entry(now, "health", "gym", None, "")
            .unwrap();
        let now2 = chrono::Local::now().naive_local() - chrono::Duration::days(6);
        vault
            .add_tracking_entry(now2, "health", "body", None, "")
            .unwrap();
    });
    let result = server
        .get_stewardship_tracking(Parameters(GetStewardshipTrackingInput {
            stewardship: "health".to_owned(),
            activity: "body".to_owned(),
            period: None,
        }))
        .await
        .unwrap();
    let value = decode_json(&result);
    let entries = value["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["activity"], "body");
}

#[tokio::test]
async fn get_stewardship_tracking_accepts_period_units() {
    // Spot-check each unit (d/w/m/y) parses without an error.
    let server = server_with(|vault| {
        vault
            .create_stewardship_expanded(moment(2026, 1, 1, 9, 0), "Health", Context::Personal)
            .unwrap();
    });
    for period in ["7d", "2w", "3m", "1y"] {
        let result = server
            .get_stewardship_tracking(Parameters(GetStewardshipTrackingInput {
                stewardship: "health".to_owned(),
                activity: "gym".to_owned(),
                period: Some(period.to_owned()),
            }))
            .await;
        assert!(
            result.is_ok(),
            "period `{period}` should parse but errored: {result:?}"
        );
    }
}

#[tokio::test]
async fn get_stewardship_tracking_rejects_unknown_period_unit_with_invalid_params() {
    let server = server_with(|vault| {
        vault
            .create_stewardship_expanded(moment(2026, 1, 1, 9, 0), "Health", Context::Personal)
            .unwrap();
    });
    for bad in ["30x", "abc", "0d", "10"] {
        let err = server
            .get_stewardship_tracking(Parameters(GetStewardshipTrackingInput {
                stewardship: "health".to_owned(),
                activity: "gym".to_owned(),
                period: Some(bad.to_owned()),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS, "period `{bad}`");
        assert!(err.message.contains("period"), "msg: {}", err.message);
    }
}

// ---- search_notes (#172) ------------------------------------------------

fn search_seed(vault: &Vault) {
    vault
        .create_question(
            moment(2026, 1, 10, 9, 0),
            QuestionDomain::Research,
            "Does sparse attention beat dense?",
        )
        .unwrap();
    vault
        .create_portfolio(
            moment(2026, 2, 1, 9, 0),
            "Dense versus sparse tradeoffs",
            None,
        )
        .unwrap();
}

#[tokio::test]
async fn search_notes_returns_hits_with_expected_shape() {
    let server = server_with(search_seed);
    let result = server
        .search_notes(Parameters(SearchNotesInput {
            query: "sparse".to_owned(),
            note_type: None,
            from: None,
            to: None,
            portfolio: None,
            limit: 20,
        }))
        .await
        .unwrap();
    let value = decode_json(&result);
    let hits = value.as_array().unwrap();
    assert!(
        hits.len() >= 2,
        "should find the question and the portfolio"
    );
    for hit in hits {
        assert!(hit["path"].is_string());
        assert!(hit["note_type"].is_string());
        assert!(hit["snippet"].is_string());
        assert!(hit["score"].is_number());
        // `title` is the one Option field — present as a string or null,
        // never absent (no skip_serializing_if).
        let title = &hit["title"];
        assert!(title.is_string() || title.is_null(), "title: {title}");
    }
}

#[tokio::test]
async fn search_notes_passes_the_date_window_through() {
    // Two questions created in different months; the `from` bound must
    // reach the domain filter (a from/to transposition would fail here).
    let server = server_with(|vault| {
        vault
            .create_question(
                moment(2026, 1, 10, 9, 0),
                QuestionDomain::Research,
                "sparse attention in January",
            )
            .unwrap();
        vault
            .create_question(
                moment(2026, 6, 10, 9, 0),
                QuestionDomain::Research,
                "sparse attention in June",
            )
            .unwrap();
    });
    let result = server
        .search_notes(Parameters(SearchNotesInput {
            query: "sparse".to_owned(),
            note_type: None,
            from: Some(NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()),
            to: None,
            portfolio: None,
            limit: 20,
        }))
        .await
        .unwrap();
    let value = decode_json(&result);
    let hits = value.as_array().unwrap();
    assert_eq!(
        hits.len(),
        1,
        "only the June question is after the `from` bound"
    );
    assert!(hits[0]["path"].as_str().unwrap().contains("june"));
}

#[tokio::test]
async fn search_notes_honours_the_limit() {
    let server = server_with(search_seed);
    let result = server
        .search_notes(Parameters(SearchNotesInput {
            query: "sparse".to_owned(),
            note_type: None,
            from: None,
            to: None,
            portfolio: None,
            limit: 1,
        }))
        .await
        .unwrap();
    let value = decode_json(&result);
    assert_eq!(value.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn search_notes_filters_by_note_type() {
    let server = server_with(search_seed);
    let result = server
        .search_notes(Parameters(SearchNotesInput {
            query: "sparse".to_owned(),
            note_type: Some("question".to_owned()),
            from: None,
            to: None,
            portfolio: None,
            limit: 20,
        }))
        .await
        .unwrap();
    let value = decode_json(&result);
    let hits = value.as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h["note_type"] == "question"));
}

#[tokio::test]
async fn search_notes_rejects_unknown_note_type() {
    // `note_type` accepts a built-in or config-defined custom type; an unknown
    // name is a clear INVALID_PARAMS rather than a silent empty result (an LLM
    // client has no tab-completion to catch a typo).
    let server = server_with(search_seed);
    let err = server
        .search_notes(Parameters(SearchNotesInput {
            query: "sparse".to_owned(),
            note_type: Some("bogus".to_owned()),
            from: None,
            to: None,
            portfolio: None,
            limit: 20,
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    assert!(err.message.contains("note_type"), "msg: {}", err.message);
}

#[tokio::test]
async fn search_notes_blank_query_returns_no_results() {
    let server = server_with(search_seed);
    let result = server
        .search_notes(Parameters(SearchNotesInput {
            query: "   ".to_owned(),
            note_type: None,
            from: None,
            to: None,
            portfolio: None,
            limit: 20,
        }))
        .await
        .unwrap();
    let value = decode_json(&result);
    assert!(value.as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------
// list_projects / get_commitments / lint (#204)
// ---------------------------------------------------------------------

#[tokio::test]
async fn list_projects_splits_active_and_parked_with_slots() {
    let server = server_with(|v| {
        v.create_project(
            moment(2026, 5, 1, 9, 0).date(),
            "Alpha",
            Context::Work,
            None,
        )
        .unwrap();
        v.create_project(moment(2026, 5, 1, 9, 0).date(), "Beta", Context::Work, None)
            .unwrap();
        v.park_project(moment(2026, 5, 2, 9, 0), "beta").unwrap();
    });

    let result = server
        .list_projects(Parameters(EmptyInput {}))
        .await
        .expect("list_projects");
    let v = decode_json(&result);

    assert_eq!(v["slots"]["active"].as_u64().unwrap(), 1);
    assert_eq!(v["slots"]["cap"].as_u64().unwrap(), 5);
    let active = v["active"].as_array().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0]["slug"].as_str().unwrap(), "alpha");
    assert_eq!(
        active[0]["frontmatter"]["status"].as_str().unwrap(),
        "active"
    );
    let parked = v["parked"].as_array().unwrap();
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0]["slug"].as_str().unwrap(), "beta");
}

#[tokio::test]
async fn get_commitments_aggregates_a_standalone_commitment_due_soon() {
    // The handler uses `Local::now()` for `today` and the aggregation
    // only spans `[today - 30d, today + lookahead]`, so seed relative
    // to today rather than a fixed date.
    let today = chrono::Local::now().date_naive();
    let due = today + chrono::Duration::days(3);
    let at = today.and_hms_opt(9, 0, 0).unwrap();
    let server = server_with(move |v| {
        v.create_commitment(at, "Submit the report", due, Context::Work, None, None)
            .unwrap();
    });

    let result = server
        .get_commitments(Parameters(GetCommitmentsInput {
            lookahead_weeks: Some(2),
        }))
        .await
        .expect("get_commitments");
    let v = decode_json(&result);
    let arr = v.as_array().unwrap();
    let entry = arr
        .iter()
        .find(|c| c["title"].as_str() == Some("Submit the report"))
        .expect("the standalone commitment is aggregated");
    assert!(!entry["is_overdue"].as_bool().unwrap());
    // The commitment's own context surfaces on the wire (kebab-case).
    assert_eq!(entry["context"].as_str(), Some("work"));
}

#[tokio::test]
async fn lint_reports_a_broken_wikilink_as_a_warning() {
    let server = server_with_notes(&[(
        "journal/2026/daily/2026-05-01.md",
        "---\ntype: daily\ntitle: Day\n---\n# Day\n\nSee [[projects/ghost]] for details.\n",
    )]);

    let result = server.lint(Parameters(EmptyInput {})).await.expect("lint");
    let v = decode_json(&result);

    assert!(!v["clean"].as_bool().unwrap());
    assert_eq!(v["error_count"].as_u64().unwrap(), 0);
    assert_eq!(v["warning_count"].as_u64().unwrap(), 1);
    assert_eq!(v["issues"][0]["severity"].as_str().unwrap(), "warning");
    assert!(
        v["issues"][0]["message"]
            .as_str()
            .unwrap()
            .contains("[[projects/ghost]]")
    );
}

#[tokio::test]
async fn lint_is_clean_on_an_empty_vault() {
    let server = empty_server();
    let result = server.lint(Parameters(EmptyInput {})).await.expect("lint");
    let v = decode_json(&result);
    assert!(v["clean"].as_bool().unwrap());
    assert_eq!(v["issues"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn triage_inbox_lists_pending_captures() {
    let server = server_with(|vault| {
        vault
            .capture_to_inbox(moment(2026, 4, 26, 9, 0), "buy milk")
            .unwrap();
    });

    let result = server
        .triage_inbox(Parameters(EmptyInput {}))
        .await
        .expect("triage_inbox");
    let v = decode_json(&result);
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["text"].as_str().unwrap(), "buy milk");
    assert!(arr[0]["slug"].as_str().unwrap().starts_with("2026-04-26-"));
}
