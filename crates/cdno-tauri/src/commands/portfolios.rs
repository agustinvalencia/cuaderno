//! Portfolio Browser (M8, plan §1.6; #58): the selector list behind
//! `/portfolios`, the composed detail behind `/portfolios/:slug`, and
//! the one write — evidence quick-add, the app's only note-creation
//! form (sanctioned by #58).
//!
//! The detail is a knowledge surface: a portfolio's unifying question,
//! the project and questions it links, and every evidence note filed
//! under it. The quick-add composer is the single place the GUI mints a
//! note, so it tightens the domain's looser `origin` contract (see
//! [`add_evidence_impl`]).

use std::collections::HashMap;

use chrono::{Local, NaiveDate, NaiveDateTime};

use cdno_core::extractors::extract_wikilinks;
use cdno_core::path::VaultPath;
use cdno_domain::Vault;
use cdno_domain::vault::PortfolioSummary;

use crate::error::CmdError;
use crate::events::VaultArea;
use crate::state::AppState;
use crate::with_vault::with_vault;

use super::actions::record_and_emit;

/// The composed Portfolio Detail view-model (plan §1.6). One invoke
/// backs the whole `/portfolios/:slug` page: the portfolio's unifying
/// question, its links (the `project:` frontmatter and the related
/// questions the body wikilinks), and every filed evidence note as a
/// wire-ready row, newest first.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct PortfolioDetail {
    pub slug: String,
    /// The unifying question the portfolio collects evidence against
    /// (the `question` frontmatter field, shown as the page title).
    pub question: String,
    pub created: NaiveDate,
    /// Bare wikilink target of the linked project (brackets stripped,
    /// e.g. `"projects/surrogate-model"`), or `None` for a standalone
    /// portfolio. The frontend navigates to `/projects/:slug`.
    pub project: Option<String>,
    /// Bare wikilink targets of the related question notes the
    /// portfolio body links (from its `## Related Questions`), e.g.
    /// `"questions/research/foo"`. Opened in the reader on click.
    pub questions: Vec<String>,
    /// Every evidence note filed under the portfolio, newest first.
    pub evidence: Vec<EvidenceRow>,
}

/// One evidence note in the detail's timeline — the domain
/// `EvidenceFrontmatter` with its `VaultPath` lowered to a wire string
/// so the frontend can open the note in the reader and render its date,
/// source, and origin chip.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct EvidenceRow {
    pub path: String,
    pub created: NaiveDate,
    pub source: String,
    /// Bare wikilink target of the evidence `origin` (brackets
    /// stripped), e.g. `"projects/surrogate-model"`. The frontend
    /// resolves it to open the producing note in the reader.
    pub origin: String,
}

/// Strip the `[[…]]` wrapper from a stored wikilink and drop any
/// `|label`, yielding the bare target the frontend resolves/navigates
/// on (`"[[projects/foo|Foo]]"` → `"projects/foo"`). Idempotent on an
/// already-bare target, so it is safe to run on frontmatter that a hand
/// edit left unbracketed. `#[doc(hidden)] pub` so the unit-test seam can
/// exercise its edge cases directly.
#[doc(hidden)]
pub fn strip_wikilink(raw: &str) -> String {
    let inner = raw.trim().trim_start_matches("[[").trim_end_matches("]]");
    match inner.find('|') {
        Some(pipe) => inner[..pipe].trim().to_owned(),
        None => inner.trim().to_owned(),
    }
}

/// The `_index.md` path for a portfolio slug. A `..`/absolute slug is
/// rejected by the `VaultPath` guard and surfaces as `Invalid`.
fn portfolio_index_path(slug: &str) -> Result<VaultPath, CmdError> {
    VaultPath::new(format!("{}/{slug}/_index.md", cdno_core::paths::PORTFOLIOS))
        .map_err(|e| CmdError::Invalid(e.to_string()))
}

/// One [`PortfolioSummary`] per indexed portfolio, sorted by slug,
/// stamped as of `today` for the staleness line. Public + synchronous —
/// the test seam. Pure read: no journal, no events.
pub fn list_portfolios_impl(
    vault: &Vault,
    today: NaiveDate,
) -> Result<Vec<PortfolioSummary>, CmdError> {
    Ok(vault.list_portfolios(today)?)
}

/// Compose the Portfolio Detail bundle. Public + synchronous — the test
/// seam, exercised directly over the Memory doubles.
///
/// Three domain reads: the typed frontmatter (question, created,
/// project), the `_index.md` body (scanned for related-question
/// wikilinks — the portfolio ↔ question link lives in the body, not the
/// frontmatter), and the evidence contents (already newest-first). The
/// leading `get_portfolio` is also the missing-portfolio guard
/// (`get_portfolio_contents` returns empty for a bad slug, so it can't
/// tell "empty" from "missing" on its own).
pub fn get_portfolio_impl(vault: &Vault, slug: &str) -> Result<PortfolioDetail, CmdError> {
    let fm = vault.get_portfolio(slug)?;

    // Related questions live in the body's `## Related Questions`
    // section as `[[questions/…]]` wikilinks, never in the frontmatter
    // (only `project` is a frontmatter link). Scan the whole body and
    // keep the question-note targets: any question the portfolio links
    // is a related one, and filtering by the `questions/` prefix is more
    // robust than pinning the exact heading a hand edit might rename.
    let note = vault.read_note(&portfolio_index_path(slug)?)?;
    let questions = extract_wikilinks(&note.body)
        .into_iter()
        .map(|w| w.target)
        .filter(|t| t.starts_with("questions/"))
        .collect();

    let evidence = vault
        .get_portfolio_contents(slug)?
        .into_iter()
        .map(|(path, ef)| EvidenceRow {
            path: path.to_string(),
            created: ef.created,
            source: ef.source,
            origin: strip_wikilink(&ef.origin),
        })
        .collect();

    Ok(PortfolioDetail {
        slug: slug.to_owned(),
        question: fm.question,
        created: fm.created,
        project: fm.project.as_deref().map(strip_wikilink),
        questions,
        evidence,
    })
}

/// File one evidence note into `portfolio` — the quick-add composer's
/// write. Returns the new note's path. Public + synchronous — the test
/// seam.
///
/// A deliberate tightening over the MCP `file_to_portfolio` tool: the
/// domain does **not** validate `origin` (a wrong slug writes a dangling
/// `[[…]]` link that only lint later notices), so we resolve it here
/// first and refuse an unresolvable target. The desktop composer
/// free-texts `origin`, and a silent dangling link is exactly the quiet
/// data rot the GUI should prevent — the CLI/MCP surfaces keep the
/// looser contract for scripted callers who know what they're linking.
pub fn add_evidence_impl(
    vault: &Vault,
    now: NaiveDateTime,
    portfolio: &str,
    source: &str,
    origin: &str,
    content: &str,
) -> Result<VaultPath, CmdError> {
    let origin = origin.trim();
    // Resolve before writing so an origin that can't be pinned to one
    // note is refused rather than persisted as a dangling link (the
    // tightening above). `resolve_wikilink` returns `None` for BOTH a
    // no-match and an ambiguous stem (two notes share the last segment),
    // so the message covers both truthfully and points at the fix — the
    // full `folder/slug` path disambiguates either way.
    if vault.resolve_wikilink(origin)?.is_none() {
        return Err(CmdError::Invalid(format!(
            "origin does not resolve to a single note: [[{origin}]] — use the note's full path (folder/slug)"
        )));
    }
    // Mirror the MCP handler's plain-evidence path (`file_evidence_with_vars`);
    // the composer gathers no prompted variables, so the map is empty.
    Ok(vault.file_evidence_with_vars(now, portfolio, source, origin, content, &HashMap::new())?)
}

/// Every indexed portfolio with its staleness line — the selector
/// behind `/portfolios`. Pure read.
#[tauri::command]
pub async fn list_portfolios(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PortfolioSummary>, CmdError> {
    let today = Local::now().date_naive();
    with_vault(&state.vault, move |vault| {
        list_portfolios_impl(vault, today)
    })
    .await?
}

/// The composed Portfolio Detail read behind `/portfolios/:slug`.
#[tauri::command]
pub async fn get_portfolio(
    state: tauri::State<'_, AppState>,
    slug: String,
) -> Result<PortfolioDetail, CmdError> {
    with_vault(&state.vault, move |vault| get_portfolio_impl(vault, &slug)).await?
}

/// File an evidence note into a portfolio — the quick-add composer. An
/// unresolvable `origin` comes back as `Invalid` (see
/// [`add_evidence_impl`]); the caller shows the message inline.
#[tauri::command]
pub async fn add_evidence<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    portfolio: String,
    source: String,
    origin: String,
    content: String,
) -> Result<(), CmdError> {
    let now: NaiveDateTime = Local::now().naive_local();
    let path = with_vault(&state.vault, move |vault| {
        add_evidence_impl(vault, now, &portfolio, &source, &origin, &content)
    })
    .await??;
    record_and_emit(&app, &state, vec![path], vec![VaultArea::Portfolios]);
    Ok(())
}
