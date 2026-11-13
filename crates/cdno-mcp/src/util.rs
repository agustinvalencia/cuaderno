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
    ErrorData::internal_error(message(&e), data)
}

/// The client-facing message for `e`. A damaged index also wakes the
/// health loop and says what the client most needs to know: the notes
/// are safe, and the cache is being rebuilt (#526, #520).
fn message(e: &DomainError) -> String {
    if !e.is_index_corruption() {
        return e.to_string();
    }
    crate::index_health::corruption_seen();
    // Only an `IndexStale` commit has written its files: any other index
    // error on a write path comes from a lookup made before anything was
    // written, so saying "saved" there would stop a retry and lose the
    // write (PR #997 review).
    let saved = matches!(
        e,
        DomainError::Transaction(cdno_core::error::TransactionError::IndexStale(_))
    );
    format!(
        "{e} \u{2014} {INDEX_DAMAGED_HINT} {}",
        if saved {
            INDEX_DAMAGED_SAVED
        } else {
            INDEX_DAMAGED_NOT_SAVED
        }
    )
}

/// Appended to an error caused by a damaged index.
pub(crate) const INDEX_DAMAGED_HINT: &str = "the notes on disk are intact; the index (a cache of \
     them) is damaged, and the server has started rebuilding it. `index_status` shows the repair; \
     `rebuild_index` forces one.";

/// Then, for a commit whose files were written before the index failed.
const INDEX_DAMAGED_SAVED: &str = "This write was saved; do not repeat it.";

/// Then, for any other call: nothing was written.
const INDEX_DAMAGED_NOT_SAVED: &str =
    "Nothing was written by this call; repeat it once `index_status` is healthy.";

/// [`into_mcp_error`] for a start or switch: a `focus_open` rejection also
/// carries `details.attempted`, what the caller asked for, in the shape
/// `cdno action ... --json` gives (`{project, query}` or `{project, title}`).
/// Any other error passes through unchanged.
pub(crate) fn into_mcp_error_attempting(e: DomainError, attempted: serde_json::Value) -> ErrorData {
    let mut data = crate::rejection::envelope(&e);
    if let Some(rejection) = data
        .as_mut()
        .and_then(|d| d.get_mut(crate::rejection::MARKER))
        && rejection["code"] == "focus_open"
    {
        rejection["details"]["attempted"] = attempted;
    }
    ErrorData::internal_error(message(&e), data)
}

/// [`into_mcp_error`] for a pause or a resume: a `no_focus` rejection carries
/// `message`, that verb's own wording (`NO_FOCUS_TO_PAUSE_MESSAGE` or
/// `NO_FOCUS_TO_RESUME_MESSAGE`), because the shared one cannot be specific
/// to either. Any other error passes through unchanged.
pub(crate) fn into_mcp_error_no_focus(e: DomainError, message: &str) -> ErrorData {
    let mut data = crate::rejection::envelope(&e);
    if let Some(rejection) = data
        .as_mut()
        .and_then(|d| d.get_mut(crate::rejection::MARKER))
        && rejection["code"] == "no_focus"
    {
        rejection["message"] = message.into();
    }
    ErrorData::internal_error(self::message(&e), data)
}

/// Build an InvalidParams error pointing at a specific input field.
/// Used by handlers that accept enum-typed strings (e.g. the `domain`
/// filter on `get_active_questions`) and need to reject a value that
/// doesn't parse.
pub(crate) fn invalid_argument(field: &str, reason: &str) -> ErrorData {
    ErrorData::invalid_params(format!("invalid '{field}': {reason}"), None)
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

#[cfg(test)]
mod tests {
    use cdno_core::error::{IndexError, TransactionError};

    use super::*;

    fn corrupt() -> IndexError {
        IndexError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
            None,
        ))
    }

    #[test]
    fn a_damaged_index_after_a_commit_says_the_write_was_saved() {
        let e = DomainError::Transaction(TransactionError::IndexStale(vec![corrupt()]));
        let m = message(&e);
        assert!(m.contains("the notes on disk are intact"), "{m}");
        assert!(m.contains("This write was saved"), "{m}");
    }

    #[test]
    fn a_damaged_index_before_any_write_says_nothing_was_written() {
        // A lookup a write makes before it writes: telling the caller the
        // write was saved would stop the retry and lose it.
        let m = message(&DomainError::Index(corrupt()));
        assert!(m.contains("Nothing was written"), "{m}");
        assert!(!m.contains("was saved"), "{m}");
    }

    #[test]
    fn any_other_error_reads_as_before() {
        let e = DomainError::Index(IndexError::Query("boom".into()));
        assert_eq!(message(&e), e.to_string());
    }
}
