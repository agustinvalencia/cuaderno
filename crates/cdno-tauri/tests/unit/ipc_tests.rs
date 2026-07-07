//! Real-IPC round-trips through `tauri::test`'s MockRuntime — the
//! one place argument marshalling and the serde bridge are exercised
//! for real (frontend suites run on mockIPC, where both sides are
//! authored by us; a snake_case/camelCase arg-key mismatch can only
//! surface here or in the running app). Required per the M1 design
//! review: at least one read and one write-with-args per command
//! module round-trips here as modules land.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_tauri::state::{AppState, WriteJournal};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;

const ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nUnderway.\n\n## Next Actions\n- [ ] Draft methods (deep)\n- [ ] Draft the intro (light)\n";

fn memory_vault() -> (Vault, Arc<dyn VaultStore>) {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    store
        .write_file(&VaultPath::new("projects/alpha.md").unwrap(), ALPHA)
        .unwrap();
    let (vault, _report) =
        Vault::new(Arc::clone(&store), index, VaultConfig::default()).expect("Vault::new");
    (vault, store)
}

fn mock_app() -> (tauri::App<tauri::test::MockRuntime>, Arc<dyn VaultStore>) {
    let (vault, store) = memory_vault();
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            cdno_tauri::commands::orientation::get_orientation,
            cdno_tauri::commands::orientation::get_today,
            cdno_tauri::commands::actions::start_action,
            cdno_tauri::commands::actions::complete_action,
            cdno_tauri::commands::projects::update_project_state,
            cdno_tauri::commands::capture::capture_quick,
            cdno_tauri::commands::capture::log_quick,
            cdno_tauri::commands::capture::list_inbox,
            cdno_tauri::commands::capture::discard_inbox_item,
            cdno_tauri::commands::capture::open_in_editor,
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
