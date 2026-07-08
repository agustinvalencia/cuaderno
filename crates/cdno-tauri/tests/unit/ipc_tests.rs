//! Real-IPC round-trips through `tauri::test`'s MockRuntime — the
//! one place argument marshalling and the serde bridge are exercised
//! for real (frontend suites run on mockIPC, where both sides are
//! authored by us; a snake_case/camelCase arg-key mismatch can only
//! surface here or in the running app). Required per the M1 design
//! review: at least one read and one write-with-args per command
//! module round-trips here as modules land.

use std::sync::Arc;

use cdno_core::config::{VaultConfig, VaultMeta};
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_tauri::state::{AppState, WriteJournal};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;

const ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nUnderway.\n\n## Next Actions\n- [ ] Draft methods (deep)\n- [ ] Draft the intro (light)\n";

/// Build a Memory-backed vault seeded with the baseline ALPHA project
/// plus `notes`, under a caller-supplied config — the cap round-trip
/// lowers `max_active_projects` so a single active project already fills
/// the cap.
fn memory_vault_configured(
    notes: &[(&str, &str)],
    config: VaultConfig,
) -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    store
        .write_file(&VaultPath::new("projects/alpha.md").unwrap(), ALPHA)
        .unwrap();
    for (path, body) in notes {
        store
            .write_file(&VaultPath::new(path).unwrap(), body)
            .unwrap();
    }
    let (vault, _report) = Vault::new(Arc::clone(&store), index, config).expect("Vault::new");
    (vault, store)
}

/// A vault config that caps the active-project count at `max` — the
/// cheapest honest way to drive `ProjectCapReached` without seeding five
/// full project fixtures.
fn config_capped_at(max: u8) -> VaultConfig {
    VaultConfig {
        vault: VaultMeta {
            name: "test-vault".to_owned(),
            max_active_projects: max,
        },
        ..VaultConfig::default()
    }
}

/// A mock app seeded with `notes` on top of the baseline ALPHA
/// project — the commitments round-trips need dated fixtures relative
/// to today, which the static ALPHA can't carry.
fn mock_app_with(
    notes: &[(&str, &str)],
) -> (tauri::App<tauri::test::MockRuntime>, Arc<dyn VaultStore>) {
    mock_app_configured(notes, VaultConfig::default())
}

/// A mock app over a config-driven vault — the cap round-trip lowers
/// `max_active_projects` so a single active project fills the slot.
fn mock_app_configured(
    notes: &[(&str, &str)],
    config: VaultConfig,
) -> (tauri::App<tauri::test::MockRuntime>, Arc<dyn VaultStore>) {
    let (vault, store) = memory_vault_configured(notes, config);
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            cdno_tauri::commands::orientation::get_orientation,
            cdno_tauri::commands::orientation::get_today,
            cdno_tauri::commands::actions::start_action,
            cdno_tauri::commands::actions::complete_action,
            cdno_tauri::commands::actions::add_action,
            cdno_tauri::commands::actions::promote_action,
            cdno_tauri::commands::actions::list_all_actions,
            cdno_tauri::commands::projects::update_project_state,
            cdno_tauri::commands::projects::get_project,
            cdno_tauri::commands::projects::add_waiting_on,
            cdno_tauri::commands::projects::resolve_waiting,
            cdno_tauri::commands::projects::park_project,
            cdno_tauri::commands::projects::activate_project,
            cdno_tauri::commands::notes::read_note,
            cdno_tauri::commands::notes::resolve_wikilink,
            cdno_tauri::commands::search::search_vault,
            cdno_tauri::commands::commitments::get_commitments,
            cdno_tauri::commands::commitments::complete_commitment,
            cdno_tauri::commands::commitments::complete_milestone,
            cdno_tauri::commands::weekly::get_weekly_bundle,
            cdno_tauri::commands::weekly::save_weekly_section,
            cdno_tauri::commands::capture::capture_quick,
            cdno_tauri::commands::capture::log_quick,
            cdno_tauri::commands::capture::list_inbox,
            cdno_tauri::commands::capture::discard_inbox_item,
            cdno_tauri::commands::capture::open_in_editor,
            cdno_tauri::commands::stewardships::list_stewardships,
            cdno_tauri::commands::stewardships::get_stewardship_detail,
            cdno_tauri::commands::stewardships::get_tracking_template_fields,
            cdno_tauri::commands::stewardships::log_tracking_entry,
            cdno_tauri::commands::portfolios::list_portfolios,
            cdno_tauri::commands::portfolios::get_portfolio,
            cdno_tauri::commands::portfolios::add_evidence,
        ])
        .manage(AppState {
            vault: Arc::new(vault),
            journal: WriteJournal::default(),
            root: std::path::PathBuf::from("/nonexistent-test-vault"),
        })
        .build(mock_context(noop_assets()))
        .expect("mock app builds");
    (app, store)
}

fn mock_app() -> (tauri::App<tauri::test::MockRuntime>, Arc<dyn VaultStore>) {
    mock_app_with(&[])
}

fn request(cmd: &str) -> InvokeRequest {
    request_with(cmd, InvokeBody::default())
}

fn request_with(cmd: &str, body: InvokeBody) -> InvokeRequest {
    InvokeRequest {
        cmd: cmd.to_owned(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        // The local-app origin differs per platform; a wrong origin
        // makes the ACL treat the call as remote and deny it.
        url: if cfg!(any(windows, target_os = "android")) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body,
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_owned(),
    }
}

fn response_json(response: InvokeResponseBody) -> serde_json::Value {
    match response {
        InvokeResponseBody::Json(raw) => serde_json::from_str(&raw).expect("valid JSON"),
        InvokeResponseBody::Raw(bytes) => serde_json::from_slice(&bytes).expect("valid JSON"),
    }
}

#[test]
fn get_orientation_round_trips_the_real_ipc_serialiser() {
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("mock webview");

    let response =
        get_ipc_response(&webview, request("get_orientation")).expect("command succeeds");
    let value = response_json(response);

    assert_eq!(value["projects"][0]["slug"], "alpha");
    assert_eq!(value["projects"][0]["context"], "work");
    assert_eq!(value["projects"][0]["actions"][0]["energy"], "deep");
    // Wire date is an ISO string (chrono serde default) — the shape
    // the ts-rs bindings promise.
    assert!(
        value["today"]
            .as_str()
            .is_some_and(|s| s.len() == 10 && s.as_bytes()[4] == b'-'),
        "today is YYYY-MM-DD, got {:?}",
        value["today"]
    );
}

#[test]
fn get_today_round_trips() {
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "today", Default::default())
        .build()
        .expect("mock webview");

    let response = get_ipc_response(&webview, request("get_today")).expect("command succeeds");
    let value = response_json(response);
    assert!(value.as_str().is_some_and(|s| s.len() == 10));
}

#[test]
fn unknown_command_is_rejected() {
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "nope", Default::default())
        .build()
        .expect("mock webview");

    let result = get_ipc_response(&webview, request("no_such_command"));
    assert!(result.is_err(), "unregistered command must error");
}

#[test]
fn start_action_round_trips_args_and_writes_the_daily_note() {
    let (app, store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-start", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "project": "alpha",
        "action": "Draft methods",
    }));
    get_ipc_response(&webview, request_with("start_action", body)).expect("command succeeds");

    let daily = cdno_tauri::commands::actions::daily_path_for(chrono::Local::now().date_naive());
    let content = store.read_file(&daily).expect("daily note written");
    assert!(
        content.contains("started [[alpha]] \u{2014} Draft methods"),
        "daily carries the started line: {content}"
    );
}

#[test]
fn update_project_state_round_trips_camel_cased_args() {
    // `new_state` in Rust is `newState` on the wire — the exact
    // marshalling mismatch these tests exist to catch.
    let (app, store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-state", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "project": "alpha",
        "newState": "Rewired and humming.",
    }));
    get_ipc_response(&webview, request_with("update_project_state", body))
        .expect("command succeeds");

    let content = store
        .read_file(&VaultPath::new("projects/alpha.md").unwrap())
        .unwrap();
    assert!(content.contains("Rewired and humming."), "{content}");
}

#[test]
fn capture_quick_round_trips_args_and_writes_the_inbox_note() {
    let (app, store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-capture", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "text": "buy milk" }));
    get_ipc_response(&webview, request_with("capture_quick", body)).expect("command succeeds");

    // Filename layout is `inbox/<YYYY-MM-DD>-<slug>.md`; the slug of
    // "buy milk" is "buy-milk".
    let today = chrono::Local::now().date_naive().format("%Y-%m-%d");
    let path = VaultPath::new(format!("inbox/{today}-buy-milk.md")).unwrap();
    assert!(
        store.exists(&path).expect("store query"),
        "capture wrote the inbox note at {path}"
    );
    let content = store.read_file(&path).expect("inbox note readable");
    assert!(
        content.contains("buy milk"),
        "inbox note carries the text: {content}"
    );
}

#[test]
fn log_quick_round_trips_args_and_appends_to_the_daily() {
    let (app, store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-log", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "text": "a passing thought" }));
    get_ipc_response(&webview, request_with("log_quick", body)).expect("command succeeds");

    let daily = cdno_tauri::commands::actions::daily_path_for(chrono::Local::now().date_naive());
    let content = store.read_file(&daily).expect("daily note written");
    assert!(
        content.contains("a passing thought"),
        "daily carries the logged line: {content}"
    );
}

#[test]
fn list_inbox_returns_the_captured_item() {
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-list-inbox", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "text": "triage me" }));
    get_ipc_response(&webview, request_with("capture_quick", body)).expect("capture succeeds");

    let response = get_ipc_response(&webview, request("list_inbox")).expect("command succeeds");
    let value = response_json(response);
    let items = value.as_array().expect("list_inbox returns an array");
    assert_eq!(items.len(), 1, "the one capture is listed: {value}");
    assert_eq!(items[0]["text"], "triage me");
    assert!(
        items[0]["slug"]
            .as_str()
            .is_some_and(|s| s.ends_with("triage-me")),
        "slug carries the date-prefixed stem: {value}"
    );
}

#[test]
fn discard_inbox_item_round_trips_and_logs_the_discard() {
    let (app, store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-discard", Default::default())
        .build()
        .expect("mock webview");

    // Capture first so there's a real inbox note (with valid inbox
    // frontmatter) and index row for discard to find and delete.
    let body = InvokeBody::Json(serde_json::json!({ "text": "throwaway idea" }));
    get_ipc_response(&webview, request_with("capture_quick", body)).expect("capture succeeds");

    let today = chrono::Local::now().date_naive().format("%Y-%m-%d");
    let slug = format!("{today}-throwaway-idea");
    let note = VaultPath::new(format!("inbox/{slug}.md")).unwrap();
    assert!(store.exists(&note).expect("store query"), "capture landed");

    let body = InvokeBody::Json(serde_json::json!({ "slug": slug }));
    get_ipc_response(&webview, request_with("discard_inbox_item", body)).expect("discard succeeds");

    assert!(
        !store.exists(&note).expect("store query"),
        "discard hard-deletes the inbox note"
    );
    // The domain preserves the text on today's daily as a discard line
    // (see capture.rs: "-- discarded: <text>"), so the capture stays
    // recoverable from the append-only daily.
    let daily = cdno_tauri::commands::actions::daily_path_for(chrono::Local::now().date_naive());
    let content = store.read_file(&daily).expect("daily note written");
    assert!(
        content.contains("discarded: throwaway idea"),
        "daily carries the discard line: {content}"
    );
}

#[test]
fn open_in_editor_rejects_a_path_escape() {
    // The lexical VaultPath guard fires on the `..` components before
    // the symlink-canonical layer is reached, so an escape attempt
    // rejects as `invalid` — and this round-trips the command's args.
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-open", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "path": "../../etc/passwd" }));
    let err = get_ipc_response(&webview, request_with("open_in_editor", body))
        .expect_err("a path escape must be refused");

    assert_eq!(err["kind"], "invalid", "{err}");
    assert!(err["data"].is_string());
}

#[test]
fn complete_action_error_path_serialises_the_cmd_error_contract() {
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-err", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "project": "alpha",
        "action": "no such bullet anywhere",
    }));
    let err = get_ipc_response(&webview, request_with("complete_action", body))
        .expect_err("no matching bullet must fail");

    // The rejected value is the serialised CmdError — the shape
    // commands.ts pattern-matches on.
    assert_eq!(err["kind"], "not_found", "{err}");
    assert!(err["data"].is_string());
}

#[test]
fn complete_action_ambiguous_serialises_candidates() {
    // "Draft" matches both Next Actions bullets (case-insensitive
    // substring), so the domain returns AmbiguousAction, which the
    // command maps to CmdError::Ambiguous — the picker the UI renders.
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-ambig", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "project": "alpha",
        "action": "Draft",
    }));
    let err = get_ipc_response(&webview, request_with("complete_action", body))
        .expect_err("an ambiguous match must fail");

    assert_eq!(err["kind"], "ambiguous", "{err}");
    let candidates = err["data"]["candidates"]
        .as_array()
        .expect("candidates is an array");
    assert_eq!(
        candidates.len(),
        2,
        "both Draft bullets are candidates: {err}"
    );
}

// ---------------------------------------------------------------------
// Commitments Timeline (M4, #56). The aggregation stamps `today` from
// Local::now(), so these fixtures are dated relative to today rather
// than a fixed date.
// ---------------------------------------------------------------------

fn project_with_hard_milestone(
    slug_title: &str,
    milestone: &str,
    due: chrono::NaiveDate,
) -> String {
    format!(
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# {slug_title}\n\n## Milestones\n- [ ] {milestone} \u{2014} hard: {due}\n\n## Next Actions\n",
        due = due.format("%Y-%m-%d"),
    )
}

fn standalone_commitment_note(due: chrono::NaiveDate) -> String {
    format!(
        "---\ntype: commitment\nstatus: active\ndue: {due}\ncreated: 2026-05-01\ncompleted: null\ncontext: personal\n---\n\n# Renew passport\n",
        due = due.format("%Y-%m-%d"),
    )
}

#[test]
fn get_commitments_round_trips_the_camel_cased_arg_and_both_sources() {
    // `lookahead_days` in Rust is `lookaheadDays` on the wire — the
    // camelCase seam commands.ts pins. A 30-day window covers both
    // fixtures below.
    let today = chrono::Local::now().date_naive();
    let milestone_due = today + chrono::Duration::days(5);
    let commitment_due = today + chrono::Duration::days(7);
    let (app, _store) = mock_app_with(&[
        (
            "projects/gamma.md",
            &project_with_hard_milestone("Gamma", "Ship v1", milestone_due),
        ),
        (
            "commitments/renew-passport.md",
            &standalone_commitment_note(commitment_due),
        ),
    ]);
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-commitments", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "lookaheadDays": 30 }));
    let response = get_ipc_response(&webview, request_with("get_commitments", body))
        .expect("command succeeds");
    let value = response_json(response);

    // `today` is stamped in Rust and rides the wire as an ISO string.
    assert_eq!(
        value["today"].as_str(),
        Some(today.format("%Y-%m-%d").to_string().as_str()),
        "today is stamped for the frontend: {value}"
    );

    let entries = value["entries"].as_array().expect("entries is an array");
    let milestone = entries
        .iter()
        .find(|e| e["title"] == "Ship v1")
        .expect("the hard milestone is aggregated");
    assert_eq!(milestone["source"]["kind"], "project_milestone");
    assert_eq!(milestone["source"]["slug"], "gamma");
    assert_eq!(milestone["context"], "work");

    let standalone = entries
        .iter()
        .find(|e| e["title"] == "Renew passport")
        .expect("the standalone commitment is aggregated");
    // The slug now rides the wire so the done button can complete it.
    assert_eq!(standalone["source"]["kind"], "standalone_commitment");
    assert_eq!(standalone["source"]["slug"], "renew-passport");
    assert_eq!(standalone["context"], "personal");
}

#[test]
fn complete_commitment_round_trips_args_and_moves_the_note_to_done() {
    use chrono::Datelike;

    let today = chrono::Local::now().date_naive();
    let (app, store) = mock_app_with(&[(
        "commitments/renew-passport.md",
        &standalone_commitment_note(today + chrono::Duration::days(7)),
    )]);
    let webview =
        tauri::WebviewWindowBuilder::new(&app, "w-complete-commitment", Default::default())
            .build()
            .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "slug": "renew-passport" }));
    get_ipc_response(&webview, request_with("complete_commitment", body))
        .expect("command succeeds");

    let active = VaultPath::new("commitments/renew-passport.md").unwrap();
    assert!(
        !store.exists(&active).expect("store query"),
        "the active commitment is moved away"
    );
    // The domain files it under `_done/<completion year>/`.
    let done = VaultPath::new(format!(
        "commitments/_done/{}/renew-passport.md",
        today.year()
    ))
    .unwrap();
    assert!(
        store.exists(&done).expect("store query"),
        "the completed commitment lands under _done/<year>/"
    );
}

// ---------------------------------------------------------------------
// Detail / reader / palette (M5). Reads and the Project-Detail writes.
// ---------------------------------------------------------------------

// A richer project than ALPHA: actions, a hard milestone, and a
// waiting-on item — the Project Detail fixture.
const DETAIL: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Detail\n\n## Current State\nMoving.\n\n## Next Actions\n- [ ] Wire the reader (deep)\n- [ ] Tidy imports (light)\n\n## Waiting On\n- Review from Sam\n\n## Milestones\n- [ ] Cut release \u{2014} hard: 2026-08-01\n";

#[test]
fn read_note_round_trips_the_path_arg_and_returns_the_note() {
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-read-note", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "path": "projects/alpha.md" }));
    let response =
        get_ipc_response(&webview, request_with("read_note", body)).expect("command succeeds");
    let value = response_json(response);

    assert_eq!(value["path"], "projects/alpha.md");
    assert_eq!(value["note_type"], "project");
    assert!(
        value["body"]
            .as_str()
            .is_some_and(|b| b.contains("## Next Actions")),
        "body carries the markdown: {value}"
    );
}

#[test]
fn resolve_wikilink_round_trips_a_resolved_target() {
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-resolve", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "target": "alpha" }));
    let response = get_ipc_response(&webview, request_with("resolve_wikilink", body))
        .expect("command succeeds");
    let value = response_json(response);

    assert_eq!(value["path"], "projects/alpha.md");
    assert_eq!(value["note_type"], "project");
}

#[test]
fn resolve_wikilink_returns_null_for_an_unresolved_target() {
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-resolve-none", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "target": "no-such-note" }));
    let response = get_ipc_response(&webview, request_with("resolve_wikilink", body))
        .expect("command succeeds");
    let value = response_json(response);

    assert!(
        value.is_null(),
        "an unresolved target rides the wire as null: {value}"
    );
}

#[test]
fn get_project_round_trips_and_composes_the_detail_view() {
    let (app, _store) = mock_app_with(&[("projects/detail.md", DETAIL)]);
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-get-project", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "slug": "detail" }));
    let response =
        get_ipc_response(&webview, request_with("get_project", body)).expect("command succeeds");
    let value = response_json(response);

    assert_eq!(value["slug"], "detail");
    assert_eq!(value["status"], "active");
    assert_eq!(value["context"], "work");
    let actions = value["actions"].as_array().expect("actions is an array");
    assert_eq!(actions.len(), 2, "both open bullets: {value}");
    let milestones = value["open_milestones"]
        .as_array()
        .expect("open_milestones is an array");
    assert_eq!(milestones.len(), 1);
    assert_eq!(milestones[0]["name"], "Cut release");
    assert_eq!(milestones[0]["is_hard"], true);
}

// ---------------------------------------------------------------------
// Weekly Review (M6, #55). One composed read and the section write.
// ---------------------------------------------------------------------

// A completed action note dated inside the reviewed week (Mon
// 2026-07-06 .. Sun 2026-07-12) — the wins source get_weekly_bundle
// aggregates.
const DONE_ACTION: &str = "---\ntype: action\nstatus: completed\nproject: alpha\nenergy: deep\nmilestone: null\ndue: null\ncreated: 2026-07-01\ncompleted: 2026-07-08\nblocker: null\ncriteria: |\n  Reader wired.\n---\n\n# Wire the reader\n";

#[test]
fn save_weekly_section_round_trips_args_and_writes_the_section() {
    // `week_of` in Rust is `weekOf` on the wire — the camelCase seam
    // commands.ts pins, exercised here for real.
    let (app, store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-save-weekly", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "weekOf": "2026-07-08",
        "section": "wins",
        "content": "We shipped M6.",
    }));
    get_ipc_response(&webview, request_with("save_weekly_section", body))
        .expect("command succeeds");

    // The note is keyed by ISO week, so any day resolves to the same
    // file; the section lands under its `## Wins` heading.
    let weekly = VaultPath::new(cdno_core::paths::weekly_note_relpath(
        chrono::NaiveDate::from_ymd_opt(2026, 7, 8).unwrap(),
    ))
    .unwrap();
    let content = store.read_file(&weekly).expect("weekly note written");
    assert!(
        content.contains("## Wins") && content.contains("We shipped M6."),
        "the Wins section carries the composed content: {content}"
    );
}

#[test]
fn save_weekly_section_round_trips_the_kebab_goal_section() {
    // "this-weeks-goal" is the multi-word kebab wire string FocusStep
    // actually sends — the tolerant parser must map it (hyphens to
    // spaces, apostrophe dropped) onto WeeklySection::ThisWeeksGoal.
    let (app, store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-save-goal", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "weekOf": "2026-07-13",
        "section": "this-weeks-goal",
        "content": "Start M7.",
    }));
    get_ipc_response(&webview, request_with("save_weekly_section", body))
        .expect("command succeeds");

    let weekly = VaultPath::new(cdno_core::paths::weekly_note_relpath(
        chrono::NaiveDate::from_ymd_opt(2026, 7, 13).unwrap(),
    ))
    .unwrap();
    let content = store.read_file(&weekly).expect("weekly note written");
    assert!(
        content.contains("## This Week's Goal") && content.contains("Start M7."),
        "the goal section carries the focus: {content}"
    );
}

#[test]
fn save_weekly_section_rejects_an_unknown_section() {
    // The section string is parsed into WeeklySection; a bad value is a
    // user-visible Invalid whose message names the valid sections.
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-save-weekly-bad", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "weekOf": "2026-07-08",
        "section": "nonsense",
        "content": "x",
    }));
    let err = get_ipc_response(&webview, request_with("save_weekly_section", body))
        .expect_err("an unknown section must fail");
    assert_eq!(err["kind"], "invalid", "{err}");
    assert!(err["data"].is_string());
}

#[test]
fn get_weekly_bundle_round_trips_and_composes_the_review() {
    let (app, _store) = mock_app_with(&[("actions/wire-reader.md", DONE_ACTION)]);
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-get-weekly", Default::default())
        .build()
        .expect("mock webview");

    // Seed the week's Wins first so the bundle carries existing content.
    let save = InvokeBody::Json(serde_json::json!({
        "weekOf": "2026-07-08",
        "section": "wins",
        "content": "We shipped M6.",
    }));
    get_ipc_response(&webview, request_with("save_weekly_section", save)).expect("seed succeeds");

    let body = InvokeBody::Json(serde_json::json!({ "weekOf": "2026-07-08" }));
    let response = get_ipc_response(&webview, request_with("get_weekly_bundle", body))
        .expect("command succeeds");
    let value = response_json(response);

    // The anchor normalises to the Monday of the ISO week and rides the
    // wire as an ISO string.
    assert_eq!(value["week_of"], "2026-07-06", "{value}");
    // Existing section content is parsed and carried.
    assert_eq!(value["weekly"]["wins"], "We shipped M6.", "{value}");
    assert_eq!(value["weekly"]["exists"], true);

    // The completed action inside the week is a wins source.
    let completed = value["completed_actions"]
        .as_array()
        .expect("completed_actions is an array");
    assert!(
        completed.iter().any(|c| c["title"] == "Wire the reader"),
        "the week's completed action is aggregated: {value}"
    );

    // The baseline ALPHA project shows in the step-2 scan.
    let projects = value["projects"].as_array().expect("projects is an array");
    assert!(
        projects.iter().any(|p| p["slug"] == "alpha"),
        "the active project shows in the scan: {value}"
    );
}

#[test]
fn search_vault_round_trips_the_query_arg() {
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-search", Default::default())
        .build()
        .expect("mock webview");

    // ALPHA's body carries "Draft methods" — a term-based match.
    let body = InvokeBody::Json(serde_json::json!({ "query": "methods" }));
    let response =
        get_ipc_response(&webview, request_with("search_vault", body)).expect("command succeeds");
    let value = response_json(response);

    let results = value.as_array().expect("search_vault returns an array");
    assert!(
        results.iter().any(|r| r["path"] == "projects/alpha.md"),
        "the alpha project is a hit for 'methods': {value}"
    );
}

#[test]
fn add_action_round_trips_args_including_the_energy_string() {
    let (app, store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-add-action", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "project": "alpha",
        "action": "Wire the palette",
        "energy": "deep",
    }));
    get_ipc_response(&webview, request_with("add_action", body)).expect("command succeeds");

    let content = store
        .read_file(&VaultPath::new("projects/alpha.md").unwrap())
        .unwrap();
    assert!(
        content.contains("- [ ] Wire the palette (deep)"),
        "the new bullet lands with its energy suffix: {content}"
    );
}

#[test]
fn add_action_rejects_an_unknown_energy_string() {
    // The energy string is parsed into EnergyLevel; a bad value is a
    // user-visible Invalid, never a silent default.
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-add-action-bad", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "project": "alpha",
        "action": "Something",
        "energy": "turbo",
    }));
    let err = get_ipc_response(&webview, request_with("add_action", body))
        .expect_err("an unknown energy must fail");
    assert_eq!(err["kind"], "invalid", "{err}");
}

#[test]
fn park_project_round_trips_and_moves_the_map_to_parked() {
    let (app, store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-park", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "slug": "alpha" }));
    get_ipc_response(&webview, request_with("park_project", body)).expect("command succeeds");

    let active = VaultPath::new("projects/alpha.md").unwrap();
    let parked = VaultPath::new("projects/_parked/alpha.md").unwrap();
    assert!(
        !store.exists(&active).expect("store query"),
        "the active map is moved away"
    );
    assert!(
        store.exists(&parked).expect("store query"),
        "the map lands under projects/_parked/"
    );
}

#[test]
fn add_waiting_on_round_trips_args_and_appends_to_the_map() {
    let (app, store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-add-waiting", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "project": "alpha",
        "item": "sign-off from Legal",
    }));
    get_ipc_response(&webview, request_with("add_waiting_on", body)).expect("command succeeds");

    let content = store
        .read_file(&VaultPath::new("projects/alpha.md").unwrap())
        .unwrap();
    assert!(
        content.contains("sign-off from Legal"),
        "the waiting-on item lands on the map: {content}"
    );
}

#[test]
fn activate_project_at_cap_serialises_the_project_cap_reached_contract() {
    // ALPHA is already active; with the cap lowered to one, activating a
    // parked project must fail with the structured ProjectCapReached the
    // allocator modal keys on — kind "project_cap_reached", data.active
    // naming the blocking projects — rather than a generic error. A
    // cap-of-one vault is the cheapest honest way to hit the cap (one
    // active fixture instead of five).
    const PARKED: &str = "---\ntype: project\ncontext: personal\nstatus: parked\ncreated: 2026-03-01\n---\n\n# Beta\n\n## Current State\nOn ice.\n";
    let (app, _store) =
        mock_app_configured(&[("projects/_parked/beta.md", PARKED)], config_capped_at(1));
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-activate-cap", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "slug": "beta" }));
    let err = get_ipc_response(&webview, request_with("activate_project", body))
        .expect_err("activating past the cap must fail");

    assert_eq!(err["kind"], "project_cap_reached", "{err}");
    let active = err["data"]["active"]
        .as_array()
        .expect("active is an array of slugs");
    assert!(
        active.iter().any(|s| s == "alpha"),
        "the blocking active project is named: {err}"
    );
    assert_eq!(err["data"]["max"], 1, "the cap rides the wire: {err}");
}

#[test]
fn promote_action_missing_bullet_serialises_not_found() {
    let (app, _store) = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-promote-err", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "project": "alpha",
        "action": "no such bullet anywhere",
    }));
    let err = get_ipc_response(&webview, request_with("promote_action", body))
        .expect_err("no matching bullet must fail");
    assert_eq!(err["kind"], "not_found", "{err}");
}

// ---------------------------------------------------------------------
// Stewardship views (M7, #59). The list, the composed detail (expanded
// fixture with tracking + a table so the series is non-empty), the log
// write (happy path + the flat-stewardship error), and the
// template-field discovery.
// ---------------------------------------------------------------------

// An expanded stewardship folder with one tracking note carrying a body
// table — enough for a non-empty trend series in the detail round-trip.
const HEALTH_INDEX: &str = "---\ntype: stewardship\ncontext: personal\n---\n\n# Health\n\n## Current Status\nConsistent.\n";
const GYM_ENTRY: &str = "---\ntype: tracking\nstewardship: health\nactivity: gym\ndate: 2026-07-01\nduration_min: 60\nroutine: null\n---\n\n# Gym\n\n| Sets | Reps |\n|------|------|\n| 3 | 5 |\n\n## Notes\nSolid.\n";
// A flat stewardship: tracking on it must fail as Invalid.
const FINANCES: &str =
    "---\ntype: stewardship\ncontext: household\n---\n\n# Finances\n\n## Current Status\nSteady.\n";

#[test]
fn list_stewardships_round_trips_and_stamps_the_variant() {
    let (app, _store) = mock_app_with(&[
        ("stewardships/health/_index.md", HEALTH_INDEX),
        ("stewardships/health/tracking/2026-07-01-gym.md", GYM_ENTRY),
    ]);
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-list-stew", Default::default())
        .build()
        .expect("mock webview");

    let response =
        get_ipc_response(&webview, request("list_stewardships")).expect("command succeeds");
    let value = response_json(response);
    let rows = value.as_array().expect("list is an array");
    let health = rows
        .iter()
        .find(|s| s["slug"] == "health")
        .expect("the health stewardship is listed");
    assert_eq!(health["variant"], "expanded");
    assert_eq!(health["tracking_count"], 1);
    assert_eq!(health["context"], "personal");
}

#[test]
fn get_stewardship_detail_round_trips_series_and_recent() {
    let (app, _store) = mock_app_with(&[
        ("stewardships/health/_index.md", HEALTH_INDEX),
        ("stewardships/health/tracking/2026-07-01-gym.md", GYM_ENTRY),
    ]);
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-stew-detail", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "slug": "health" }));
    let response = get_ipc_response(&webview, request_with("get_stewardship_detail", body))
        .expect("command succeeds");
    let value = response_json(response);

    assert_eq!(value["slug"], "health");
    assert_eq!(value["name"], "Health");
    assert_eq!(value["variant"], "expanded");
    assert_eq!(value["tracking_count"], 1);
    let series = value["series"].as_array().expect("series is an array");
    assert!(
        !series.is_empty(),
        "the table yields at least one series: {value}"
    );
    let recent = value["recent"].as_array().expect("recent is an array");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0]["activity"], "gym");
    assert_eq!(
        recent[0]["path"],
        "stewardships/health/tracking/2026-07-01-gym.md"
    );
}

#[test]
fn log_tracking_entry_round_trips_args_and_writes_the_note() {
    let (app, store) = mock_app_with(&[("stewardships/health/_index.md", HEALTH_INDEX)]);
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-log-track", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "stewardship": "health",
        "activity": "gym",
        "routine": null,
        "content": "Great session.",
        "vars": {},
    }));
    get_ipc_response(&webview, request_with("log_tracking_entry", body)).expect("command succeeds");

    let today = chrono::Local::now().date_naive().format("%Y-%m-%d");
    let path = VaultPath::new(format!("stewardships/health/tracking/{today}-gym.md")).unwrap();
    assert!(
        store.exists(&path).expect("store query"),
        "the tracking note lands under the stewardship's tracking/ subdir"
    );
    let content = store.read_file(&path).expect("tracking note readable");
    assert!(content.contains("Great session."), "{content}");
}

#[test]
fn log_tracking_entry_on_a_flat_stewardship_serialises_invalid() {
    // Flat stewardships have no tracking/ subdir — the domain refuses
    // with TrackingOnFlatStewardship, mapped to a user-fixable Invalid.
    let (app, _store) = mock_app_with(&[("stewardships/finances.md", FINANCES)]);
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-log-flat", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "stewardship": "finances",
        "activity": "audit",
        "routine": null,
        "content": "",
        "vars": {},
    }));
    let err = get_ipc_response(&webview, request_with("log_tracking_entry", body))
        .expect_err("tracking on a flat stewardship must fail");
    assert_eq!(err["kind"], "invalid", "{err}");
    assert!(err["data"].is_string());
}

#[test]
fn get_tracking_template_fields_round_trips_the_generic_empty_set() {
    // No custom template and no prompt vars → the generic tracking
    // template carries no prompts, so the fields list is empty.
    let (app, _store) = mock_app_with(&[("stewardships/health/_index.md", HEALTH_INDEX)]);
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-track-fields", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "activity": "gym" }));
    let response = get_ipc_response(&webview, request_with("get_tracking_template_fields", body))
        .expect("command succeeds");
    let value = response_json(response);
    let fields = value.as_array().expect("fields is an array");
    assert!(
        fields.is_empty(),
        "the generic template has no prompts: {value}"
    );
}

#[test]
fn complete_milestone_ambiguous_serialises_candidates() {
    // Two milestones sharing the "Draft" substring make the query
    // ambiguous — the picker the UI renders (kind "ambiguous").
    let today = chrono::Local::now().date_naive();
    let project = format!(
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Delta\n\n## Milestones\n- [ ] Draft chapter one \u{2014} hard: {}\n- [ ] Draft chapter two \u{2014} hard: {}\n\n## Next Actions\n",
        (today + chrono::Duration::days(5)).format("%Y-%m-%d"),
        (today + chrono::Duration::days(6)).format("%Y-%m-%d"),
    );
    let (app, _store) = mock_app_with(&[("projects/delta.md", &project)]);
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-milestone-ambig", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "project": "delta",
        "milestone": "Draft",
    }));
    let err = get_ipc_response(&webview, request_with("complete_milestone", body))
        .expect_err("an ambiguous milestone must fail");

    assert_eq!(err["kind"], "ambiguous", "{err}");
    let candidates = err["data"]["candidates"]
        .as_array()
        .expect("candidates is an array");
    assert_eq!(
        candidates.len(),
        2,
        "both Draft milestones are candidates: {err}"
    );
}

// ---------------------------------------------------------------------
// Portfolio Browser (M8, #58). The selector list, the composed detail
// (frontmatter question + project, body-linked questions, evidence rows
// newest-first), and the quick-add write — the happy path (origin
// resolves to the seeded ALPHA project) plus the invalid-origin
// tightening the GUI adds over the MCP tool.
// ---------------------------------------------------------------------

// A portfolio folder: an `_index.md` linking the baseline ALPHA project
// (frontmatter) and a research question (body `## Related Questions`),
// plus one evidence note whose `origin` points at ALPHA.
const SURROGATE_INDEX: &str = "---\ntype: portfolio\nquestion: How does the surrogate behave?\ncreated: 2026-06-01\nproject: \"[[projects/alpha]]\"\n---\n\n# How does the surrogate behave?\n\n## Related Questions\n- [[questions/research/surrogate-fidelity]]\n\n## Evidence\n";
const SURROGATE_EVIDENCE: &str = "---\ntype: evidence\ncreated: 2026-07-01\nsource: Smith 2024\nportfolio: surrogate\norigin: \"[[projects/alpha]]\"\n---\n\n# Smith 2024\n\nThe error stayed bounded.\n";

fn portfolio_fixture() -> Vec<(&'static str, &'static str)> {
    vec![
        ("portfolios/surrogate/_index.md", SURROGATE_INDEX),
        (
            "portfolios/surrogate/2026-07-01-smith-2024.md",
            SURROGATE_EVIDENCE,
        ),
    ]
}

#[test]
fn list_portfolios_round_trips_with_evidence_count_and_staleness() {
    let (app, _store) = mock_app_with(&portfolio_fixture());
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-list-portfolios", Default::default())
        .build()
        .expect("mock webview");

    let response =
        get_ipc_response(&webview, request("list_portfolios")).expect("command succeeds");
    let value = response_json(response);
    let rows = value.as_array().expect("list is an array");
    let surrogate = rows
        .iter()
        .find(|p| p["slug"] == "surrogate")
        .expect("the surrogate portfolio is listed");
    assert_eq!(surrogate["question"], "How does the surrogate behave?");
    assert_eq!(surrogate["evidence_count"], 1);
    assert_eq!(surrogate["last_updated"], "2026-07-01");
    // staleness_days is stamped in Rust against today — a plain integer.
    assert!(surrogate["staleness_days"].is_number(), "{surrogate}");
}

#[test]
fn get_portfolio_round_trips_links_and_evidence_rows() {
    let (app, _store) = mock_app_with(&portfolio_fixture());
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-get-portfolio", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({ "slug": "surrogate" }));
    let response =
        get_ipc_response(&webview, request_with("get_portfolio", body)).expect("command succeeds");
    let value = response_json(response);

    assert_eq!(value["slug"], "surrogate");
    assert_eq!(value["question"], "How does the surrogate behave?");
    // Frontmatter `project` wikilink lowered to a bare navigable target.
    assert_eq!(value["project"], "projects/alpha");
    // The body's `## Related Questions` link is surfaced for the sidebar.
    let questions = value["questions"]
        .as_array()
        .expect("questions is an array");
    assert_eq!(
        questions,
        &[serde_json::json!("questions/research/surrogate-fidelity")]
    );
    // One evidence row, origin stripped to a bare target the UI resolves.
    let evidence = value["evidence"].as_array().expect("evidence is an array");
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0]["source"], "Smith 2024");
    assert_eq!(evidence[0]["created"], "2026-07-01");
    assert_eq!(evidence[0]["origin"], "projects/alpha");
    assert_eq!(
        evidence[0]["path"],
        "portfolios/surrogate/2026-07-01-smith-2024.md"
    );
}

#[test]
fn add_evidence_round_trips_args_and_files_the_note() {
    let (app, store) = mock_app_with(&portfolio_fixture());
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-add-evidence", Default::default())
        .build()
        .expect("mock webview");

    // origin "projects/alpha" resolves to the seeded baseline project.
    let body = InvokeBody::Json(serde_json::json!({
        "portfolio": "surrogate",
        "source": "Lab notebook p.12",
        "origin": "projects/alpha",
        "content": "Reran the sweep; matches Smith.",
    }));
    get_ipc_response(&webview, request_with("add_evidence", body)).expect("command succeeds");

    let today = chrono::Local::now().date_naive().format("%Y-%m-%d");
    let path =
        VaultPath::new(format!("portfolios/surrogate/{today}-lab-notebook-p-12.md")).unwrap();
    assert!(
        store.exists(&path).expect("store query"),
        "the evidence note lands inside the portfolio folder"
    );
    let content = store.read_file(&path).expect("evidence note readable");
    assert!(content.contains("Reran the sweep"), "{content}");
    // The origin was wrapped back into a wikilink by the domain.
    assert!(content.contains("[[projects/alpha]]"), "{content}");
}

#[test]
fn add_evidence_with_unresolvable_origin_serialises_invalid() {
    // The GUI tightening: an origin naming no note is refused before the
    // write, so a dangling link can never be persisted from the composer.
    let (app, store) = mock_app_with(&portfolio_fixture());
    let webview = tauri::WebviewWindowBuilder::new(&app, "w-bad-origin", Default::default())
        .build()
        .expect("mock webview");

    let body = InvokeBody::Json(serde_json::json!({
        "portfolio": "surrogate",
        "source": "Stray thought",
        "origin": "projects/does-not-exist",
        "content": "…",
    }));
    let err = get_ipc_response(&webview, request_with("add_evidence", body))
        .expect_err("an unresolvable origin must fail");
    assert_eq!(err["kind"], "invalid", "{err}");
    assert!(
        err["data"].as_str().is_some_and(|s| s.contains("origin")),
        "{err}"
    );

    // No evidence note was written for the rejected origin.
    let today = chrono::Local::now().date_naive().format("%Y-%m-%d");
    let path = VaultPath::new(format!("portfolios/surrogate/{today}-stray-thought.md")).unwrap();
    assert!(
        !store.exists(&path).expect("store query"),
        "nothing is filed when the origin is refused"
    );
}
