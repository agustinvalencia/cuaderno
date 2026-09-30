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
use serde::Serialize;
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

/// The `code` an agent branches on, as a type rather than a string.
///
/// The wire form is **derived** from the variant name by `rename_all`, so
/// the two properties the docs advertise — every code distinct, every code
/// snake_case — hold by construction instead of by a test that samples the
/// table. Two variants cannot share a name, so they cannot share a code,
/// and there is no hand-written string to mistype. This replaces a
/// hand-maintained list of 45 codes that guarded the same properties by
/// convention (review of #560).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RejectionCode {
    ActionAlreadyPromoted,
    ActionNotFound,
    AlreadyExists,
    AmbiguousAction,
    AmbiguousMilestone,
    AmbiguousPeriodic,
    AmbiguousProject,
    AmbiguousSection,
    AmbiguousSlug,
    AmbiguousWaitingOn,
    BuiltinTypeNotCustom,
    BulletMissingEnergy,
    CommitmentAlreadyDue,
    CommitmentNotActive,
    EmptyField,
    FieldNotSettable,
    HardMilestoneRequiresDate,
    ImplausibleDate,
    InvalidField,
    InvalidFieldValue,
    MalformedWikilink,
    MilestoneNotFound,
    MissingField,
    MissingFrontmatterField,
    MissingRequiredField,
    HistorySectionNotReplaceable,
    HistoryEntryHeadingInvalid,
    MissingSection,
    NotFound,
    NoteNotRevisable,
    PeriodicDateUnwritable,
    PeriodicNotFound,
    PeriodicRecurrenceUnreadable,
    ProjectCapReached,
    ProjectNotActive,
    ProjectNotParked,
    ReservedSchemaField,
    ReservedTypeName,
    RevisionInvalid,
    SectionNotFound,
    StaleRevision,
    StateTooLong,
    TemplateAlreadyExists,
    TrackingOnFlatStewardship,
    UndeclaredSchemaField,
    UnknownField,
    UnknownNoteType,
    UnknownTemplateVariant,
    UnrepresentableFrontmatterValue,
    UnresolvedPrompts,
    WaitingOnNotFound,
}

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
            RejectionCode::ProjectCapReached,
            json!({ "current": current, "max": max, "active_projects": active_projects }),
        ),
        DomainError::StateTooLong { slug, chars, max } => (
            RejectionCode::StateTooLong,
            json!({ "slug": slug, "chars": chars, "max": max }),
        ),
        DomainError::ProjectNotActive(slug) => {
            (RejectionCode::ProjectNotActive, json!({ "slug": slug }))
        }
        DomainError::ProjectNotParked(slug) => {
            (RejectionCode::ProjectNotParked, json!({ "slug": slug }))
        }
        DomainError::CommitmentNotActive(slug) => {
            (RejectionCode::CommitmentNotActive, json!({ "slug": slug }))
        }
        DomainError::CommitmentAlreadyDue { slug, due } => (
            RejectionCode::CommitmentAlreadyDue,
            json!({ "slug": slug, "due": due.to_string() }),
        ),

        // -------------------------------------------------------------
        // Query resolution. The `ambiguous_*` codes are the reason this
        // module exists: `candidates` is *recovery data*, and flattening
        // it into prose forces an agent to parse English to get it back.
        // -------------------------------------------------------------
        DomainError::ActionNotFound { slug, query } => (
            RejectionCode::ActionNotFound,
            json!({ "slug": slug, "query": query }),
        ),
        DomainError::AmbiguousAction {
            slug,
            query,
            candidates,
        } => (
            RejectionCode::AmbiguousAction,
            json!({ "slug": slug, "query": query, "candidates": candidates }),
        ),
        DomainError::ActionAlreadyPromoted { slug, line } => (
            RejectionCode::ActionAlreadyPromoted,
            json!({ "slug": slug, "line": line }),
        ),
        DomainError::BulletMissingEnergy { slug, line } => (
            RejectionCode::BulletMissingEnergy,
            json!({ "slug": slug, "line": line }),
        ),
        DomainError::MilestoneNotFound { slug, query } => (
            RejectionCode::MilestoneNotFound,
            json!({ "slug": slug, "query": query }),
        ),
        DomainError::AmbiguousMilestone {
            slug,
            query,
            candidates,
        } => (
            RejectionCode::AmbiguousMilestone,
            json!({ "slug": slug, "query": query, "candidates": candidates }),
        ),
        DomainError::PeriodicNotFound { slug, query } => (
            RejectionCode::PeriodicNotFound,
            json!({ "slug": slug, "query": query }),
        ),
        DomainError::AmbiguousPeriodic {
            slug,
            query,
            candidates,
        } => (
            RejectionCode::AmbiguousPeriodic,
            json!({ "slug": slug, "query": query, "candidates": candidates }),
        ),
        DomainError::WaitingOnNotFound { slug, query } => (
            RejectionCode::WaitingOnNotFound,
            json!({ "slug": slug, "query": query }),
        ),
        DomainError::AmbiguousWaitingOn {
            slug,
            query,
            candidates,
        } => (
            RejectionCode::AmbiguousWaitingOn,
            json!({ "slug": slug, "query": query, "candidates": candidates }),
        ),
        DomainError::AmbiguousProject { slug, candidates } => (
            RejectionCode::AmbiguousProject,
            json!({
                "slug": slug,
                "candidates": candidates.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
            }),
        ),
        DomainError::AmbiguousSlug(slug) => (RejectionCode::AmbiguousSlug, json!({ "slug": slug })),

        // -------------------------------------------------------------
        // Shape of the thing being written.
        // -------------------------------------------------------------
        DomainError::HardMilestoneRequiresDate { slug, title } => (
            RejectionCode::HardMilestoneRequiresDate,
            json!({ "slug": slug, "title": title }),
        ),
        DomainError::PeriodicRecurrenceUnreadable { slug, title } => (
            RejectionCode::PeriodicRecurrenceUnreadable,
            json!({ "slug": slug, "title": title }),
        ),
        DomainError::PeriodicDateUnwritable { slug, line } => (
            RejectionCode::PeriodicDateUnwritable,
            json!({ "slug": slug, "line": line }),
        ),
        DomainError::TrackingOnFlatStewardship(slug) => (
            RejectionCode::TrackingOnFlatStewardship,
            json!({ "stewardship": slug }),
        ),
        DomainError::EmptyField { field } => (RejectionCode::EmptyField, json!({ "field": field })),
        DomainError::MalformedWikilink { value } => {
            (RejectionCode::MalformedWikilink, json!({ "value": value }))
        }
        // Append-only sections (#638's `Notes`, the daily log): the call
        // asked to replace history. Appending instead succeeds, so the
        // caller can act — and `reason` on the heading variant is exactly
        // the recovery data an agent needs to pick a legal heading.
        DomainError::HistorySectionNotReplaceable { section } => (
            RejectionCode::HistorySectionNotReplaceable,
            json!({ "section": section }),
        ),
        DomainError::HistoryEntryHeadingInvalid {
            section,
            heading,
            reason,
        } => (
            RejectionCode::HistoryEntryHeadingInvalid,
            json!({ "section": section, "heading": heading, "reason": reason }),
        ),
        DomainError::MissingSection(section) => {
            (RejectionCode::MissingSection, json!({ "section": section }))
        }
        DomainError::MissingFrontmatterField(field) => (
            RejectionCode::MissingFrontmatterField,
            json!({ "field": field }),
        ),
        DomainError::UnrepresentableFrontmatterValue { field, reason } => (
            RejectionCode::UnrepresentableFrontmatterValue,
            json!({ "field": field, "reason": reason }),
        ),
        DomainError::ImplausibleDate {
            date,
            earliest,
            latest,
        } => (
            RejectionCode::ImplausibleDate,
            json!({
                "date": date.to_string(),
                "earliest": earliest.to_string(),
                "latest": latest.to_string(),
            }),
        ),
        DomainError::UnresolvedPrompts { note_type, names } => (
            RejectionCode::UnresolvedPrompts,
            json!({ "note_type": note_type, "names": names }),
        ),

        // -------------------------------------------------------------
        // Revising a note in place (T7). A non-revisable note is revised
        // through its own commands instead; a malformed revision succeeds
        // with a different heading or content; a stale hash succeeds after
        // reading the note again — `actual` is the hash to re-read against.
        // -------------------------------------------------------------
        DomainError::NoteNotRevisable { path, reason } => (
            RejectionCode::NoteNotRevisable,
            json!({ "path": path, "reason": reason }),
        ),
        DomainError::RevisionInvalid { reason } => {
            (RejectionCode::RevisionInvalid, json!({ "reason": reason }))
        }
        DomainError::StaleRevision {
            path,
            expected,
            actual,
        } => (
            RejectionCode::StaleRevision,
            json!({ "path": path, "expected": expected, "actual": actual }),
        ),

        // -------------------------------------------------------------
        // Note types and schemas: the vault's config decides these, and
        // the agent's move is either to pick a declared thing or to tell
        // the user which declaration is missing.
        // -------------------------------------------------------------
        DomainError::UnknownNoteType { note_type } => (
            RejectionCode::UnknownNoteType,
            json!({ "note_type": note_type }),
        ),
        DomainError::ReservedTypeName { name } => {
            (RejectionCode::ReservedTypeName, json!({ "name": name }))
        }
        DomainError::ReservedSchemaField { note_type, field } => (
            RejectionCode::ReservedSchemaField,
            json!({ "note_type": note_type, "field": field }),
        ),
        DomainError::BuiltinTypeNotCustom { note_type } => (
            RejectionCode::BuiltinTypeNotCustom,
            json!({ "note_type": note_type }),
        ),
        DomainError::UndeclaredSchemaField { note_type, field } => (
            RejectionCode::UndeclaredSchemaField,
            json!({ "note_type": note_type, "field": field }),
        ),
        DomainError::FieldNotSettable { note_type, field } => (
            RejectionCode::FieldNotSettable,
            json!({ "note_type": note_type, "field": field }),
        ),
        DomainError::InvalidFieldValue {
            note_type,
            field,
            reason,
        } => (
            RejectionCode::InvalidFieldValue,
            json!({ "note_type": note_type, "field": field, "reason": reason }),
        ),
        DomainError::MissingRequiredField { note_type, field } => (
            RejectionCode::MissingRequiredField,
            json!({ "note_type": note_type, "field": field }),
        ),
        DomainError::UnknownField { note_type, field } => (
            RejectionCode::UnknownField,
            json!({ "note_type": note_type, "field": field }),
        ),
        DomainError::UnknownTemplateVariant { note_type, variant } => (
            RejectionCode::UnknownTemplateVariant,
            json!({ "note_type": note_type, "variant": variant }),
        ),
        DomainError::TemplateAlreadyExists { path } => (
            RejectionCode::TemplateAlreadyExists,
            json!({ "path": path }),
        ),

        // -------------------------------------------------------------
        // The one core type on this side: both variants name a field the
        // caller supplied, so both are recoverable by the caller.
        // -------------------------------------------------------------
        DomainError::Validation(ValidationError::MissingField { field }) => {
            (RejectionCode::MissingField, json!({ "field": field }))
        }
        DomainError::Validation(ValidationError::InvalidField { field, reason }) => (
            RejectionCode::InvalidField,
            json!({ "field": field, "reason": reason }),
        ),

        // -------------------------------------------------------------
        // -------------------------------------------------------------
        // Core forwards the caller CAN act on. The `message` carries the
        // domain's own text, which for `NotFound` includes the
        // available-slugs hint it deliberately appends.
        // -------------------------------------------------------------
        DomainError::Store(StoreError::NotFound(what)) => {
            (RejectionCode::NotFound, json!({ "what": what }))
        }
        DomainError::Store(StoreError::AlreadyExists(what)) => {
            (RejectionCode::AlreadyExists, json!({ "what": what }))
        }
        DomainError::Manipulation(ManipulationError::SectionNotFound(section)) => (
            RejectionCode::SectionNotFound,
            json!({ "section": section }),
        ),
        DomainError::Manipulation(ManipulationError::AmbiguousSection(section)) => (
            RejectionCode::AmbiguousSection,
            json!({ "section": section }),
        ),

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

/// A rejection raised by a handler itself rather than by the domain.
///
/// Some caller-actionable outcomes never become a [`DomainError`]: note
/// reference resolution returns `RefResolution::NotFound` and
/// `RefResolution::Ambiguous` as `Ok` values, by design, so the interface
/// decides what to say. This marks such an outcome exactly as
/// [`crate::util::into_mcp_error`] marks a classified domain error, so it
/// reaches the client through the same [`decode`] step, in the same shape.
pub(crate) fn reject(code: RejectionCode, message: String, details: Value) -> ErrorData {
    let payload = json!({ "code": code, "message": message, "details": details });
    ErrorData::internal_error(message, Some(json!({ MARKER: payload })))
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
    fn a_project_at_two_locations_is_ambiguous_with_its_candidates() {
        use cdno_core::path::VaultPath;

        let payload = classify(&DomainError::AmbiguousProject {
            slug: "thesis".into(),
            candidates: vec![
                VaultPath::new("projects/_parked/thesis.md").unwrap(),
                VaultPath::new("projects/thesis.md").unwrap(),
            ],
        })
        .expect("a duplicated project is caller-actionable");

        assert_eq!(payload["code"], "ambiguous_project");
        assert_eq!(payload["details"]["slug"], "thesis");
        assert_eq!(
            payload["details"]["candidates"],
            json!(["projects/_parked/thesis.md", "projects/thesis.md"])
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

    /// The two variants `main` added while this PR was open (#638's
    /// append-only `Notes` section). They are here because the exhaustive
    /// match refused to compile against the newer `cdno-domain` — which is
    /// the mechanism working as intended, caught by CI building the merge
    /// commit rather than by anybody remembering to look.
    ///
    /// Both are caller-actionable: appending instead of replacing succeeds,
    /// and a legal heading succeeds, so a different call changes the
    /// outcome. `reason` travels as data because it is what tells an agent
    /// which heading to pick.
    #[test]
    fn the_append_only_history_rejections_carry_what_to_do_instead() {
        let payload = classify(&DomainError::HistorySectionNotReplaceable {
            section: "Notes".into(),
        })
        .expect("replacing an append-only section is the caller's to fix");
        assert_eq!(payload["code"], "history_section_not_replaceable");
        assert_eq!(payload["details"]["section"], "Notes");

        let payload = classify(&DomainError::HistoryEntryHeadingInvalid {
            section: "Notes".into(),
            heading: "## Too shallow".into(),
            reason: "entries use h3".into(),
        })
        .expect("an illegal heading is the caller's to fix");
        assert_eq!(payload["code"], "history_entry_heading_invalid");
        assert_eq!(
            payload["details"]["reason"], "entries use h3",
            "the reason is the recovery data — without it an agent can only guess"
        );
    }

    /// Guards the `rename_all` derive, which is now the only thing turning a
    /// variant into its wire code. Distinctness and snake_case are no longer
    /// testable properties — two variants cannot share a name, and no code is
    /// hand-written — so the 45-sample list this replaces is gone (review of
    /// #560). What a test can still lose is the attribute itself: drop
    /// `rename_all` and every code silently becomes PascalCase, breaking every
    /// client branching on it.
    #[test]
    fn the_wire_code_is_derived_as_snake_case() {
        let cases = [
            (RejectionCode::NotFound, "not_found"),
            (RejectionCode::AmbiguousAction, "ambiguous_action"),
            (
                RejectionCode::UnrepresentableFrontmatterValue,
                "unrepresentable_frontmatter_value",
            ),
            (
                RejectionCode::TrackingOnFlatStewardship,
                "tracking_on_flat_stewardship",
            ),
        ];
        for (code, expected) in cases {
            assert_eq!(
                serde_json::to_value(code).expect("code serialises"),
                json!(expected),
                "the derived wire form changed — clients branch on this string"
            );
        }
    }

    /// T7's revision rejections: each is fixed by a different call (read
    /// the note again, pick another heading or content, use the note's own
    /// commands), so each is a tool result carrying what to do next.
    #[test]
    fn the_revision_rejections_are_the_callers_to_fix() {
        let payload = classify(&DomainError::NoteNotRevisable {
            path: "projects/foo.md".into(),
            reason: "built-in".into(),
        })
        .expect("a non-revisable note is caller-actionable");
        assert_eq!(payload["code"], "note_not_revisable");
        assert_eq!(payload["details"]["path"], "projects/foo.md");

        let payload = classify(&DomainError::StaleRevision {
            path: "concepts/x.md".into(),
            expected: "aaa".into(),
            actual: "bbb".into(),
        })
        .expect("a stale hash is caller-actionable: read again");
        assert_eq!(payload["code"], "stale_revision");
        assert_eq!(payload["details"]["actual"], "bbb");

        let payload = classify(&DomainError::RevisionInvalid {
            reason: "section heading 'A | B' contains '|'".into(),
        })
        .expect("a malformed revision is caller-actionable");
        assert_eq!(payload["code"], "revision_invalid");
        assert!(
            payload["details"]["reason"]
                .as_str()
                .expect("reason")
                .contains('|')
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
