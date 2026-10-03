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
    let instructions = info.instructions.as_deref().unwrap_or_default();
    assert!(
        instructions.contains("Cuaderno"),
        "instructions should identify the server"
    );
    // The instructions are the only surface that can teach the method to an
    // agent arriving with no skill loaded, and the distinctions below are the
    // ones it otherwise gets wrong — filing a perpetual responsibility as a
    // project, or a note per task. Losing them would be silent: every tool
    // would still work, and the vault would fill with the wrong note types.
    for phrase in [
        "PROJECTS END, STEWARDSHIPS DO NOT",
        "add_action",
        "promote_action",
        "APPEND-ONLY",
    ] {
        assert!(
            instructions.contains(phrase),
            "instructions must still state {phrase:?} — that is the judgement \
             an agent cannot get from the tool schemas"
        );
    }
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
        // Context (18)
        "get_orientation",
        // What is open right now (#568) — read-only, so it belongs to
        // the context router and the read-only surface with it.
        "current_focus",
        "get_weekly_context",
        "get_monthly_context",
        "get_project_context",
        "get_portfolio_contents",
        "get_stewardship_tracking",
        "get_active_questions",
        "read_daily_note",
        "read_weekly_note",
        "read_monthly_note",
        // Any note, whole (RFC 0002 T8, #621).
        "read_note",
        "search_notes",
        "list_projects",
        "list_note_types",
        "get_commitments",
        "lint",
        "triage_inbox",
        // Operations (35)
        "append_to_log",
        "capture",
        "discard_inbox_item",
        "file_to_portfolio",
        "update_project_state",
        // Revise a mutable custom note in place (RFC 0002 T9, #622).
        "revise_note",
        "add_action",
        "promote_action",
        "start_action",
        "start_unplanned_action",
        // Focus verbs (RFC 0005, #735).
        "pause_action",
        "switch_action",
        "switch_unplanned_action",
        "resume_action",
        "complete_action",
        "drop_action",
        "add_milestone",
        "set_core_question",
        "complete_milestone",
        "drop_milestone",
        "add_waiting_on",
        "resolve_waiting_on",
        "create_commitment",
        "complete_commitment",
        "drop_commitment",
        "reschedule_commitment",
        "complete_periodic",
        "create_tracking_entry",
        "upsert_daily_section",
        // Substance to the daily note's `## Notes`, a pointer to `## Logs`
        // (RFC 0002 T10, #623).
        "note_to_daily",
        "upsert_weekly_section",
        "upsert_monthly_section",
        "create_project",
        "create_portfolio",
        "link_portfolio_to_question",
        "link_portfolio_to_project",
        "create_question",
        "create_stewardship",
        "create_custom_note",
        // Lifecycle (6)
        "park_project",
        "activate_project",
        // Closing a project (RFC 0004).
        "complete_project",
        "drop_project",
        "set_question_status",
        "add_periodic_commitment",
        // Generic frontmatter setter (1, #301)
        "set_frontmatter",
    ];
    expected.sort();
    assert_eq!(got, expected, "advertised tool set drifted");
    assert_eq!(tools.len(), 64);
}

/// `read_note` is a read, so it rides the context router onto the
/// read-only server (RFC 0002 T8) — a concept an agent can find through
/// `search_notes` there but not open would be worthless.
#[test]
fn read_only_server_advertises_read_note() {
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    let server = CuadernoServer::read_only(Arc::new(vault));
    let names: Vec<String> = server
        .advertised_tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(names.iter().any(|n| n == "read_note"), "{names:?}");
    assert!(names.iter().any(|n| n == "search_notes"), "{names:?}");
    assert_eq!(
        names.len(),
        18,
        "the read-only surface is the context router: {names:?}"
    );
}

/// `revise_note` writes, so it is on the full catalogue and not on the
/// read-only server: the read-only surface is the context router alone
/// (RFC 0002 T9).
#[test]
fn revise_note_is_a_write_tool_absent_from_the_read_only_server() {
    let full: Vec<String> = empty_server()
        .advertised_tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(full.iter().any(|n| n == "revise_note"), "{full:?}");

    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    let read_only: Vec<String> = CuadernoServer::read_only(Arc::new(vault))
        .advertised_tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(
        !read_only.iter().any(|n| n == "revise_note"),
        "{read_only:?}"
    );
    assert_eq!(read_only.len(), 18, "{read_only:?}");
}

/// `note_to_daily` writes, so it is on the full catalogue and not on the
/// read-only server (RFC 0002 T10).
#[test]
fn note_to_daily_is_a_write_tool_absent_from_the_read_only_server() {
    let full: Vec<String> = empty_server()
        .advertised_tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(full.iter().any(|n| n == "note_to_daily"), "{full:?}");

    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _r) = Vault::new(store, index, VaultConfig::default()).expect("Vault::new");
    let read_only: Vec<String> = CuadernoServer::read_only(Arc::new(vault))
        .advertised_tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(
        !read_only.iter().any(|n| n == "note_to_daily"),
        "{read_only:?}"
    );
    assert_eq!(read_only.len(), 18, "{read_only:?}");
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

/// The same rationale as `complete_action`'s pointer below: an agent
/// reaching for `complete_commitment` for a promise that was cancelled
/// writes `commitment completed ...` into the permanent record. #569's
/// review found that kind of pointer deletable with the suite green, so
/// it is pinned rather than trusted.
#[test]
fn complete_commitment_points_at_the_drop_verb_for_a_promise_not_kept() {
    let server = empty_server();
    let tools = server.advertised_tools();
    let desc = tools
        .iter()
        .find(|t| t.name.as_ref() == "complete_commitment")
        .and_then(|t| t.description.clone())
        .expect("tool 'complete_commitment' not advertised");
    assert!(
        desc.contains("drop_commitment"),
        "complete_commitment must name the drop verb: {desc}"
    );
}

/// `start_action` must name `start_unplanned_action`. The domain keeps
/// the two apart so a non-matching query cannot silently create an
/// action (#568); an agent that does not know the creating verb exists
/// will either give up or, worse, reach for `add_action` and leave the
/// work unstarted. Same rationale as the completion/drop pointer below:
/// the description is the only instruction surface an agent sees.
#[test]
fn start_action_points_at_the_unplanned_verb_for_work_not_on_the_map() {
    let server = empty_server();
    let tools = server.advertised_tools();
    let desc = tools
        .iter()
        .find(|t| t.name.as_ref() == "start_action")
        .and_then(|t| t.description.clone())
        .expect("tool 'start_action' not advertised");
    assert!(
        desc.contains("start_unplanned_action"),
        "start_action must name the creating verb so an agent knows what \
         to reach for when the work is not on the map yet: {desc}"
    );
    assert!(
        desc.contains("will not create"),
        "and must say plainly that it refuses to create the bullet: {desc}"
    );
}

/// `complete_action`'s description is the only place an agent is told
/// that abandoning an action is a separate verb. Without the pointer the
/// reachable-looking move for work that was never performed is
/// `complete_action`, which writes `action done on ...` to the permanent
/// record — the exact failure #559 reports. Pinned here on the same
/// rationale as the linking mandate below: the description is the only
/// instruction surface an agent sees.
#[test]
fn complete_action_points_at_the_drop_verb_for_work_never_performed() {
    let server = empty_server();
    let tools = server.advertised_tools();
    let desc = tools
        .iter()
        .find(|t| t.name.as_ref() == "complete_action")
        .and_then(|t| t.description.clone())
        .expect("tool 'complete_action' not advertised");
    assert!(
        desc.contains("drop_action"),
        "complete_action must name the drop verb so an agent does not \
         record work that never happened: {desc}"
    );
}

/// A rejection an agent could have avoided is worse than one it could not:
/// the cap is knowable up front, and #560 records six blind retries spent
/// discovering it by binary search over the wire. Pinned because the
/// description is the only place the cap is stated before the fact.
#[test]
fn update_project_state_states_the_length_cap_before_it_bites() {
    let server = empty_server();
    let tools = server.advertised_tools();
    let desc = tools
        .iter()
        .find(|t| t.name.as_ref() == "update_project_state")
        .and_then(|t| t.description.clone())
        .expect("tool 'update_project_state' not advertised");
    assert!(
        desc.contains("max_state_chars"),
        "the cap must be named so an agent can respect it: {desc}"
    );
    assert!(
        desc.contains("500"),
        "the default must be stated, not just the setting's name: {desc}"
    );
    // "capped" alone would read as truncation, which would make an agent
    // think a long state is silently shortened rather than refused.
    assert!(
        desc.to_lowercase().contains("truncat"),
        "the description must rule out silent truncation: {desc}"
    );
    // Named as the DEFAULT policy, not as an absolute. Rejection needs
    // `state_overflow = reject` and a state that isn't already over the
    // cap — `warn` accepts with an advisory, and trimming an inherited
    // sprawl is accepted even under `reject`
    // (cdno-domain/src/vault/projects/state.rs). An agent told "always
    // rejected" would wrongly conclude a vault on `warn` had failed.
    let lower = desc.to_lowercase();
    assert!(
        lower.contains("reject") && lower.contains("default"),
        "the description must name `reject` as the DEFAULT policy, not an invariant: {desc}"
    );
    assert!(
        lower.contains("warn"),
        "the other policy must be named, or `warn` behaviour reads as a bug: {desc}"
    );
}

/// The tool description is the only instruction surface an agent that has
/// loaded no cuaderno skill ever sees, so the vault's linking convention has
/// to live there or the narrative tools produce plain-text lines that need
/// repairing downstream (#438). Asserted on the substance (wikilink syntax,
/// the bare-`#N` prohibition) rather than the exact wording, so the sentence
/// can be reworded without breaking the test.
#[test]
fn narrative_tools_mandate_linking_in_their_description() {
    let server = empty_server();
    let tools = server.advertised_tools();
    for name in ["append_to_log", "upsert_daily_section"] {
        let desc = tools
            .iter()
            .find(|t| t.name.as_ref() == name)
            .and_then(|t| t.description.clone())
            .unwrap_or_else(|| panic!("tool '{name}' not advertised"));
        assert!(
            desc.contains("[[slug]]"),
            "tool '{name}' description must show the wikilink form: {desc}"
        );
        assert!(
            desc.contains("`#N`"),
            "tool '{name}' description must rule out a bare forge reference: {desc}"
        );
    }

    // The parameter schema is the second surface, not a duplicate: a client
    // that renders field descriptions instead of the tool description sees
    // only this one, which is the whole reason the sentence is written twice.
    // Asserting just the tool description would let a cleanup trim the doc
    // comment on `AppendToLogInput.text` and leave those clients unguided
    // with the suite still green.
    let text_param = tools
        .iter()
        .find(|t| t.name.as_ref() == "append_to_log")
        .and_then(|t| {
            t.input_schema
                .get("properties")
                .and_then(|p: &serde_json::Value| p.get("text"))
                .and_then(|f| f.get("description"))
                .and_then(|d| d.as_str())
                .map(str::to_owned)
        })
        .expect("append_to_log's `text` parameter must carry a schema description");
    assert!(
        text_param.contains("[[slug]]") && text_param.contains("`#N`"),
        "the linking rule must survive on the parameter schema too: {text_param}"
    );

    // The counterpart: `capture` is deliberately verbatim and zero-friction,
    // so it must NOT acquire the same mandate.
    let capture = tools
        .iter()
        .find(|t| t.name.as_ref() == "capture")
        .and_then(|t| t.description.clone())
        .expect("capture not advertised");
    assert!(
        !capture.contains("[[slug]]"),
        "capture is a verbatim quick-capture and must stay friction-free: {capture}"
    );
}

/// The `append_to_log` description has to say the timestamp is applied for
/// you, or an agent prefixes its own and the line is stamped twice.
#[test]
fn append_to_log_description_warns_against_a_self_prefixed_time() {
    let server = empty_server();
    let desc = server
        .advertised_tools()
        .iter()
        .find(|t| t.name.as_ref() == "append_to_log")
        .and_then(|t| t.description.clone())
        .expect("append_to_log not advertised");
    assert!(
        desc.contains("HH:MM"),
        "description must show the stamped form it produces: {desc}"
    );
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

/// `create_custom_note`'s description must tell the agent to leave the title
/// heading out of `body` (the engine writes the H1), and use the same
/// "inserted after the H1" wording as every other surface.
#[test]
fn create_custom_note_says_body_excludes_the_title_heading() {
    let server = empty_server();
    let tools = server.advertised_tools();
    let desc = tools
        .iter()
        .find(|t| t.name.as_ref() == "create_custom_note")
        .and_then(|t| t.description.clone())
        .expect("tool 'create_custom_note' not advertised");
    assert!(desc.contains("WITHOUT the title heading"), "{desc}");
    assert!(desc.contains("inserted after the H1"), "{desc}");
    assert!(!desc.contains("appended after the H1"), "{desc}");
}

fn description_of(name: &str) -> String {
    empty_server()
        .advertised_tools()
        .iter()
        .find(|t| t.name.as_ref() == name)
        .and_then(|t| t.description.as_deref().map(str::to_owned))
        .unwrap_or_else(|| panic!("tool '{name}' not advertised"))
}

/// RFC 0002 T15 (#628): the concept method exists for an agent only if the
/// server instructions state it. The bullet is conditional because the
/// instructions are static text and the `concept` type is declared per vault;
/// it carries the filing test, the provenance rule and the refinement rule.
#[test]
fn instructions_carry_the_conditional_concept_filing_test() {
    let info = empty_server().get_info();
    let instructions = info.instructions.as_deref().unwrap_or_default();
    for phrase in [
        "concept type",
        "CONCEPTS, if the vault declares a `concept` type",
        "list_note_types",
        "is evidence",
        "is a routine",
        "is a concept note",
        "`## Notes` with note_to_daily until promoted",
        "Evidence never depends on a concept's current text",
        "link a concept only as see-also",
        "never make a concept the `origin` of evidence",
        "Evidence records what happened then; a concept describes understanding now",
        "refined in place with revise_note, never appended to",
    ] {
        assert!(
            instructions.contains(phrase),
            "instructions must state {phrase:?}: {instructions}"
        );
    }
}

/// The descriptions T15 extends must keep the method clauses and stay one
/// paragraph (a newline would render as a break in some clients' listings).
#[test]
fn concept_method_clauses_are_pinned_on_the_tool_descriptions() {
    let read_note = description_of("read_note");
    assert!(read_note.contains("expected_hash"), "{read_note}");
    assert!(read_note.contains("A concept's `origin`"), "{read_note}");
    assert!(
        read_note.contains("passing the part before the `#`"),
        "{read_note}"
    );

    let create = description_of("create_custom_note");
    assert!(create.contains("`origin`"), "{create}");
    assert!(create.contains("search before you create"), "{create}");
    assert!(create.contains("one concept per note"), "{create}");
    assert!(create.contains("terms are ANDed"), "{create}");
    assert!(
        create.contains("rather than merging them yourself"),
        "{create}"
    );
    assert!(!create.contains("merged into the first"), "{create}");
    assert!(create.contains("`ambiguous_section`"), "{create}");
    assert!(create.contains("needs no separate log line"), "{create}");

    let search = description_of("search_notes");
    assert!(search.contains("#concept"), "{search}");
    assert!(search.contains("matches the word, not the tag"), "{search}");
    assert!(
        search.contains("Read each hit with `read_note`"),
        "{search}"
    );
    assert!(!search.contains("search `#concept`"), "{search}");
    assert!(search.contains("`note_type: concept`"), "{search}");
    assert!(search.contains("two or more dates"), "{search}");

    let note_to_daily = description_of("note_to_daily");
    assert!(
        note_to_daily.contains("two or more dates") && note_to_daily.contains("`origin`"),
        "{note_to_daily}"
    );

    let revise = description_of("revise_note");
    assert!(revise.contains("current best account"), "{revise}");
    assert!(revise.contains("when it was last run"), "{revise}");

    for name in [
        "read_note",
        "create_custom_note",
        "search_notes",
        "note_to_daily",
        "upsert_daily_section",
        "revise_note",
    ] {
        let desc = description_of(name);
        assert!(
            !desc.contains('\n'),
            "tool '{name}' description must stay one paragraph: {desc}"
        );
    }
}
