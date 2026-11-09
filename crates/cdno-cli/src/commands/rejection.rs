//! Rejection objects shared by the create verbs: the `{code, message, details}`
//! shape `--json` prints for a refused explicit `--slug`.

/// The `{code, message, details}` object for a refused explicit `--slug`
/// (RFC 0007 §5.7, §6.2), or `None` for any other domain error.
pub fn slug_rejection(err: &cdno_domain::error::DomainError) -> Option<serde_json::Value> {
    use cdno_domain::error::DomainError;
    let (code, details) = match err {
        DomainError::InvalidSlug { slug, why } => (
            "invalid_slug",
            serde_json::json!({ "slug": slug, "why": why.to_string() }),
        ),
        DomainError::SlugTaken { slug, holder, kind } => (
            "slug_taken",
            serde_json::json!({
                "slug": slug,
                "holder": holder.to_string(),
                "kind": kind.as_str(),
            }),
        ),
        DomainError::PortfolioSlugMismatch {
            question_slug,
            slug,
        } => (
            "portfolio_slug_mismatch",
            serde_json::json!({ "question_slug": question_slug, "slug": slug }),
        ),
        DomainError::SlugGivenTwice { field } => {
            ("slug_given_twice", serde_json::json!({ "field": field }))
        }
        _ => return None,
    };
    Some(serde_json::json!({
        "code": code,
        "message": err.to_string(),
        "details": details,
    }))
}

/// Where a create verb's error leaves the handler: under `--json` a slug
/// refusal is printed as its rejection object and the process exits
/// non-zero (as `reject_json` does); any other error, and every error in
/// text mode, is returned unchanged for the usual `Error: …` rendering.
/// Looks through `.context(..)`, so call it on the contextualised error.
pub fn slug_refusal(err: anyhow::Error, json: bool) -> anyhow::Error {
    if json
        && let Some(object) = err
            .downcast_ref::<cdno_domain::error::DomainError>()
            .and_then(slug_rejection)
        && let Err(printing) = crate::commands::action::reject_json(object)
    {
        return printing;
    }
    err
}

/// The `{code, message, details}` object for a refused `cdno note rename`
/// (RFC 0007 §6.4): the slug refusals of [`slug_rejection`], plus the
/// codes only a rename raises. `None` for any other domain error.
pub fn rename_rejection(err: &cdno_domain::error::DomainError) -> Option<serde_json::Value> {
    use cdno_domain::error::DomainError;
    let (code, details) = match err {
        DomainError::NoteNotRenamable { path, reason } => (
            "note_not_renamable",
            serde_json::json!({ "path": path, "reason": reason }),
        ),
        DomainError::StemShared { slug, path, others } => (
            "stem_shared",
            serde_json::json!({
                "slug": slug,
                "path": path.to_string(),
                "others": others.iter().map(ToString::to_string).collect::<Vec<_>>(),
            }),
        ),
        DomainError::StalePlan { expected, actual } => (
            "stale_plan",
            serde_json::json!({ "expected": expected, "actual": actual }),
        ),
        DomainError::ReferenceDrift { path, drift } => (
            "reference_drift",
            serde_json::json!({ "path": path.to_string(), "drift": drift.to_string() }),
        ),
        DomainError::ReferenceUnaddressable { path, key } => (
            "reference_unaddressable",
            serde_json::json!({ "path": path.to_string(), "key": key }),
        ),
        DomainError::RenameIncomplete { failed, source } => (
            "rename_incomplete",
            serde_json::json!({
                "paths": failed.iter().map(ToString::to_string).collect::<Vec<_>>(),
                "source": source.to_string(),
            }),
        ),
        DomainError::BehindUpstream { behind } => {
            // The domain carries the fact; the remedy is this interface's.
            let remedy = crate::freshness::BEHIND_REMEDY;
            return Some(serde_json::json!({
                "code": "behind_upstream",
                "message": format!("{err}; {remedy}"),
                "details": { "behind": behind, "remedy": remedy },
            }));
        }
        DomainError::GitOperationInProgress { reason } => (
            "git_operation_in_progress",
            serde_json::json!({ "reason": reason }),
        ),
        other => return slug_rejection(other),
    };
    Some(serde_json::json!({
        "code": code,
        "message": err.to_string(),
        "details": details,
    }))
}

/// The `{code, message, details}` object for a Typst creation refusal
/// (RFC 0008 §5.7, §6.4), shaped as the MCP emits it: the message ends on
/// the remedy and `details` carries the facts plus the `remedy` itself.
/// `None` for any other domain error.
pub fn typst_rejection(err: &cdno_domain::error::DomainError) -> Option<serde_json::Value> {
    use cdno_domain::error::DomainError;
    let (code, mut details) = match err {
        DomainError::BodyCarriesMetadataBlock => {
            ("body_carries_metadata_block", serde_json::json!({}))
        }
        DomainError::BodyDoesNotParse { line, message } => (
            "body_does_not_parse",
            serde_json::json!({ "line": line, "message": message }),
        ),
        DomainError::LinkNeedsEscape { line, leaf } => (
            "link_needs_escape",
            serde_json::json!({ "line": line, "leaf": leaf }),
        ),
        DomainError::FormatUnsupportedForType { note_type } => (
            "format_unsupported_for_type",
            serde_json::json!({ "note_type": note_type }),
        ),
        DomainError::VarsUnsupportedForTypst => {
            ("vars_unsupported_for_typst", serde_json::json!({}))
        }
        DomainError::StemTakenByOtherFormat { stem, holder } => (
            "stem_taken_by_other_format",
            serde_json::json!({ "stem": stem, "holder": holder.to_string() }),
        ),
        DomainError::AssetPathOutsideFolder { path, line } => (
            "asset_path_outside_folder",
            serde_json::json!({ "path": path, "line": line }),
        ),
        DomainError::AdoptionRefused { .. } => ("adoption_refused", serde_json::json!({})),
        DomainError::AssetPathNotLiteral { call, line } => (
            "asset_path_not_literal",
            serde_json::json!({ "call": call, "line": line }),
        ),
        _ => return None,
    };
    let remedy = err.remedy();
    if let (Some(remedy), Some(map)) = (remedy, details.as_object_mut()) {
        map.insert("remedy".to_owned(), serde_json::json!(remedy));
    }
    Some(serde_json::json!({
        "code": code,
        "message": typst_message(err),
        "details": details,
    }))
}

/// The refusal's text with its remedy, as the MCP renders it.
pub fn typst_message(err: &cdno_domain::error::DomainError) -> String {
    match err.remedy() {
        Some(remedy) => format!("{err}. {remedy}"),
        None => err.to_string(),
    }
}

/// Where a Typst-capable verb's error leaves the handler: a Typst refusal
/// is printed under `--json` as its rejection object (exit 1, as
/// `reject_json` does) and otherwise rendered as the message plus its
/// remedy; every other error is returned unchanged. Looks through
/// `.context(..)`.
pub fn typst_refusal(err: anyhow::Error, json: bool) -> anyhow::Error {
    let Some(object) = err
        .downcast_ref::<cdno_domain::error::DomainError>()
        .and_then(typst_rejection)
    else {
        return err;
    };
    if json && let Err(printing) = crate::commands::action::reject_json(object.clone()) {
        return printing;
    }
    anyhow::anyhow!("{}", object["message"].as_str().unwrap_or("refused"))
}
