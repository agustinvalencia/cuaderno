//! `cdno search <query> [--type T] [--from D] [--to D] [--portfolio P]
//! [--limit N]` — full-text content search over the vault.
//!
//! Thin terminal surface over [`cdno_domain::Vault::search`]: it maps the
//! CLI flags onto a [`SearchFilters`] and renders the ranked hits. The
//! query sanitisation, FTS ranking, and filtering all live in the domain;
//! this module only parses `--type` and formats rows. Split into a
//! `build_search` seam (like `cdno orient` / `cdno commitments`) so tests
//! assert the rendered text without capturing stdout.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use chrono::NaiveDate;

use cdno_domain::note_type::NoteType;
use cdno_domain::{SearchFilters, SearchResultEntry};

use crate::bootstrap;

pub fn run(
    root: &Path,
    query: &str,
    note_type: Option<String>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    portfolio: Option<String>,
    limit: usize,
) -> Result<()> {
    let mut filters = SearchFilters {
        date_from: from,
        date_to: to,
        portfolio,
        ..Default::default()
    };
    if let Some(raw) = note_type {
        let parsed: NoteType = raw
            .parse()
            .map_err(|e| anyhow!("invalid --type `{raw}`: {e}"))?;
        filters.note_types.push(parsed);
    }

    print!("{}", build_search(root, query, &filters, limit)?);
    Ok(())
}

/// Open the vault, run the search, and render the hits to a string.
/// Separate from [`run`] so tests can assert on the formatted output.
pub fn build_search(
    root: &Path,
    query: &str,
    filters: &SearchFilters,
    limit: usize,
) -> Result<String> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let results = vault
        .search(query, filters, limit)
        .context("searching the vault")?;
    Ok(render(query, &results))
}

/// Render search hits, ranked best-first. Pure for testability.
pub fn render(query: &str, results: &[SearchResultEntry]) -> String {
    let mut out = format!("Search: {query}\n");
    if results.is_empty() {
        out.push_str("  (no matches)\n");
        return out;
    }
    for (i, r) in results.iter().enumerate() {
        let title = r.title.as_deref().unwrap_or("(untitled)");
        out.push_str(&format!(
            "\n  {}. {title}  \u{00b7}  {}\n",
            i + 1,
            r.note_type
        ));
        out.push_str(&format!("     {}\n", r.path.as_path().display()));
        // Collapse the snippet's internal whitespace/newlines onto one
        // line so each hit stays a compact three-line block.
        let snippet: String = r.snippet.split_whitespace().collect::<Vec<_>>().join(" ");
        if !snippet.is_empty() {
            out.push_str(&format!("     {snippet}\n"));
        }
    }
    out
}
