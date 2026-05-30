//! Question notes: create, lifecycle, query (design §5.8).
//!
//! Questions are the strategic anchors a researcher's projects answer
//! against. They're long-lived — created in a quiet moment, parked /
//! answered / retired during reviews — so the operations here are
//! deliberately few: create, set_status, and the `active_questions`
//! query that feeds orientation surfaces and the `cdno questions`
//! verb.
//!
//! Slugs are unique across both domains. `set_question_status`
//! resolves a slug by searching `questions/research/` and
//! `questions/life/`; a slug that exists in both is rejected as
//! ambiguous so commands like `cdno question park <slug>` can never
//! act on the wrong file.

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{QuestionDomain, QuestionFrontmatter, QuestionStatus};
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::projects::rewrite_field_in_frontmatter;
use super::slug::slugify;

/// One row in the `Vault::active_questions` output. Carries enough
/// for a renderer to display the question without re-reading the
/// file: the slug (filename stem, used for lookups), the domain
/// (for grouping), the question text (extracted from the body H1),
/// and the most recent update date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionSummary {
    pub slug: String,
    pub domain: QuestionDomain,
    /// The question text from the body H1. Empty string if the file
    /// has no H1 — lint will flag that separately.
    pub question_text: String,
    pub updated: NaiveDate,
}

const QUESTION_TEMPLATE: &str = include_str!("../../templates/question.md");

impl Vault {
    /// Create a new question at `questions/<domain>/<slug>.md` from
    /// the question template. The slug is derived from `text`; the
    /// folder is implicit (the store creates parents on write).
    ///
    /// `text` becomes the body H1. `created` and `updated` are both
    /// set to `at.date()`; `status` defaults to `active`.
    ///
    /// Errors:
    /// - [`DomainError::EmptyField`] — `text` is whitespace-only.
    /// - [`StoreError::AlreadyExists`] — a question with the same slug
    ///   already exists in *either* domain (slugs are unique across
    ///   the whole `questions/` tree so lookup-by-slug is unambiguous).
    pub fn create_question(
        &self,
        at: NaiveDateTime,
        domain: QuestionDomain,
        text: &str,
    ) -> Result<VaultPath, DomainError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(DomainError::EmptyField { field: "question" });
        }
        let slug = slugify(text);

        // Cross-domain collision check: even though we'll write under
        // `<domain>/`, the slug must be globally unique so
        // `set_question_status(slug)` resolves to exactly one file.
        for d in QuestionDomain::ALL {
            let path = question_path(d, &slug)?;
            if self.store.exists(&path)? {
                return Err(DomainError::Store(StoreError::AlreadyExists(
                    path.to_string(),
                )));
            }
        }

        let path = question_path(domain, &slug)?;
        let content = render_question_template(text, domain, at.date());
        let entry = build_index_entry_for(&path, &content, NoteType::Question.as_str())?;

        let mut tx = self.transaction();
        tx.write_file(path.clone(), content);
        tx.upsert_note(entry);
        tx.commit()?;

        Ok(path)
    }

    /// Update a question's status, bump its `updated` field, and log
    /// the transition to the daily note — all in one atomic commit.
    ///
    /// `slug` is searched across both `questions/research/` and
    /// `questions/life/`. A slug present in both surfaces as
    /// `DomainError::AmbiguousSlug` (defensive — `create_question`
    /// prevents this from arising, but a hand-edited vault could).
    ///
    /// No-op (no log entry, no file rewrite, returns the resolved
    /// path) when `new_status` equals the current status.
    ///
    /// Errors:
    /// - [`StoreError::NotFound`] — slug exists in neither domain.
    /// - [`DomainError::AmbiguousSlug`] — slug exists in both domains.
    /// - [`DomainError::MissingSection("Logs")`] — today's daily note
    ///   exists but has no `## Logs` section (mirrors
    ///   `update_project_state`; rare unless the user hand-edited).
    pub fn set_question_status(
        &self,
        at: NaiveDateTime,
        slug: &str,
        new_status: QuestionStatus,
    ) -> Result<VaultPath, DomainError> {
        let (path, current) = self.resolve_question_by_slug(slug)?;
        if current.status == new_status {
            return Ok(path);
        }

        let raw = self.store.read_file(&path)?;
        let new_content = rewrite_field_in_frontmatter(&raw, "status", new_status.as_str())?;
        let new_content =
            rewrite_field_in_frontmatter(&new_content, "updated", &at.date().to_string())?;
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Question.as_str())?;

        let log_entry = format_status_log_entry(current.domain, slug, current.status, new_status);

        let mut tx = self.transaction();
        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(path)
    }

    /// Every question with `status: active`, sorted by
    /// `(domain, slug)`. The renderer is responsible for grouping —
    /// keeping the API a flat vec matches `PortfolioSummary` and
    /// avoids leaking a `BTreeMap` shape into the public surface.
    pub fn active_questions(&self) -> Result<Vec<QuestionSummary>, DomainError> {
        let entries = self.index.list_by_type(NoteType::Question.as_str())?;
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            let raw = self.store.read_file(&entry.path)?;
            let (fm, body) = Frontmatter::parse(&raw)?;
            let qf = QuestionFrontmatter::try_from(fm)?;
            if qf.status != QuestionStatus::Active {
                continue;
            }
            out.push(QuestionSummary {
                slug: question_slug_from_path(&entry.path),
                domain: qf.domain,
                question_text: extract_h1(body),
                updated: qf.updated,
            });
        }
        // Group by domain by sorting on it first; slug tie-breaks for
        // stable output. QuestionDomain isn't Ord, so compare by str.
        out.sort_by(|a, b| {
            a.domain
                .as_str()
                .cmp(b.domain.as_str())
                .then_with(|| a.slug.cmp(&b.slug))
        });
        Ok(out)
    }

    /// Locate `slug` across both question domains, returning the
    /// resolved path and parsed frontmatter. Helper for
    /// `set_question_status` and any future read-side ops.
    fn resolve_question_by_slug(
        &self,
        slug: &str,
    ) -> Result<(VaultPath, QuestionFrontmatter), DomainError> {
        let mut found: Option<(VaultPath, QuestionFrontmatter)> = None;
        for d in QuestionDomain::ALL {
            let path = question_path(d, slug)?;
            if !self.store.exists(&path)? {
                continue;
            }
            let raw = self.store.read_file(&path)?;
            let (fm, _body) = Frontmatter::parse(&raw)?;
            let qf = QuestionFrontmatter::try_from(fm)?;
            if found.is_some() {
                return Err(DomainError::AmbiguousSlug(slug.to_owned()));
            }
            found = Some((path, qf));
        }
        found.ok_or_else(|| {
            DomainError::Store(StoreError::NotFound(format!("questions/*/{slug}.md")))
        })
    }
}

/// Vault-relative path for a question note.
fn question_path(domain: QuestionDomain, slug: &str) -> Result<VaultPath, DomainError> {
    Ok(VaultPath::new(format!(
        "questions/{}/{slug}.md",
        domain.as_str()
    ))?)
}

/// Extract the slug from `questions/<domain>/<slug>.md`. Returns an
/// empty string for malformed paths; callers expecting a slug have
/// already filtered to the question note type.
fn question_slug_from_path(path: &VaultPath) -> String {
    path.as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_owned()
}

/// Render the built-in question template with every field stamped.
fn render_question_template(text: &str, domain: QuestionDomain, today: NaiveDate) -> String {
    let date = today.format("%Y-%m-%d").to_string();
    QUESTION_TEMPLATE
        .replace("{{domain}}", domain.as_str())
        .replace("{{created}}", &date)
        .replace("{{updated}}", &date)
        .replace("{{question}}", text)
}

/// Build the daily-log entry recording a question status change.
/// Mirrors `format_state_change_log_entry` in projects/state.rs:
/// wikilink the file with its full vault path so the link survives a
/// click from the daily note, then carry the old/new pair on
/// indented continuation lines.
fn format_status_log_entry(
    domain: QuestionDomain,
    slug: &str,
    old: QuestionStatus,
    new: QuestionStatus,
) -> String {
    format!(
        "status on [[questions/{domain}/{slug}]]\n  was: {old}\n  now: {new}",
        domain = domain.as_str(),
        old = old.as_str(),
        new = new.as_str(),
    )
}

/// Return the text of the first ATX H1 line in `body`, with the
/// leading `# ` and trailing whitespace stripped. Falls back to
/// `String::new()` when no H1 is present.
fn extract_h1(body: &str) -> String {
    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            return rest.trim().to_owned();
        }
    }
    String::new()
}
