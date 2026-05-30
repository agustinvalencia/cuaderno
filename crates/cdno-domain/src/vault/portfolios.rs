//! Portfolios + evidence: knowledge-layer (design §5.4-§5.5).
//!
//! `create_portfolio` lays down an `_index.md` inside its own folder
//! at `portfolios/<slug>/`. `file_evidence` writes an evidence note
//! inside that folder. The mandatory `origin:` wikilink on evidence
//! gives provenance ("which work produced this?") so projects and
//! action notes can list their evidence via the wikilink backlink
//! graph without any structural duplication.

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;

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
