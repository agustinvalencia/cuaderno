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
