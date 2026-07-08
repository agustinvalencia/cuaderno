//! In-process tests for [`cdno_mcp::CuadernoServer`].
//!
//! Subprocess + JSON-RPC tests would exercise the same thing more
//! expensively. These call into rmcp directly: build a server, ask
//! it for its info, and verify the advertised tool catalogue.

use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::store::{MemoryVaultStore, VaultStore};
use cdno_domain::Vault;
use cdno_mcp::CuadernoServer;
use rmcp::ServerHandler;

fn empty_server() -> CuadernoServer {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    CuadernoServer::new(Arc::new(vault))
}

#[test]
fn server_announces_name_and_tools_capability() {
    let server = empty_server();
    let info = server.get_info();
    assert_eq!(info.server_info.name, "cdno-mcp");
    assert!(
        info.capabilities.tools.is_some(),
        "tools capability must be advertised so MCP clients call tools/list"
    );
    assert!(
        info.instructions
            .as_deref()
            .map(|s| s.contains("Cuaderno MCP server"))
            .unwrap_or(false),
        "instructions should mention the server"
    );
}

#[test]
fn advertised_catalogue_matches_expected_surface() {
    let server = empty_server();
    let tools = server.advertised_tools();
    let got: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();

    // Sorted to match `advertised_tools`'s order so a failure points
    // at the missing or extra tool cleanly. The 16 design §11 tools,
    // the two daily-note tools (GH #158), the two weekly-note tools, and
    // the two monthly-note tools (GH #228).
    let mut expected = vec![
        // Context (15)
        "get_orientation",
        "get_weekly_context",
        "get_monthly_context",
        "get_project_context",
        "get_portfolio_contents",
        "get_stewardship_tracking",
        "get_active_questions",
        "read_daily_note",
        "read_weekly_note",
        "read_monthly_note",
        "search_notes",
        "list_projects",
        "get_commitments",
        "lint",
        "triage_inbox",
        // Operations (23)
        "append_to_log",
        "capture",
        "discard_inbox_item",
        "file_to_portfolio",
        "update_project_state",
        "add_action",
        "promote_action",
        "complete_action",
        "add_milestone",
        "complete_milestone",
        "add_waiting_on",
        "resolve_waiting_on",
        "create_commitment",
        "complete_commitment",
        "create_tracking_entry",
        "upsert_daily_section",
        "upsert_weekly_section",
        "upsert_monthly_section",
        "create_project",
        "create_portfolio",
        "link_portfolio_to_question",
        "link_portfolio_to_project",
        "create_question",
        "create_stewardship",
        "create_custom_note",
        // Lifecycle (4)
        "park_project",
        "activate_project",
        "set_question_status",
        "add_periodic_commitment",
    ];
    expected.sort();
    assert_eq!(got, expected, "advertised tool set drifted");
    assert_eq!(tools.len(), 44);
}

#[test]
fn every_tool_has_description_and_object_input_schema() {
    let server = empty_server();
    for tool in server.advertised_tools() {
        let desc = tool
            .description
            .as_ref()
            .expect("tool must have a description");
        assert!(!desc.is_empty(), "tool '{}' empty description", tool.name);
        // Every input schema is a JSON Schema `object` (even
        // no-arg tools, which use `EmptyInput`).
        let schema = &tool.input_schema;
        assert_eq!(
            schema
                .get("type")
                .and_then(|v: &serde_json::Value| v.as_str()),
            Some("object"),
            "tool '{}' has a non-object input schema",
            tool.name
        );
    }
}

/// No tools should still be flagged as "not yet implemented" in
/// their description now that GH #142 is fully closed (every
/// design §11 tool has a real handler). This is the post-condition
/// counterpart to the earlier `stub_tools_flag_their_status_in_the_description`
/// test, kept so a future regression that re-stubs a tool without
/// flagging the description fails loudly.
#[test]
fn no_advertised_tool_flags_itself_as_unimplemented() {
    let server = empty_server();
    for tool in server.advertised_tools() {
        let desc = tool.description.as_ref().unwrap();
        assert!(
            !desc.to_lowercase().contains("not yet implemented"),
            "tool '{}' still advertised as unimplemented: {}",
            tool.name,
            desc
        );
    }
}
