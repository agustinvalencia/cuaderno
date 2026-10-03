//! The focus on a write payload is read in the same `with_vault` closure as
//! the write's verification (RFC 0005 §5.5, #736).
//!
//! A unit test rather than an integration target because it counts
//! [`CuadernoServer::with_vault`] entries through a `cfg(test)`-only hook
//! (`server::WITH_VAULT_CALLS`), which an integration test cannot see.

use std::sync::Arc;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorData};

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_domain::frontmatter::{Context, EnergyLevel};

use crate::input::*;
use crate::server::{CuadernoServer, WITH_VAULT_CALLS};

fn today_at(h: u32, m: u32) -> NaiveDateTime {
    chrono::Local::now()
        .date_naive()
        .and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap())
}

fn may_first() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 5, 1)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap())
}

const PROJECT: &str = "---\ntype: project\ncontext: work\nstatus: {status}\ncreated: 2026-01-01\ncore_question: null\nclosed: null\n---\n\n# {title}\n\n## Next Actions\n";

/// `surrogate-model` with three open bullets and an open pause on `Run
/// ablation`; `side-quest` active with nothing open; `old-idea` parked.
/// With `focused`, `Draft methods section` is in focus as well.
fn server(focused: bool) -> CuadernoServer {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _) = Vault::new(Arc::clone(&store), index, VaultConfig::default()).unwrap();
    vault
        .create_project(may_first(), "Surrogate model", Context::Work, None)
        .unwrap();
    for title in ["Draft methods section", "Run ablation", "Write abstract"] {
        vault
            .add_action(may_first(), "surrogate-model", title, EnergyLevel::Deep)
            .unwrap();
    }
    vault
        .start_action(today_at(8, 0), "surrogate-model", "Run ablation")
        .unwrap();
    vault.pause_action(today_at(8, 30), None, None).unwrap();
    if focused {
        vault
            .start_action(today_at(9, 0), "surrogate-model", "Draft methods")
            .unwrap();
    }
    for (path, status, title) in [
        ("projects/side-quest.md", "active", "Side quest"),
        ("projects/_parked/old-idea.md", "parked", "Old idea"),
    ] {
        let body = PROJECT
            .replace("{status}", status)
            .replace("{title}", title);
        store
            .write_file(&VaultPath::new(path).unwrap(), &body)
            .unwrap();
    }
    CuadernoServer::new(Arc::new(vault))
}

async fn call(server: &CuadernoServer, tool: &str) -> Result<CallToolResult, ErrorData> {
    let project = || "surrogate-model".to_owned();
    let slug = |s: &str| {
        Parameters(ProjectSlugInput {
            project: s.to_owned(),
        })
    };
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
                    project: project(),
                    query: "Write abstract".to_owned(),
                    vars: None,
                }))
                .await
        }
        "add_action" => {
            server
                .add_action(Parameters(AddActionInput {
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
        "park_project" => server.park_project(slug("side-quest")).await,
        "activate_project" => server.activate_project(slug("old-idea")).await,
        "complete_project" => server.complete_project(slug("side-quest")).await,
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
        other => panic!("not a focused write: {other}"),
    }
}

/// Each of the seventeen tools enters `with_vault` exactly twice: once for
/// the domain write, once for the verification **and** the focus. A focus
/// read in a closure of its own would be a third `spawn_blocking`, reading
/// a focus another process may have changed in between.
#[tokio::test]
async fn focus_is_read_in_the_same_closure() {
    let tools = [
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
    ];
    for tool in tools {
        // Only a pause needs something in focus; a start or a resume
        // needs nothing in it.
        let server = server(tool == "pause_action");
        WITH_VAULT_CALLS.with(|calls| calls.set(0));
        let result = call(&server, tool)
            .await
            .unwrap_or_else(|e| panic!("{tool} failed: {e:?}"));
        assert_eq!(result.is_error, Some(false), "{tool}: {result:?}");
        let calls = WITH_VAULT_CALLS.with(|calls| calls.get());
        assert_eq!(
            calls, 2,
            "{tool}: the domain call and one closure for verify + focus"
        );
    }
}
