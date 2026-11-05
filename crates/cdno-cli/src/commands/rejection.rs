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
