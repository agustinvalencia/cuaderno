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

use anyhow::{Context, Result};
use chrono::NaiveDate;

use cdno_domain::{SearchFilters, SearchResultEntry};

use crate::bootstrap;

// Thin CLI passthrough of the search flags plus `--json`; bundling them
// into a struct would add indirection for no real gain (same rationale
// as `file::run` / `commit::run`).
#[allow(clippy::too_many_arguments)]
pub fn run(
    root: &Path,
    query: &str,
    note_type: Option<String>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    portfolio: Option<String>,
    limit: usize,
    json: bool,
) -> Result<()> {
    let mut filters = SearchFilters {
        date_from: from,
        date_to: to,
        portfolio,
        ..Default::default()
    };
    // Any built-in or config-defined custom type name; an unknown name simply
    // matches nothing (search is lenient — no error). Tab-completion offers the
    // vault's known types.
    if let Some(raw) = note_type {
        filters.note_type_names.push(raw);
    }

    if json {
        // Emit the raw hits (#227), in the same best-first order the text
        // renderer uses (the result Vec's order, which serde preserves).
        let results = search_hits(root, query, &filters, limit)?;
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        print!("{}", build_search(root, query, &filters, limit)?);
    }
    Ok(())
}

/// Open the vault and run the search. The shared seam behind both output
/// modes so the JSON and text paths can't drift on filters/order/limit.
fn search_hits(
    root: &Path,
    query: &str,
    filters: &SearchFilters,
    limit: usize,
) -> Result<Vec<SearchResultEntry>> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    vault
        .search(query, filters, limit)
        .context("searching the vault")
}

/// Open the vault, run the search, and render the hits to a string.
/// Separate from [`run`] so tests can assert on the formatted output.
pub fn build_search(
    root: &Path,
    query: &str,
    filters: &SearchFilters,
    limit: usize,
) -> Result<String> {
    let results = search_hits(root, query, filters, limit)?;
    Ok(render(query, &results))
}

/// Render search hits, ranked best-first. Pure for testability.
pub fn render(query: &str, results: &[SearchResultEntry]) -> String {
    if results.is_empty() {
        return format!("Search: {query}\n  (no matches)\n");
    }
    // One row per hit: a pinned rank column beside a block cell holding
    // `title · type` / path / snippet. The block reflows to the terminal,
    // so a long snippet wraps under the hit instead of overflowing (#153).
    let mut table = crate::output::styled_table();
    for (i, r) in results.iter().enumerate() {
        let title = r.title.as_deref().unwrap_or("(untitled)");
        let mut block = format!(
            "{title}  \u{00b7}  {}\n{}",
            r.note_type,
            r.path.as_path().display()
        );
        // Collapse the snippet's internal whitespace/newlines so the cell
        // re-wraps it cleanly to the available width.
        let snippet: String = r.snippet.split_whitespace().collect::<Vec<_>>().join(" ");
        if !snippet.is_empty() {
            block.push('\n');
            block.push_str(&snippet);
        }
        table.add_row(vec![format!("{}.", i + 1), block]);
    }
    crate::output::no_wrap_columns(&mut table, &[0]);
    format!("Search: {query}\n{}\n", crate::output::render(&table))
}
