//! Portfolios + evidence: knowledge-layer (design §5.4-§5.5).
//!
//! `create_portfolio` lays down an `_index.md` inside its own folder
//! at `portfolios/<slug>/`. `file_evidence` writes an evidence note
//! inside that folder. The mandatory `origin:` wikilink on evidence
//! gives provenance ("which work produced this?") so projects and
//! action notes can list their evidence via the wikilink backlink
//! graph without any structural duplication.

use std::collections::HashMap;
use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;
use cdno_core::transaction::VaultTransaction;

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

/// Heading in a question note that lists the portfolios collecting
/// evidence against it. `create_portfolio` appends the backlink here.
const RELATED_PORTFOLIOS_SECTION: &str = "Related Portfolios";

/// Heading in a portfolio `_index.md` that lists the question(s) it
/// collects evidence against. The reciprocal of
/// [`RELATED_PORTFOLIOS_SECTION`] — both ends of the link are written
/// together so the navigation works in either direction (#200).
const RELATED_QUESTIONS_SECTION: &str = "Related Questions";

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
    /// When a question note shares the portfolio's slug, the same
    /// commit links the two **both ways** (#200): the new portfolio's
    /// `## Related Questions` section gains a
    /// `[[questions/<domain>/<slug>]]` bullet, and the question note's
    /// `## Related Portfolios` gains a `[[portfolios/<slug>]]` bullet.
    /// A portfolio whose question has no note (a standalone capture)
    /// gets neither and commits unchanged.
    ///
    /// Errors:
    /// - [`DomainError::EmptyField`] — question is whitespace-only.
    /// - [`StoreError::AlreadyExists`] — a portfolio with the same
    ///   slug already exists.
    /// - [`DomainError::AmbiguousSlug`] — the slug exists as a
    ///   question in *both* domains (a hand-edited vault; normally
    ///   unreachable since `create_question` rejects cross-domain
    ///   dupes).
    pub fn create_portfolio(
        &self,
        at: NaiveDateTime,
        question: &str,
        project: Option<&str>,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
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

        let mut content = render_portfolio_template(question, at.date(), project);

        // Bidirectional link when a question note shares the slug. The
        // portfolio side is written into the fresh template (cheaper
        // than a read-modify-write of a file we're creating); the
        // question side is staged onto the same tx so both ends land
        // atomically — or neither does.
        if let Some(question_path) = self.find_question_path(&slug)? {
            let mut doc = MarkdownDocument::parse(content)?;
            append_wikilink_to_section(
                &mut doc,
                RELATED_QUESTIONS_SECTION,
                &note_wikilink_target(&question_path),
            )?;
            content = doc.render().to_owned();
            self.stage_backlink_into_note(
                &question_path,
                RELATED_PORTFOLIOS_SECTION,
                &format!("portfolios/{slug}"),
                NoteType::Question.as_str(),
                &mut tx,
            )?;
        }

        let entry = build_index_entry_for(&path, &content, NoteType::Portfolio.as_str())?;
        tx.write_file(path.clone(), content);
        tx.upsert_note(entry);
        tx.commit()?;

        Ok(path)
    }

    /// Link an *existing* portfolio to an *existing* question — the
    /// retrofit counterpart to the linking `create_portfolio` does
    /// automatically (#200). Reach for it when the portfolio predates
    /// the question, or when the two slugs differ (so the create-time
    /// 1:1 match never fired). Writes **both** ends in one commit: the
    /// question note's `## Related Portfolios` gains
    /// `[[portfolios/<portfolio>]]`, and the portfolio's `## Related
    /// Questions` gains `[[questions/<domain>/<slug>]]`.
    ///
    /// Returns the resolved question-note path. Idempotent on each
    /// end — a bullet already present is left untouched and the call
    /// still succeeds, so a question accumulates portfolios (and a
    /// portfolio accumulates questions) without duplicates.
    ///
    /// Errors:
    /// - [`StoreError::NotFound`] — no portfolio `_index.md` for
    ///   `portfolio`, or no question note for `question` (the message
    ///   lists the available slugs so the caller can self-correct).
    /// - [`DomainError::AmbiguousSlug`] — the question slug exists in
    ///   both domains (a hand-edited vault).
    pub fn link_portfolio_to_question(
        &self,
        portfolio: &str,
        question: &str,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)

        let portfolio_index = VaultPath::new(format!(
            "{}/{portfolio}/_index.md",
            cdno_core::paths::PORTFOLIOS
        ))?;
        if !self.store.exists(&portfolio_index)? {
            return Err(DomainError::Store(StoreError::NotFound(format!(
                "{portfolio_index}{}",
                self.available_portfolios_hint()
            ))));
        }

        let (question_path, _fm) = self.resolve_question_by_slug(question)?;
        // Stage both ends; either side already linked is a no-op. The
        // commit is unconditional — when nothing changed it's a harmless
        // empty commit rather than a special case to branch on.
        self.stage_backlink_into_note(
            &question_path,
            RELATED_PORTFOLIOS_SECTION,
            &format!("portfolios/{portfolio}"),
            NoteType::Question.as_str(),
            &mut tx,
        )?;
        self.stage_backlink_into_note(
            &portfolio_index,
            RELATED_QUESTIONS_SECTION,
            &note_wikilink_target(&question_path),
            NoteType::Portfolio.as_str(),
            &mut tx,
        )?;
        tx.commit()?;
        Ok(question_path)
    }

    /// Read the note at `note_path`, append a `[[<target>]]` bullet
    /// under `## <heading>`, and stage the rewrite onto `tx`. Returns
    /// `true` when the bullet was added, `false` when an identical
    /// wikilink was already present (idempotent — re-linking never
    /// duplicates). `note_type` re-stamps the index row.
    ///
    /// The shared write behind both directions of the portfolio ↔
    /// question link: the question side passes the `Related Portfolios`
    /// heading and a `portfolios/<slug>` target, the portfolio side the
    /// `Related Questions` heading and a `questions/<domain>/<slug>`
    /// target.
    fn stage_backlink_into_note(
        &self,
        note_path: &VaultPath,
        heading: &str,
        target: &str,
        note_type: &str,
        tx: &mut VaultTransaction,
    ) -> Result<bool, DomainError> {
        let raw = self.store.read_file(note_path)?;
        let mut doc = MarkdownDocument::parse(raw)?;
        if !append_wikilink_to_section(&mut doc, heading, target)? {
            return Ok(false); // already linked — don't duplicate the bullet
        }

        let new_content = doc.render().to_owned();
        let entry = build_index_entry_for(note_path, &new_content, note_type)?;
        tx.write_file(note_path.clone(), new_content);
        tx.upsert_note(entry);
        Ok(true)
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
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
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
            return Err(DomainError::Store(StoreError::NotFound(format!(
                "{portfolio_index}{}",
                self.available_portfolios_hint()
            ))));
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

        tx.write_file(path.clone(), body);
        tx.upsert_note(entry);
        tx.commit()?;

        Ok(path)
    }

    /// File a non-markdown artefact as evidence (#154): copy (or, with the
    /// CLI `--move` flag handled upstream, relocate) `artefact` into
    /// `portfolios/<portfolio>/<evidence-slug>/`, and write a flat
    /// `type: evidence` markdown **stub** beside it at
    /// `portfolios/<portfolio>/<evidence-slug>.md` that links the artefact
    /// relatively and carries a `kind` field. The stub is the indexed
    /// citizen; the artefact rides along, referenced but never parsed.
    /// `abstract_body` becomes the stub's prose (the only thing search and
    /// agents see — an empty one gets a placeholder prompting for it).
    ///
    /// Returns the stub path. Errors mirror [`file_evidence`](Self::file_evidence),
    /// plus `EmptyField { field: "attach" }` when `artefact` has no
    /// filename, and `AlreadyExists` if either the stub or the artefact
    /// destination is occupied. The copy + stub write commit atomically —
    /// a failed stub rolls the imported artefact back out.
    pub fn file_attachment(
        &self,
        at: NaiveDateTime,
        portfolio: &str,
        artefact: &Path,
        source: &str,
        origin: &str,
        abstract_body: &str,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
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
        let filename = artefact
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .ok_or(DomainError::EmptyField { field: "attach" })?;

        let portfolio_index = VaultPath::new(format!(
            "{}/{portfolio}/_index.md",
            cdno_core::paths::PORTFOLIOS
        ))?;
        if !self.store.exists(&portfolio_index)? {
            return Err(DomainError::Store(StoreError::NotFound(format!(
                "{portfolio_index}{}",
                self.available_portfolios_hint()
            ))));
        }

        let created = at.date();
        let evidence_slug = format!("{}-{}", created.format("%Y-%m-%d"), slugify(source));
        let stub_path = VaultPath::new(format!(
            "{}/{portfolio}/{evidence_slug}.md",
            cdno_core::paths::PORTFOLIOS
        ))?;
        // The artefact keeps its original filename (recognisable) inside a
        // folder named for the stub stem — the `X.md` ↔ `X/` pairing.
        let artefact_dest = VaultPath::new(format!(
            "{}/{portfolio}/{evidence_slug}/{filename}",
            cdno_core::paths::PORTFOLIOS
        ))?;
        if self.store.exists(&stub_path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                stub_path.to_string(),
            )));
        }
        if self.store.exists(&artefact_dest)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                artefact_dest.to_string(),
            )));
        }

        let kind = kind_from_extension(filename);
        let body = render_attachment_stub(
            created,
            source,
            portfolio,
            origin,
            kind,
            &evidence_slug,
            filename,
            abstract_body,
        );
        let entry = build_index_entry_for(&stub_path, &body, NoteType::Evidence.as_str())?;

        // Import first so a failed stub write rolls the artefact back out.
        tx.import_external(artefact.to_path_buf(), artefact_dest);
        tx.write_file(stub_path.clone(), body);
        tx.upsert_note(entry);
        tx.commit()?;

        Ok(stub_path)
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

/// The bare wikilink target for a note path: the vault-relative path
/// with the `.md` extension dropped (e.g.
/// `questions/research/foo.md` → `questions/research/foo`), so it
/// renders as `[[questions/research/foo]]`.
fn note_wikilink_target(path: &VaultPath) -> String {
    let s = path.to_string();
    s.strip_suffix(".md").unwrap_or(&s).to_owned()
}

/// Append a `- [[<target>]]` bullet under `## <heading>` in `doc`,
/// returning `true` when the doc changed and `false` when a line
/// already contains the `[[<target>]]` wikilink (idempotent). The
/// closing `]]` in the match guards against a slug that is a prefix of
/// another (`foo` vs `foo-bar`); the `contains` rather than exact-line
/// match also tolerates a hand-annotated bullet
/// (`- [[…]] (primary angle)`).
///
/// Normalisation mirrors `add_action`: one bullet per line and a
/// trailing blank line so the next heading stays cleanly separated.
/// `ensure_section` recreates the heading for a note that drifted from
/// its template and lost it.
fn append_wikilink_to_section(
    doc: &mut MarkdownDocument,
    heading: &str,
    target: &str,
) -> Result<bool, DomainError> {
    let bullet = format!("- [[{target}]]");
    let marker = format!("[[{target}]]");

    doc.ensure_section(heading)?;
    let existing = doc.section(heading)?.trim_end();
    if existing.lines().any(|line| line.contains(&marker)) {
        return Ok(false);
    }
    let new_section = if existing.is_empty() {
        format!("{bullet}\n\n")
    } else {
        format!("{existing}\n{bullet}\n\n")
    };
    doc.replace_section(heading, &new_section)?;
    Ok(true)
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

/// Classify an attachment by file extension into the `kind` field an
/// agent uses to decide how to re-read it (trust the abstract vs reopen
/// the artefact). Unknown extensions fall back to `"file"`.
fn kind_from_extension(filename: &str) -> &'static str {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "pdf",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "heic" | "tiff" | "bmp" => "image",
        "mp4" | "mov" | "webm" | "mkv" | "avi" => "video",
        "mp3" | "wav" | "m4a" | "flac" | "ogg" => "audio",
        "typ" => "typst",
        "tex" => "latex",
        _ => "file",
    }
}

/// Render the markdown stub for an attachment: the evidence frontmatter
/// (with `kind`), an H1 of the source (the FTS title), a relative link to
/// the artefact in its sibling folder, and the abstract. The link uses
/// angle brackets so a filename with spaces stays valid; it is a plain
/// markdown link, never a `[[wikilink]]` (the resolver only resolves
/// `.md` stems).
#[allow(clippy::too_many_arguments)]
fn render_attachment_stub(
    created: NaiveDate,
    source: &str,
    portfolio: &str,
    origin: &str,
    kind: &str,
    evidence_slug: &str,
    filename: &str,
    abstract_body: &str,
) -> String {
    let abstract_section = if abstract_body.trim().is_empty() {
        "_Abstract pending — describe the artefact so it's findable._".to_owned()
    } else {
        abstract_body.trim_end().to_owned()
    };
    // Escape for the structured contexts: `source`/`origin` go into
    // double-quoted YAML scalars, and the filename goes into an
    // angle-bracketed CommonMark link destination. The H1 keeps the raw
    // source (plain markdown text).
    let source_yaml = yaml_double_quoted_escape(source);
    let origin_yaml = yaml_double_quoted_escape(origin);
    let link_dest = link_destination_escape(filename);
    format!(
        "---\n\
         type: evidence\n\
         created: {created}\n\
         source: \"{source_yaml}\"\n\
         portfolio: {portfolio}\n\
         origin: \"[[{origin_yaml}]]\"\n\
         kind: {kind}\n\
         ---\n\
         \n\
         # {source}\n\
         \n\
         ## Attachment\n\
         \n\
         [{filename}](<./{evidence_slug}/{link_dest}>)\n\
         \n\
         ## Abstract\n\
         \n\
         {abstract_section}\n",
        created = created.format("%Y-%m-%d"),
    )
}

/// Escape `\` and `"` for embedding in a double-quoted YAML scalar, so a
/// `source`/`origin` containing a quote can't break the frontmatter.
fn yaml_double_quoted_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Escape the characters that would terminate or corrupt an
/// angle-bracketed CommonMark link destination (`<`, `>`, `\`). Spaces
/// are already valid inside `<…>`.
fn link_destination_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('<', "\\<")
        .replace('>', "\\>")
}
