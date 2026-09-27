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
//! **Can the caller do something about it?** If a different argument, or
//! a decision the agent is able to take, would change the outcome, it is
//! a rejection and belongs in the result. If only the machine underneath
//! can change the outcome — a disk that would not write, an index that
//! would not answer, a transaction that rolled back — it is a mechanical
//! failure and stays a protocol error.
//!
//! "A variant `cdno-domain` defines itself" is a tempting shorthand for
//! that question, and it is *nearly* right, but it is a proxy and the
//! proxy is wrong in both directions. Two places matter:
//!
//! - **`Store(NotFound)` is a rejection**, because a wrong slug is the
//!   commonest mistake an agent makes and the domain already treats it as
//!   recoverable: it hand-builds the error with `available_projects_hint()`
//!   appended, i.e. with the list of valid choices
//!   (`vault/projects/state.rs`, `vault/projects/mod.rs`, `lifecycle.rs`,
//!   `context.rs`, `commitments.rs`). Classifying that as machinery would
//!   have left the single most fixable error arriving as the exact
//!   "Tool execution failed" shape #560 is about. `AlreadyExists` joins it
//!   for the same reason — pick another name, or pass force — and matches
//!   how the domain's own `TemplateAlreadyExists` is treated.
//! - **`Manipulation` is a rejection**, because a note that is not shaped
//!   as expected is something an agent can act on and report, and because
//!   `MissingSection` — the domain's own twin of `SectionNotFound` — is
//!   already on this side. Splitting them would deliver one broken-note
//!   condition in two different shapes depending on which layer noticed.
//!
//! The rest of `StoreError` (`PermissionDenied`, `Io`, `LockTimeout`,
//! `OutsideVault`) is machinery, as are `Index`, `Transaction`, `Config`,
//! `Template` and `Parse`. `Path` stays mechanical too: MCP tools take
//! slugs, never paths, so a `PathError` means the *server* built a bad
//! path, not that the caller passed one.
//!
//! # The match is exhaustive on purpose
//!
//! [`classify`] has no wildcard arm. A new `DomainError` variant will
//! fail to compile here until somebody decides which side of the line it
//! falls on — which is the only mechanism that keeps this classification
//! honest as the domain grows, since a `_ =>` default would silently
//! swallow every future variant into whichever behaviour was convenient
//! the day it was written.

use cdno_core::error::{ManipulationError, StoreError, ValidationError};
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
        // -------------------------------------------------------------
        // Core forwards the caller CAN act on. The `message` carries the
        // domain's own text, which for `NotFound` includes the
        // available-slugs hint it deliberately appends.
        // -------------------------------------------------------------
        DomainError::Store(StoreError::NotFound(what)) => ("not_found", json!({ "what": what })),
        DomainError::Store(StoreError::AlreadyExists(what)) => {
            ("already_exists", json!({ "what": what }))
        }
        DomainError::Manipulation(ManipulationError::SectionNotFound(section)) => {
            ("section_not_found", json!({ "section": section }))
        }
        DomainError::Manipulation(ManipulationError::AmbiguousSection(section)) => {
            ("ambiguous_section", json!({ "section": section }))
        }

        // -------------------------------------------------------------
        // Mechanical failures: the machine under the call, not the call.
        // These stay JSON-RPC protocol errors — nothing the caller could
        // pass differently changes the outcome.
        // -------------------------------------------------------------
        DomainError::Store(
            StoreError::PermissionDenied(_)
            | StoreError::Io { .. }
            | StoreError::LockTimeout(_)
            | StoreError::OutsideVault(_),
        )
        | DomainError::Index(_)
        | DomainError::Parse(_)
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
        // Nothing the caller passed can fix a disk that will not write or
        // an index that will not answer.
        assert!(
            classify(&DomainError::Store(StoreError::PermissionDenied(
                "x".into()
            )))
            .is_none(),
            "permission denied is the machine, not the call"
        );
        assert!(classify(&DomainError::Index(IndexError::Query("boom".into()))).is_none());
        assert!(
            classify(&DomainError::Store(StoreError::LockTimeout(
                std::time::Duration::from_secs(5)
            )))
            .is_none(),
            "a contended write lock is a retry, not a different argument"
        );
    }

    /// `StoreError` straddles the line, so the split inside it is pinned
    /// separately — this is what the first review of #560 caught, and the
    /// case that matters most in practice.
    #[test]
    fn a_missing_note_is_the_callers_to_fix_and_keeps_its_hint() {
        // A wrong slug is the commonest mistake an agent makes, and the
        // domain treats it as recoverable: it appends the list of valid
        // projects to the message. Bucketing this as machinery would have
        // sent the most fixable error of all back as the bare
        // "Tool execution failed" that #560 exists to stop.
        let payload = classify(&DomainError::Store(StoreError::NotFound(
            "projects/typo.md — available projects: alpha, beta".into(),
        )))
        .expect("a missing note is caller-actionable");

        assert_eq!(payload["code"], "not_found");
        assert!(
            payload["message"]
                .as_str()
                .expect("message")
                .contains("available projects: alpha, beta"),
            "the hint the domain built must survive to the client: {payload}"
        );
    }

    /// The same broken-note condition must not arrive in two shapes
    /// depending on which layer noticed it: `MissingSection` is the
    /// domain's own twin of core's `SectionNotFound`.
    #[test]
    fn a_note_missing_its_section_classifies_from_either_layer() {
        assert_eq!(
            classify(&DomainError::MissingSection("Current State")).expect("domain twin")["code"],
            "missing_section"
        );
        assert_eq!(
            classify(&DomainError::Manipulation(
                ManipulationError::SectionNotFound("Current State".into())
            ))
            .expect("core twin")["code"],
            "section_not_found"
        );
    }

    /// Every code the classifier can emit, checked for distinctness and
    /// format. Codes are wire values an agent branches on: two variants
    /// sharing one makes them indistinguishable, and a stray capital or
    /// hyphen makes the set inconsistent. Neither is caught anywhere else,
    /// since `classify` is one long match nobody reads end to end twice.
    ///
    /// The list is hand-maintained and MUST grow with `classify`. The
    /// exhaustive match there forces a decision about a new variant but
    /// cannot force an entry here, so this is a convention, not a
    /// guarantee — deriving it from the source was considered and rejected
    /// as fragile, since the codes are not textually distinguishable from
    /// the `json!` keys around them.
    #[test]
    fn every_code_is_distinct_snake_case() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 27).expect("valid date");
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
            DomainError::CommitmentAlreadyDue {
                slug: "s".into(),
                due: date,
            },
            DomainError::ActionNotFound {
                slug: "s".into(),
                query: "q".into(),
            },
            ambiguous(),
            DomainError::ActionAlreadyPromoted {
                slug: "s".into(),
                line: "l".into(),
            },
            DomainError::BulletMissingEnergy {
                slug: "s".into(),
                line: "l".into(),
            },
            DomainError::MilestoneNotFound {
                slug: "s".into(),
                query: "q".into(),
            },
            DomainError::AmbiguousMilestone {
                slug: "s".into(),
                query: "q".into(),
                candidates: vec![],
            },
            DomainError::PeriodicNotFound {
                slug: "s".into(),
                query: "q".into(),
            },
            DomainError::AmbiguousPeriodic {
                slug: "s".into(),
                query: "q".into(),
                candidates: vec![],
            },
            DomainError::WaitingOnNotFound {
                slug: "s".into(),
                query: "q".into(),
            },
            DomainError::AmbiguousWaitingOn {
                slug: "s".into(),
                query: "q".into(),
                candidates: vec![],
            },
            DomainError::AmbiguousSlug("s".into()),
            DomainError::HardMilestoneRequiresDate {
                slug: "s".into(),
                title: "t".into(),
            },
            DomainError::PeriodicRecurrenceUnreadable {
                slug: "s".into(),
                title: "t".into(),
            },
            DomainError::PeriodicDateUnwritable {
                slug: "s".into(),
                line: "l".into(),
            },
            DomainError::TrackingOnFlatStewardship("s".into()),
            DomainError::EmptyField { field: "title" },
            DomainError::MalformedWikilink { value: "v".into() },
            DomainError::MissingSection("Current State"),
            DomainError::MissingFrontmatterField("f".into()),
            DomainError::UnrepresentableFrontmatterValue {
                field: "f".into(),
                reason: "r".into(),
            },
            DomainError::ImplausibleDate {
                date,
                earliest: date,
                latest: date,
            },
            DomainError::UnresolvedPrompts {
                note_type: "t".into(),
                names: vec![],
            },
            DomainError::UnknownNoteType {
                note_type: "t".into(),
            },
            DomainError::ReservedTypeName { name: "n".into() },
            DomainError::ReservedSchemaField {
                note_type: "t".into(),
                field: "f".into(),
            },
            DomainError::BuiltinTypeNotCustom {
                note_type: "t".into(),
            },
            DomainError::UndeclaredSchemaField {
                note_type: "t".into(),
                field: "f".into(),
            },
            DomainError::FieldNotSettable {
                note_type: "t".into(),
                field: "f".into(),
            },
            DomainError::InvalidFieldValue {
                note_type: "t".into(),
                field: "f".into(),
                reason: "r".into(),
            },
            DomainError::MissingRequiredField {
                note_type: "t".into(),
                field: "f".into(),
            },
            DomainError::UnknownField {
                note_type: "t".into(),
                field: "f".into(),
            },
            DomainError::UnknownTemplateVariant {
                note_type: "t".into(),
                variant: "v".into(),
            },
            DomainError::TemplateAlreadyExists { path: "p".into() },
            DomainError::Validation(ValidationError::MissingField { field: "f".into() }),
            DomainError::Validation(ValidationError::InvalidField {
                field: "f".into(),
                reason: "r".into(),
            }),
            DomainError::Store(StoreError::NotFound("projects/x.md".into())),
            DomainError::Store(StoreError::AlreadyExists("projects/x.md".into())),
            DomainError::Manipulation(ManipulationError::SectionNotFound("Logs".into())),
            DomainError::Manipulation(ManipulationError::AmbiguousSection("Logs".into())),
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

        // Guards the list above against silently shrinking, e.g. if two
        // arms are merged and a sample is dropped with one of them.
        assert_eq!(
            seen.len(),
            45,
            "expected every classified variant to be sampled; update this \
             count deliberately when adding or removing one"
        );
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
