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

const ALPHA: &str = "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Alpha\n\n## Current State\nUnderway.\n\n## Next Actions\n- [ ] Draft methods (deep)\n";

fn memory_vault() -> Vault {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    store
        .write_file(&VaultPath::new("projects/alpha.md").unwrap(), ALPHA)
        .unwrap();
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    vault
}

fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .invoke_handler(tauri::generate_handler![
            cdno_tauri::commands::orientation::get_orientation,
            cdno_tauri::commands::orientation::get_today,
        ])
        .manage(AppState {
            vault: Arc::new(memory_vault()),
            journal: WriteJournal::default(),
        })
        .build(mock_context(noop_assets()))
        .expect("mock app builds")
}

fn request(cmd: &str) -> InvokeRequest {
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
        body: InvokeBody::default(),
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
    let app = mock_app();
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
    let app = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "today", Default::default())
        .build()
        .expect("mock webview");

    let response = get_ipc_response(&webview, request("get_today")).expect("command succeeds");
    let value = response_json(response);
    assert!(value.as_str().is_some_and(|s| s.len() == 10));
}

#[test]
fn unknown_command_is_rejected() {
    let app = mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "nope", Default::default())
        .build()
        .expect("mock webview");

    let result = get_ipc_response(&webview, request("no_such_command"));
    assert!(result.is_err(), "unregistered command must error");
}
