//! Portfolios + evidence: knowledge-layer (design §5.4-§5.5).
//!
//! `create_portfolio` lays down an `_index.md` inside its own folder
//! at `portfolios/<slug>/`. `file_evidence` writes an evidence note
//! inside that folder. The mandatory `origin:` wikilink on evidence
//! gives provenance ("which work produced this?") so projects and
//! action notes can list their evidence via the wikilink backlink
//! graph without any structural duplication.

use std::collections::HashMap;

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{EvidenceFrontmatter, PortfolioFrontmatter};
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;

/// One row in the `Vault::list_portfolios` output. Aggregates per-
/// portfolio metadata that's expensive to recompute by hand: the
/// number of evidence notes filed into the folder, the most recent
/// `created` date among them, and a derived staleness in days.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioSummary {
    pub slug: String,
    pub question: String,
    pub evidence_count: usize,
    /// `created` date of the most recent evidence note in the folder,
    /// or `None` for a portfolio that has no evidence yet.
    pub last_updated: Option<NaiveDate>,
    /// Days from `today` (passed into `list_portfolios`) back to
    /// `last_updated`. `None` when there's no evidence to measure
    /// against. Negative for evidence dated in the future (rare;
    /// mostly catches typos).
    pub staleness_days: Option<i64>,
}

const PORTFOLIO_TEMPLATE: &str = include_str!("../../templates/portfolio.md");
const EVIDENCE_TEMPLATE: &str = include_str!("../../templates/evidence.md");

impl Vault {
    /// Create a new portfolio at `portfolios/<slug>/_index.md` from
    /// the portfolio template. The slug is derived from `question`;
    /// the folder is implicit (the store creates parents on write).
    ///
    /// `project` is an optional wikilink target — the caller passes
    /// the bare path (e.g. `"projects/surrogate-model"`), the domain
    /// wraps it in `[[…]]` for the frontmatter. Same convention as
    /// `create_project`'s `core_question`.
    ///
    /// Errors:
    /// - [`DomainError::EmptyField`] — question is whitespace-only.
    /// - [`StoreError::AlreadyExists`] — a portfolio with the same
    ///   slug already exists.
    pub fn create_portfolio(
        &self,
        at: NaiveDateTime,
        question: &str,
        project: Option<&str>,
    ) -> Result<VaultPath, DomainError> {
        let question = question.trim();
        if question.is_empty() {
            return Err(DomainError::EmptyField { field: "question" });
        }
        let slug = slugify(question);
        let path = VaultPath::new(format!("{}/{slug}/_index.md", cdno_core::paths::PORTFOLIOS))?;
        if self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                path.to_string(),
            )));
        }

        let content = render_portfolio_template(question, at.date(), project);
        let entry = build_index_entry_for(&path, &content, NoteType::Portfolio.as_str())?;

        let mut tx = self.transaction();
        tx.write_file(path.clone(), content);
        tx.upsert_note(entry);
        tx.commit()?;

        Ok(path)
    }

    /// File an evidence note inside `portfolios/<portfolio>/`. The
    /// filename is `<YYYY-MM-DD>-<source-slug>.md`.
    ///
    /// `source` is the citation / experiment id / conversation
    /// reference; it doubles as the filename slug. `origin` is the
    /// bare wikilink target (e.g. `"projects/surrogate-model"`); the
    /// domain wraps it. Both are required — the design rationale
    /// (§5.5) is that without `origin` the backlink graph can't
    /// surface which work produced this evidence, and the field is
    /// expensive to migrate in later.
    ///
    /// Errors:
    /// - [`DomainError::EmptyField`] — `source` or `origin` is empty.
    /// - [`DomainError::MalformedWikilink`] — `origin` already
    ///   contains `[[` or `]]`; pass the bare path.
    /// - [`StoreError::NotFound`] — the parent portfolio's
    ///   `_index.md` doesn't exist.
    /// - [`StoreError::AlreadyExists`] — same-day same-source slug.
    pub fn file_evidence(
        &self,
        at: NaiveDateTime,
        portfolio: &str,
        source: &str,
        origin: &str,
        content: &str,
    ) -> Result<VaultPath, DomainError> {
        let source = source.trim();
        let origin = origin.trim();
        if source.is_empty() {
            return Err(DomainError::EmptyField { field: "source" });
        }
        if origin.is_empty() {
            return Err(DomainError::EmptyField { field: "origin" });
        }
        if origin.contains("[[") || origin.contains("]]") {
            return Err(DomainError::MalformedWikilink {
                value: origin.to_owned(),
            });
        }

        let portfolio_index = VaultPath::new(format!(
            "{}/{portfolio}/_index.md",
            cdno_core::paths::PORTFOLIOS
        ))?;
        if !self.store.exists(&portfolio_index)? {
            return Err(DomainError::Store(StoreError::NotFound(
                portfolio_index.to_string(),
            )));
        }

        let created = at.date();
        let source_slug = slugify(source);
        let filename = format!("{}-{source_slug}.md", created.format("%Y-%m-%d"));
        let path = VaultPath::new(format!(
            "{}/{portfolio}/{filename}",
            cdno_core::paths::PORTFOLIOS
        ))?;
        if self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                path.to_string(),
            )));
        }

        let body = render_evidence_template(created, source, portfolio, origin, content);
        let entry = build_index_entry_for(&path, &body, NoteType::Evidence.as_str())?;

        let mut tx = self.transaction();
        tx.write_file(path.clone(), body);
        tx.upsert_note(entry);
        tx.commit()?;

        Ok(path)
    }

    /// One [`PortfolioSummary`] per indexed portfolio, sorted by
    /// slug. Counts evidence notes and finds the most recent
    /// `created` date in a single pass over the evidence index — each
    /// evidence file is read once even when several portfolios share
    /// the scan.
    ///
    /// `today` lets the function stay pure (no `Local::now`); pass
    /// `Local::now().date_naive()` at the CLI boundary.
    ///
    /// A malformed portfolio or evidence note propagates its parse
    /// error rather than being silently skipped — lint is the place to
    /// surface partial-coverage warnings.
    pub fn list_portfolios(&self, today: NaiveDate) -> Result<Vec<PortfolioSummary>, DomainError> {
        let portfolio_entries = self.index.list_by_type(NoteType::Portfolio.as_str())?;
        let evidence_entries = self.index.list_by_type(NoteType::Evidence.as_str())?;

        // Single pass over evidence: bucket by the `portfolio` field
        // of each note's frontmatter. The field is required on
        // evidence (design §5.5), so this is the canonical grouping
        // key — robust against hand-edited filenames that don't follow
        // the `<date>-<slug>` convention.
        let mut by_portfolio: HashMap<String, (usize, Option<NaiveDate>)> = HashMap::new();
        for entry in &evidence_entries {
            let raw = self.store.read_file(&entry.path)?;
            let (fm, _body) = Frontmatter::parse(&raw)?;
            let ef = EvidenceFrontmatter::try_from(fm)?;
            let bucket = by_portfolio
                .entry(ef.portfolio.clone())
                .or_insert((0, None));
            bucket.0 += 1;
            bucket.1 = Some(match bucket.1 {
                Some(prev) => prev.max(ef.created),
                None => ef.created,
            });
        }

        let mut out = Vec::with_capacity(portfolio_entries.len());
        for p_entry in portfolio_entries {
            let slug = portfolio_slug_from_path(&p_entry.path);
            let raw = self.store.read_file(&p_entry.path)?;
            let (fm, _body) = Frontmatter::parse(&raw)?;
            let pf = PortfolioFrontmatter::try_from(fm)?;
            let (evidence_count, last_updated) =
                by_portfolio.get(&slug).copied().unwrap_or((0, None));
            let staleness_days = last_updated.map(|d| (today - d).num_days());
            out.push(PortfolioSummary {
                slug,
                question: pf.question,
                evidence_count,
                last_updated,
                staleness_days,
            });
        }
        out.sort_by(|a, b| a.slug.cmp(&b.slug));
        Ok(out)
    }

    /// Read a single portfolio's `_index.md` frontmatter. Useful for
    /// detail views (`cdno portfolio show`) where the caller needs
    /// the question, created date, and project fields that
    /// [`list_portfolios`](Self::list_portfolios) doesn't aggregate.
    /// Errors with `Store(NotFound)` when no portfolio exists at
    /// `portfolios/<slug>/_index.md`.
    pub fn get_portfolio(&self, slug: &str) -> Result<PortfolioFrontmatter, DomainError> {
        let path = VaultPath::new(format!("{}/{slug}/_index.md", cdno_core::paths::PORTFOLIOS))?;
        if !self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::NotFound(format!(
                "{path}{}",
                self.available_portfolios_hint()
            ))));
        }
        let raw = self.store.read_file(&path)?;
        let (fm, _body) = Frontmatter::parse(&raw)?;
        Ok(PortfolioFrontmatter::try_from(fm)?)
    }

    /// " — available portfolios: …" suffix for a portfolio slug not-found,
    /// listing every indexed portfolio so a caller can self-correct. See
    /// [`slug_hint::available_slugs_hint`](super::slug_hint::available_slugs_hint).
    fn available_portfolios_hint(&self) -> String {
        super::slug_hint::available_slugs_hint(
            self.index.as_ref(),
            NoteType::Portfolio.as_str(),
            "portfolios",
            |path| {
                let slug = portfolio_slug_from_path(path);
                if slug.is_empty() {
                    return None;
                }
                Some((slug.clone(), slug))
            },
        )
    }

    /// Every evidence note filed into `portfolio`, paired with its
    /// parsed frontmatter, sorted most-recent first (ties broken by
    /// path for determinism). Returns an empty vec when the portfolio
    /// has no evidence — and also when the portfolio slug doesn't
    /// match any `_index.md` (the caller can ask `list_portfolios`
    /// first if they want to distinguish "empty" from "missing").
    pub fn get_portfolio_contents(
        &self,
        portfolio: &str,
    ) -> Result<Vec<(VaultPath, EvidenceFrontmatter)>, DomainError> {
        let evidence_entries = self.index.list_by_type(NoteType::Evidence.as_str())?;
        let mut out = Vec::new();
        for entry in evidence_entries {
            let raw = self.store.read_file(&entry.path)?;
            let (fm, _body) = Frontmatter::parse(&raw)?;
            let ef = EvidenceFrontmatter::try_from(fm)?;
            if ef.portfolio == portfolio {
                out.push((entry.path, ef));
            }
        }
        // Most recent first, then path for a stable tie-break.
        out.sort_by(|a, b| {
            b.1.created
                .cmp(&a.1.created)
                .then_with(|| a.0.as_path().cmp(b.0.as_path()))
        });
        Ok(out)
    }
}

/// Extract the slug from `portfolios/<slug>/_index.md`. Returns an
/// empty string for malformed paths; callers expecting a slug have
/// already filtered to the portfolio note type.
fn portfolio_slug_from_path(path: &VaultPath) -> String {
    path.as_path()
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_owned()
}

/// Render the built-in portfolio `_index.md` template with every
/// field stamped. `project` becomes a quoted wikilink when present
/// (YAML requires the quotes around `[[…]]` to keep it from parsing
/// as a flow sequence) and `null` when absent.
fn render_portfolio_template(question: &str, created: NaiveDate, project: Option<&str>) -> String {
    let project_field = match project {
        Some(target) => format!("\"[[{target}]]\""),
        None => "null".to_owned(),
    };
    PORTFOLIO_TEMPLATE
        .replace("{{question}}", question)
        .replace("{{created}}", &created.format("%Y-%m-%d").to_string())
        .replace("{{project}}", &project_field)
}

/// Render the built-in evidence template. `origin` arrives bare and
/// is wrapped in `[[…]]` before substitution.
fn render_evidence_template(
    created: NaiveDate,
    source: &str,
    portfolio: &str,
    origin: &str,
    content: &str,
) -> String {
    EVIDENCE_TEMPLATE
        .replace("{{created}}", &created.format("%Y-%m-%d").to_string())
        .replace("{{source}}", source)
        .replace("{{portfolio}}", portfolio)
        .replace("{{origin}}", &format!("[[{origin}]]"))
        .replace("{{content}}", content.trim_end())
}
