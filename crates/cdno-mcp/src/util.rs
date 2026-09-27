//! Shared helpers for the tool handlers: result/error wrapping and a
//! couple of small parsers. Kept crate-internal — handlers in
//! `server` call these directly.

use rmcp::model::{CallToolResult, Content, ErrorData};

use cdno_domain::error::DomainError;

/// Wrap a serialisable DTO as the single content item of a successful
/// tool result. Shared by every handler so the JSON-encoding step is
/// one call site.
pub(crate) fn json_result<S: serde::Serialize>(value: S) -> Result<CallToolResult, ErrorData> {
    let content = Content::json(value)?;
    Ok(CallToolResult::success(vec![content]))
}

/// Translate a [`DomainError`] into an rmcp [`ErrorData`]. We surface
/// the domain's `Display` output as the JSON-RPC error message — it's
/// already human-readable (see `cdno-domain/src/error.rs`).
///
/// Every variant still lands as `InternalError` *here*, but that is no
/// longer the end of the story: a **caller-actionable** rejection also
/// carries a structured payload in `data`, which
/// [`crate::rejection::decode`] turns into a tool result with
/// `isError: true` at the single conversion point in
/// `CuadernoServer::call_tool` (GH #560). Mechanical failures carry no
/// payload and stay protocol errors.
///
/// So the JSON-RPC code stays uniform while the *shape* of the response
/// splits along the spec's own line — a malformed call is a protocol
/// error, an error raised inside a successful invocation is a result the
/// model can read. See [`crate::rejection`] for which is which and why
/// the classification lives one hop away from this function.
pub(crate) fn into_mcp_error(e: DomainError) -> ErrorData {
    let data = crate::rejection::envelope(&e);
    ErrorData::internal_error(e.to_string(), data)
}

/// Build an InvalidParams error pointing at a specific input field.
/// Used by handlers that accept enum-typed strings (e.g. the `domain`
/// filter on `get_active_questions`) and need to reject a value that
/// doesn't parse.
pub(crate) fn invalid_argument(field: &str, reason: &str) -> ErrorData {
    ErrorData::invalid_params(format!("invalid '{field}': {reason}"), None)
}

/// Compute the Monday of the ISO-8601 week containing `date`. ISO
/// week (Mon-Sun) rather than locale week so behaviour is identical
/// across deployments. Duplicates the domain's internal helper of the
/// same name; kept here rather than re-exporting it because each
/// handler may want a different windowing strategy in future.
pub(crate) fn monday_of_iso_week(date: chrono::NaiveDate) -> chrono::NaiveDate {
    use chrono::Datelike;
    let days_since_monday = date.weekday().num_days_from_monday() as i64;
    date - chrono::Duration::days(days_since_monday)
}

/// Parse a `period` lookback string into a `from` date relative to
/// `today`. Recognised shapes:
///
/// - `Nd` — N days back (any N >= 1)
/// - `Nw` — N weeks back (N × 7 days)
/// - `Nm` — N calendar months back (chrono `checked_sub_months`)
/// - `Ny` — N calendar years back (chrono `checked_sub_months` × 12)
///
/// Returns an `Err(reason)` string for anything off-shape; the handler
/// wraps it in `INVALID_PARAMS`. Calendar arithmetic (months / years)
/// over- or under-flowing the chrono date range also surfaces as an
/// error rather than silently saturating.
pub(crate) fn parse_period_into_from_date(
    period: &str,
    today: chrono::NaiveDate,
) -> Result<chrono::NaiveDate, String> {
    let trimmed = period.trim();
    if trimmed.is_empty() {
        return Err("empty".to_owned());
    }
    let (n_str, unit) = trimmed.split_at(trimmed.len() - 1);
    let n: u32 = n_str
        .parse()
        .map_err(|_| format!("expected `Nd|Nw|Nm|Ny`, got `{period}`"))?;
    if n == 0 {
        return Err("N must be >= 1".to_owned());
    }
    match unit {
        "d" => Ok(today - chrono::Duration::days(n as i64)),
        "w" => Ok(today - chrono::Duration::weeks(n as i64)),
        "m" => today
            .checked_sub_months(chrono::Months::new(n))
            .ok_or_else(|| format!("`{period}` overflows the supported date range")),
        "y" => today
            .checked_sub_months(chrono::Months::new(n.saturating_mul(12)))
            .ok_or_else(|| format!("`{period}` overflows the supported date range")),
        other => Err(format!("unit must be one of d, w, m, y (got `{other}`)")),
    }
}

/// Extract the question slug from a wikilink target string like
/// `"[[questions/research/key-open-question]]"`. Returns `None` for any
/// shape that isn't a `questions/<domain>/<slug>` wikilink — keeps
/// `get_project_context` silent when a project's `core_question:`
/// points somewhere odd (handled in lint, not here).
pub(crate) fn parse_question_slug_from_wikilink(link: &str) -> Option<String> {
    let inside = link.trim().strip_prefix("[[")?.strip_suffix("]]")?;
    let rest = inside.strip_prefix("questions/")?;
    // questions/<domain>/<slug> — must have at least one `/` left.
    let slug = rest.rsplit_once('/').map(|(_, slug)| slug)?;
    if slug.is_empty() {
        return None;
    }
    Some(slug.to_owned())
}
