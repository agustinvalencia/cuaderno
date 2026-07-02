//! `Vault::search` — full-text content search with optional filters.
//!
//! Wraps the core FTS5 index (`VaultIndex::search`, #172 PR 1) with two
//! things the raw primitive deliberately leaves out:
//!
//! 1. **Query sanitisation.** Raw user text is turned into a safe FTS5
//!    `MATCH` expression — every whitespace-separated term is quoted, so
//!    a stray `"`, a bare `AND`/`OR`/`*`, or punctuation can't produce a
//!    syntax error. Terms are ANDed. Power-user operators aren't exposed:
//!    forgiving recall beats fragile precision for a personal recall tool.
//! 2. **Filters.** Note type, date range, and portfolio. The core index
//!    returns ranked hits unfiltered, so we over-fetch and filter here.
//!    note_type comes straight off the hit (no I/O); date and portfolio
//!    read the note's frontmatter. A date bound excludes notes with no
//!    determinable date (project maps, questions, …).

use chrono::NaiveDate;

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::index::SearchHit;
use cdno_core::path::VaultPath;

use crate::error::DomainError;

/// Upper bound on hits pulled from the index when a filter is active.
///
/// The core index ranks and limits before we can see note bodies, so to
/// keep a filtered result from coming back short we over-fetch this many
/// candidates and filter them down. At personal-vault scale a real query
/// never matches anywhere near this many notes, so the cap is a safety
/// rail, not a practical limit. (A query restricted to a rare note type
/// whose matches all rank below 500 commoner hits could in theory miss
/// some — the exact fix is pushing the note_type filter into the index
/// SQL, noted for later if it ever bites.)
const FILTERED_SCAN_CAP: usize = 500;

/// One ranked search hit, domain-side. Mirrors the core
/// [`SearchHit`](cdno_core::index::SearchHit); kept as its own type so the
/// domain surface owns its result shape (and can diverge later) rather
/// than leaking the core struct through every caller.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SearchResultEntry {
    pub path: VaultPath,
    pub note_type: String,
    pub title: Option<String>,
    pub snippet: String,
    /// Raw bm25 relevance — lower is a better match (the sort key).
    pub score: f64,
}

impl From<SearchHit> for SearchResultEntry {
    fn from(h: SearchHit) -> Self {
        Self {
            path: h.path,
            note_type: h.note_type,
            title: h.title,
            snippet: h.snippet,
            score: h.score,
        }
    }
}

/// Optional refinements applied on top of the text query. All-empty
/// (the default) means "no filtering — just the ranked text matches".
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    /// Restrict to these note types, by name. Empty = any type. Strings (not
    /// the `NoteType` enum) so config-defined custom types filter too; an
    /// unknown name simply matches nothing.
    pub note_type_names: Vec<String>,
    /// Inclusive lower bound on the note's date. `None` = no lower bound.
    pub date_from: Option<NaiveDate>,
    /// Inclusive upper bound on the note's date. `None` = no upper bound.
    pub date_to: Option<NaiveDate>,
    /// Restrict to notes belonging to this portfolio (their frontmatter
    /// `portfolio` field). `None` = any.
    pub portfolio: Option<String>,
}

impl SearchFilters {
    fn is_empty(&self) -> bool {
        self.note_type_names.is_empty()
            && self.date_from.is_none()
            && self.date_to.is_none()
            && self.portfolio.is_none()
    }

    /// Whether evaluating a hit requires reading its frontmatter. note_type
    /// is carried on the hit itself, so a type-only filter needs no I/O.
    fn needs_frontmatter(&self) -> bool {
        self.date_from.is_some() || self.date_to.is_some() || self.portfolio.is_some()
    }
}

impl super::Vault {
    /// Full-text search over note title + body, ranked best-first, with
    /// optional filtering. `query` is free user text (sanitised here into
    /// a safe FTS5 `MATCH`); at most `limit` results are returned.
    ///
    /// An empty/blank query — or one with no searchable terms — returns no
    /// results rather than erroring.
    pub fn search(
        &self,
        query: &str,
        filters: &SearchFilters,
        limit: usize,
    ) -> Result<Vec<SearchResultEntry>, DomainError> {
        let Some(match_query) = sanitise_fts_query(query) else {
            return Ok(Vec::new());
        };
        if limit == 0 {
            return Ok(Vec::new());
        }

        // With no filter, the index `LIMIT` is exact, so ask for just
        // what we need. With a filter, over-fetch and trim post-filter.
        let scan_limit = if filters.is_empty() {
            limit
        } else {
            FILTERED_SCAN_CAP
        };
        let hits = self.index.search(&match_query, scan_limit)?;

        let mut out = Vec::new();
        for hit in hits {
            if out.len() >= limit {
                break;
            }

            // Cheapest filter first: note_type lives on the hit, no I/O.
            if !filters.note_type_names.is_empty()
                && !filters.note_type_names.iter().any(|n| n == &hit.note_type)
            {
                continue;
            }

            if filters.needs_frontmatter() {
                // The index can momentarily lead the filesystem (a note
                // deleted but not yet reconciled). Treat a missing file as
                // "not a match" rather than failing the whole search; a
                // genuine I/O error still propagates.
                let raw = match self.store.read_file(&hit.path) {
                    Ok(raw) => raw,
                    Err(StoreError::NotFound(_)) => continue,
                    Err(e) => return Err(DomainError::Store(e)),
                };
                // Forgiving recall: a note whose frontmatter can't be
                // parsed (or whose field can't be read) can't be *confirmed*
                // to pass an active filter, so skip it rather than aborting
                // the whole search over one malformed note elsewhere.
                let Ok((fm, _body)) = Frontmatter::parse(&raw) else {
                    continue;
                };

                if let Some(want) = &filters.portfolio {
                    let got = fm.optional_field::<String>("portfolio").ok().flatten();
                    if got.as_deref() != Some(want.as_str()) {
                        continue;
                    }
                }

                if filters.date_from.is_some() || filters.date_to.is_some() {
                    // A note with no determinable date can't satisfy a date
                    // window, so exclude it when one is set.
                    let Some(date) = note_logical_date(&hit.note_type, &hit.path, &fm) else {
                        continue;
                    };
                    if filters.date_from.is_some_and(|from| date < from) {
                        continue;
                    }
                    if filters.date_to.is_some_and(|to| date > to) {
                        continue;
                    }
                }
            }

            out.push(SearchResultEntry::from(hit));
        }

        Ok(out)
    }
}

/// Turn free user text into a safe FTS5 `MATCH` expression, or `None` if
/// there's nothing searchable in it.
///
/// Each whitespace-separated term is wrapped in double quotes (internal
/// quotes doubled, per FTS5's escaping) and the terms are ANDed by being
/// space-joined. Quoting means the tokenizer, not the MATCH grammar,
/// handles any punctuation inside a term, so arbitrary input — `wedding
/// "venue`, `a AND b`, `c*` — is always a valid query. Terms with no
/// alphanumeric content are dropped (an all-punctuation "phrase" matches
/// nothing and can trip FTS5).
fn sanitise_fts_query(raw: &str) -> Option<String> {
    let parts: Vec<String> = raw
        .split_whitespace()
        .filter(|term| term.chars().any(|c| c.is_alphanumeric()))
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// The note's logical date for date-range filtering: a daily note's
/// filename date (`.../YYYY-MM-DD.md`), else the frontmatter `created`
/// field, else `None` — an undated note (project map, question, …), which
/// is excluded whenever a date bound is set. A malformed `created` is
/// treated as undated rather than erroring (forgiving recall). The
/// filename branch is gated to `daily` so a non-daily note that merely
/// happens to be named like a date isn't mis-dated by its filename.
fn note_logical_date(note_type: &str, path: &VaultPath, fm: &Frontmatter) -> Option<NaiveDate> {
    if note_type == "daily"
        && let Some(stem) = path.as_path().file_stem().and_then(|s| s.to_str())
        && let Ok(date) = NaiveDate::parse_from_str(stem, "%Y-%m-%d")
    {
        return Some(date);
    }
    fm.optional_field::<NaiveDate>("created").ok().flatten()
}
