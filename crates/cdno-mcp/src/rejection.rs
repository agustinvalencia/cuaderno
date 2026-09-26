//! Caller-actionable domain rejections, carried as **tool results**
//! rather than JSON-RPC protocol errors (GH #560).
//!
//! # Why
//!
//! The MCP spec draws a line the server did not: a *malformed call*
//! (unknown tool, unparseable params) is a protocol error, while an
//! error raised *within* a successful invocation belongs in the result,
//! so the model can read it and act. Every `DomainError` used to land as
//! `-32603 INTERNAL_ERROR`, and at least one client renders that as a
//! bare "Tool execution failed" without ever showing `error.message` to
//! the model. The agent then cannot tell a validation rejection from a
//! transport failure — which defeats the whole point of a `reject`-mode
//! cap, whose job is to push a verbose agent to re-condense in its own
//! loop.
//!
//! # The line this module draws
//!
//! **A variant `cdno-domain` defines itself is a business-rule
//! rejection; a variant it merely forwards from `cdno-core` is a
//! mechanical failure.** The first kind describes something about the
//! *call* that the caller can change — a cap, a lifecycle state, a
//! query matching nothing or too much. The second describes the state of
//! the machine underneath it: a store that would not write, an index
//! that would not answer, a transaction that rolled back.
//!
//! [`ValidationError`] is the one core type on the rejection side, and
//! it earns it: both of its variants name a field the caller supplied.
//!
//! Deliberately left as protocol errors for now, and worth revisiting as
//! their own change rather than smuggled into this one:
//!
//! - `Manipulation` (`SectionNotFound` / `AmbiguousSection`) and `Parse`
//!   describe the *vault's* state, not the call's arguments. An agent can
//!   act on "this note is missing its Current State heading", but the
//!   right message is about the note rather than about the request, and
//!   choosing that wording is a separate question from this split.
//! - `Store` / `Index` / `Transaction` / `Config` / `Path` / `Template`
//!   are machinery. #560 names these as staying protocol errors.
//!
//! # The match is exhaustive on purpose
//!
//! [`classify`] has no wildcard arm. A new `DomainError` variant will
//! fail to compile here until somebody decides which side of the line it
//! falls on — which is the only mechanism that keeps this classification
//! honest as the domain grows, since a `_ =>` default would silently
//! swallow every future variant into whichever behaviour was convenient
//! the day it was written.

use cdno_core::error::ValidationError;
use cdno_domain::error::DomainError;
use rmcp::ErrorData;
use rmcp::model::{CallToolResult, Content};
use serde_json::{Value, json};

/// Key under `ErrorData::data` that carries a classified rejection from
/// [`crate::util::into_mcp_error`] to the single conversion point in
/// `CuadernoServer::call_tool`.
///
/// The hop through `ErrorData` is deliberate. Handlers convert domain
/// errors at ~55 call sites with `.map_err(into_mcp_error)?`; classifying
/// there would mean 55 chances to forget, and a forgotten one is
/// invisible (the tool still works, it just reports badly). Converting in
/// one place instead means a new handler cannot opt out by accident. The
/// `data` field is the spec's own slot for "additional information about
/// the error … defined by the sender", so the payload is legitimate even
/// on the path where it stays a protocol error.
const MARKER: &str = "cdno_rejection";

/// The `code` an agent can branch on, plus the fields it needs to
/// recover, for an error the caller can do something about.
///
/// `None` means "mechanical failure" — see the module docs for the line.
///
/// Codes are stable wire values: renaming one is a breaking change for
/// any agent that branches on it, so they are written out here rather
/// than derived from the variant name.
pub(crate) fn classify(e: &DomainError) -> Option<Value> {
    let (code, details) = match e {
        // -------------------------------------------------------------
        // Caps and lifecycle: the call asked for something the vault's
        // rules do not allow *right now*.
        // -------------------------------------------------------------
        DomainError::ProjectCapReached {
            current,
            max,
            active_projects,
        } => (
            "project_cap_reached",
            json!({ "current": current, "max": max, "active_projects": active_projects }),
        ),
        DomainError::StateTooLong { slug, chars, max } => (
            "state_too_long",
            json!({ "slug": slug, "chars": chars, "max": max }),
        ),
        DomainError::ProjectNotActive(slug) => ("project_not_active", json!({ "slug": slug })),
        DomainError::ProjectNotParked(slug) => ("project_not_parked", json!({ "slug": slug })),
        DomainError::CommitmentNotActive(slug) => {
            ("commitment_not_active", json!({ "slug": slug }))
        }
        DomainError::CommitmentAlreadyDue { slug, due } => (
            "commitment_already_due",
            json!({ "slug": slug, "due": due.to_string() }),
        ),

        // -------------------------------------------------------------
        // Query resolution. The `ambiguous_*` codes are the reason this
        // module exists: `candidates` is *recovery data*, and flattening
        // it into prose forces an agent to parse English to get it back.
        // -------------------------------------------------------------
        DomainError::ActionNotFound { slug, query } => {
            ("action_not_found", json!({ "slug": slug, "query": query }))
        }
        DomainError::AmbiguousAction {
            slug,
            query,
            candidates,
        } => (
            "ambiguous_action",
            json!({ "slug": slug, "query": query, "candidates": candidates }),
        ),
        DomainError::ActionAlreadyPromoted { slug, line } => (
            "action_already_promoted",
            json!({ "slug": slug, "line": line }),
        ),
        DomainError::BulletMissingEnergy { slug, line } => (
            "bullet_missing_energy",
            json!({ "slug": slug, "line": line }),
        ),
        DomainError::MilestoneNotFound { slug, query } => (
            "milestone_not_found",
            json!({ "slug": slug, "query": query }),
        ),
        DomainError::AmbiguousMilestone {
            slug,
            query,
            candidates,
        } => (
            "ambiguous_milestone",
            json!({ "slug": slug, "query": query, "candidates": candidates }),
        ),
        DomainError::PeriodicNotFound { slug, query } => (
            "periodic_not_found",
            json!({ "slug": slug, "query": query }),
        ),
        DomainError::AmbiguousPeriodic {
            slug,
            query,
            candidates,
        } => (
            "ambiguous_periodic",
            json!({ "slug": slug, "query": query, "candidates": candidates }),
        ),
        DomainError::WaitingOnNotFound { slug, query } => (
            "waiting_on_not_found",
            json!({ "slug": slug, "query": query }),
        ),
        DomainError::AmbiguousWaitingOn {
            slug,
            query,
            candidates,
        } => (
            "ambiguous_waiting_on",
            json!({ "slug": slug, "query": query, "candidates": candidates }),
        ),
        DomainError::AmbiguousSlug(slug) => ("ambiguous_slug", json!({ "slug": slug })),

        // -------------------------------------------------------------
        // Shape of the thing being written.
        // -------------------------------------------------------------
        DomainError::HardMilestoneRequiresDate { slug, title } => (
            "hard_milestone_requires_date",
            json!({ "slug": slug, "title": title }),
        ),
        DomainError::PeriodicRecurrenceUnreadable { slug, title } => (
            "periodic_recurrence_unreadable",
            json!({ "slug": slug, "title": title }),
        ),
        DomainError::PeriodicDateUnwritable { slug, line } => (
            "periodic_date_unwritable",
            json!({ "slug": slug, "line": line }),
        ),
        DomainError::TrackingOnFlatStewardship(slug) => (
            "tracking_on_flat_stewardship",
            json!({ "stewardship": slug }),
        ),
        DomainError::EmptyField { field } => ("empty_field", json!({ "field": field })),
        DomainError::MalformedWikilink { value } => {
            ("malformed_wikilink", json!({ "value": value }))
        }
        DomainError::MissingSection(section) => ("missing_section", json!({ "section": section })),
        DomainError::MissingFrontmatterField(field) => {
            ("missing_frontmatter_field", json!({ "field": field }))
        }
        DomainError::UnrepresentableFrontmatterValue { field, reason } => (
            "unrepresentable_frontmatter_value",
            json!({ "field": field, "reason": reason }),
        ),
        DomainError::ImplausibleDate {
            date,
            earliest,
            latest,
        } => (
            "implausible_date",
            json!({
                "date": date.to_string(),
                "earliest": earliest.to_string(),
                "latest": latest.to_string(),
            }),
        ),
        DomainError::UnresolvedPrompts { note_type, names } => (
            "unresolved_prompts",
            json!({ "note_type": note_type, "names": names }),
        ),

        // -------------------------------------------------------------
        // Note types and schemas: the vault's config decides these, and
        // the agent's move is either to pick a declared thing or to tell
        // the user which declaration is missing.
        // -------------------------------------------------------------
        DomainError::UnknownNoteType { note_type } => {
            ("unknown_note_type", json!({ "note_type": note_type }))
        }
        DomainError::ReservedTypeName { name } => ("reserved_type_name", json!({ "name": name })),
        DomainError::ReservedSchemaField { note_type, field } => (
            "reserved_schema_field",
            json!({ "note_type": note_type, "field": field }),
        ),
        DomainError::BuiltinTypeNotCustom { note_type } => {
            ("builtin_type_not_custom", json!({ "note_type": note_type }))
        }
        DomainError::UndeclaredSchemaField { note_type, field } => (
            "undeclared_schema_field",
            json!({ "note_type": note_type, "field": field }),
        ),
        DomainError::FieldNotSettable { note_type, field } => (
            "field_not_settable",
            json!({ "note_type": note_type, "field": field }),
        ),
        DomainError::InvalidFieldValue {
            note_type,
            field,
            reason,
        } => (
            "invalid_field_value",
            json!({ "note_type": note_type, "field": field, "reason": reason }),
        ),
        DomainError::MissingRequiredField { note_type, field } => (
            "missing_required_field",
            json!({ "note_type": note_type, "field": field }),
        ),
        DomainError::UnknownField { note_type, field } => (
            "unknown_field",
            json!({ "note_type": note_type, "field": field }),
        ),
        DomainError::UnknownTemplateVariant { note_type, variant } => (
            "unknown_template_variant",
            json!({ "note_type": note_type, "variant": variant }),
        ),
        DomainError::TemplateAlreadyExists { path } => {
            ("template_already_exists", json!({ "path": path }))
        }

        // -------------------------------------------------------------
        // The one core type on this side: both variants name a field the
        // caller supplied, so both are recoverable by the caller.
        // -------------------------------------------------------------
        DomainError::Validation(ValidationError::MissingField { field }) => {
            ("missing_field", json!({ "field": field }))
        }
        DomainError::Validation(ValidationError::InvalidField { field, reason }) => {
            ("invalid_field", json!({ "field": field, "reason": reason }))
        }

        // -------------------------------------------------------------
        // Mechanical failures: the machine under the call, not the call.
        // These stay JSON-RPC protocol errors. See the module docs for
        // why `Manipulation` and `Parse` sit here for now.
        // -------------------------------------------------------------
        DomainError::Store(_)
        | DomainError::Index(_)
        | DomainError::Parse(_)
        | DomainError::Manipulation(_)
        | DomainError::Transaction(_)
        | DomainError::Path(_)
        | DomainError::Template(_)
        | DomainError::Config(_) => return None,
    };

    // `message` is the domain's own `Display` output, carried inside the
    // payload rather than only in the protocol envelope — that envelope
    // is exactly what the client in #560 throws away.
    Some(json!({ "code": code, "message": e.to_string(), "details": details }))
}

/// Wrap a classified rejection for transport in `ErrorData::data`.
pub(crate) fn envelope(e: &DomainError) -> Option<Value> {
    classify(e).map(|r| json!({ MARKER: r }))
}

/// The single point where a marked protocol error becomes a tool result.
///
/// `Ok(result)` for a classified rejection — `isError: true`, with the
/// rejection as the one JSON content item, matching how every successful
/// result in this server carries its payload ([`crate::util::json_result`]).
/// `Err(e)` passes anything else through untouched, which covers both
/// mechanical domain failures and the router's own errors (unknown tool,
/// unparseable params — protocol errors by the spec, and correctly so).
pub(crate) fn decode(e: ErrorData) -> Result<CallToolResult, ErrorData> {
    let Some(payload) = e.data.as_ref().and_then(|d| d.get(MARKER)).cloned() else {
        return Err(e);
    };
    match Content::json(&payload) {
        Ok(content) => Ok(CallToolResult::error(vec![content])),
        // Serialising a `Value` we just built cannot realistically fail,
        // but if it ever did, the protocol error is still correct and
        // still carries the message — degrade, don't panic.
        Err(_) => Err(e),
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------
//
// Inline rather than under `tests/`, for the reason `verify.rs` and
// `checkpoint.rs` are: `classify` and `decode` are `pub(crate)`, and
// widening them to `pub` purely so an integration target can see them
// would put the classification table in this crate's public API. The
// wire-visible half of this behaviour is covered from outside, in
// `tests/e2e_stdio.rs`.

#[cfg(test)]
mod tests {
    use super::*;
    use cdno_core::error::{IndexError, StoreError};

    fn ambiguous() -> DomainError {
        DomainError::AmbiguousAction {
            slug: "thesis".into(),
            query: "methods".into(),
            candidates: vec![
                "Draft methods (deep)".into(),
                "Revise methods (light)".into(),
            ],
        }
    }

    #[test]
    fn a_business_rule_rejection_classifies_with_its_recovery_data() {
        let payload = classify(&ambiguous()).expect("caller-actionable");

        assert_eq!(payload["code"], "ambiguous_action");
        // `candidates` survives as an array. Flattening it into the
        // message is precisely the defect #560 reports.
        assert_eq!(
            payload["details"]["candidates"],
            json!(["Draft methods (deep)", "Revise methods (light)"])
        );
        // The domain's own Display output travels inside the payload,
        // not only in the protocol envelope a client may discard.
        assert!(
            payload["message"]
                .as_str()
                .expect("message")
                .contains("ambiguous action match"),
            "payload: {payload}"
        );
    }

    #[test]
    fn a_mechanical_failure_does_not_classify() {
        // Store and Index are named in #560 as staying protocol errors:
        // nothing the caller passed can fix a store that will not write.
        assert!(classify(&DomainError::Store(StoreError::NotFound("x".into()))).is_none());
        assert!(classify(&DomainError::Index(IndexError::Query("boom".into()))).is_none());
    }

    #[test]
    fn every_code_is_distinct_snake_case() {
        // Codes are wire values an agent branches on. Two variants
        // sharing one code would make them indistinguishable, and a
        // stray capital or hyphen would make the set inconsistent —
        // neither is caught by anything else, since `classify` is one
        // big match nobody reads end to end twice.
        let samples: Vec<DomainError> = vec![
            DomainError::ProjectCapReached {
                current: 5,
                max: 5,
                active_projects: vec![],
            },
            DomainError::StateTooLong {
                slug: "s".into(),
                chars: 501,
                max: 500,
            },
            DomainError::ProjectNotActive("s".into()),
            DomainError::ProjectNotParked("s".into()),
            DomainError::CommitmentNotActive("s".into()),
            ambiguous(),
            DomainError::ActionNotFound {
                slug: "s".into(),
                query: "q".into(),
            },
            DomainError::AmbiguousSlug("s".into()),
            DomainError::EmptyField { field: "title" },
            DomainError::UnknownNoteType {
                note_type: "t".into(),
            },
            DomainError::FieldNotSettable {
                note_type: "t".into(),
                field: "f".into(),
            },
            DomainError::Validation(cdno_core::error::ValidationError::MissingField {
                field: "f".into(),
            }),
            DomainError::Validation(cdno_core::error::ValidationError::InvalidField {
                field: "f".into(),
                reason: "r".into(),
            }),
        ];

        let mut seen: Vec<String> = Vec::new();
        for e in &samples {
            let payload = classify(e).unwrap_or_else(|| panic!("should classify: {e}"));
            let code = payload["code"]
                .as_str()
                .expect("code is a string")
                .to_owned();
            assert!(
                code.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "code `{code}` is not snake_case"
            );
            assert!(!seen.contains(&code), "duplicate code `{code}`");
            seen.push(code);
        }
    }

    #[test]
    fn decode_turns_a_marked_error_into_an_is_error_tool_result() {
        let err = crate::util::into_mcp_error(ambiguous());
        let result = decode(err).expect("marked errors become tool results");

        assert_eq!(result.is_error, Some(true));
        let text = result.content[0]
            .as_text()
            .expect("one text content item")
            .text
            .clone();
        let parsed: Value = serde_json::from_str(&text).expect("JSON content");
        assert_eq!(parsed["code"], "ambiguous_action");
    }

    #[test]
    fn decode_passes_an_unmarked_error_through_unchanged() {
        // Two ways to be unmarked, and both must stay protocol errors:
        // a mechanical domain failure, and an error rmcp itself raised
        // (unknown tool, bad params) which never went through
        // `into_mcp_error` at all.
        let mechanical =
            crate::util::into_mcp_error(DomainError::Index(IndexError::Query("boom".into())));
        assert!(decode(mechanical).is_err());

        let from_router = ErrorData::invalid_params("no such tool", None);
        assert!(decode(from_router).is_err());
    }
}
