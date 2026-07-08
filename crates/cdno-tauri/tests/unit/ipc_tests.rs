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
