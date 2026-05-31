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
fn advertised_catalogue_matches_design_section_11_verbatim() {
    let server = empty_server();
    let tools = server.advertised_tools();
    let got: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();

    // Sorted to match `advertised_tools`'s order so a failure points
    // at the missing or extra tool cleanly.
    let mut expected = vec![
        // Context (7)
        "get_orientation",
        "get_weekly_context",
        "get_monthly_context",
        "get_project_context",
        "get_portfolio_contents",
        "get_stewardship_tracking",
        "get_active_questions",
        // Operations (9)
        "append_to_log",
        "file_to_portfolio",
        "update_project_state",
        "add_action",
        "promote_action",
        "complete_action",
        "create_commitment",
        "complete_commitment",
        "create_tracking_entry",
    ];
    expected.sort();
    assert_eq!(got, expected, "advertised tool set drifted from design §11");
    assert_eq!(tools.len(), 16);
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

/// The four context tools deferred to GH #142 are still advertised
/// in `tools/list` (so MCP clients see the full design §11
/// catalogue) but their bodies return `INTERNAL_ERROR` via
/// `not_yet_implemented`. Their descriptions must say so upfront so
/// the agent's tool-selection step has the status before it picks
/// the tool — otherwise the only signal of unimplemented-ness is the
/// error response after the agent already committed to calling.
#[test]
fn stub_tools_flag_their_status_in_the_description() {
    let server = empty_server();
    // get_weekly_context flipped wired in #142a; the remaining three
    // stubs land one PR each as the next #142 follow-ups.
    let stubs = [
        "get_monthly_context",
        "get_project_context",
        "get_stewardship_tracking",
    ];
    let tools = server.advertised_tools();
    for name in stubs {
        let tool = tools
            .iter()
            .find(|t| t.name.as_ref() == name)
            .unwrap_or_else(|| panic!("stub tool '{name}' missing from catalogue"));
        let desc = tool.description.as_ref().unwrap();
        assert!(
            desc.contains("not yet implemented"),
            "stub '{name}' description should flag implementation status: {desc}"
        );
    }
}
