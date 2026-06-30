//! Stewardships: perpetual responsibilities (design §5.6).
//!
//! Two on-disk variants, chosen at create time and irreversible
//! without a manual move:
//!
//! - **flat**: a single file at `stewardships/<slug>.md`. Right when
//!   the stewardship is unlikely to grow tracking entries — finances,
//!   legal admin, household.
//! - **expanded**: a folder at `stewardships/<slug>/` with `_index.md`
//!   carrying the dashboard, `tracking/` for time-series notes, and
//!   `routines/` for prescriptive reference docs. The subdirs are
//!   created lazily by the first write into them (no placeholder
//!   files) — the body of `_index.md` documents the layout.
//!
//! Creating either variant errors with `Store(AlreadyExists)` when
//! the *other* variant exists at the same slug: a slug can't be both
//! a file and a folder. This is a defensive check — `cdno
//! stewardship list` (#43) presents only one of them, so the
//! collision is unlikely outside hand edits.

use std::collections::HashMap;

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;
use cdno_core::template::VariableContext;

use crate::error::DomainError;
use crate::frontmatter::{Context, StewardshipFrontmatter, TrackingFrontmatter};
use crate::note_type::NoteType;
use crate::recurrence::Recurrence;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;

/// Heading of the section that holds periodic commitment lines on a
/// stewardship dashboard (design §5.6).
pub(in crate::vault) const PERIODIC_COMMITMENTS_SECTION: &str = "Periodic Commitments";

/// Which on-disk variant a stewardship lives as. Drives where tracking
/// notes can land (only expanded stewardships have a `tracking/`
/// subdirectory) and what `cdno stewardship show` (#44) renders.
// `lowercase` so CLI `--json` matches the MCP DTO's `flat`/`expanded`
// (dto.rs `stewardship_variant_str`); the two JSON surfaces serialise
// from different layers, so casing is kept in sync by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StewardshipVariant {
    /// `stewardships/<slug>.md` — single file, no tracking subdir.
    Flat,
    /// `stewardships/<slug>/_index.md` — folder with optional
    /// `tracking/` and `routines/` siblings.
    Expanded,
}

/// One row in the `Vault::list_stewardships` output. Aggregates
/// per-stewardship metadata that's expensive to recompute by hand:
/// the variant, the count of tracking notes filed into the folder,
/// and the most recent tracking date (the staleness proxy that
/// stands in for a hard "status" field — design §5.6 keeps the
/// dashboard's status as prose in the body).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StewardshipSummary {
    pub slug: String,
    /// The dashboard's body H1. Empty string if absent — lint will
    /// flag that separately.
    pub name: String,
    pub context: Context,
    pub variant: StewardshipVariant,
    /// Always `0` for [`StewardshipVariant::Flat`]: flat dashboards
    /// have no `tracking/` subdir by design.
    pub tracking_count: usize,
    /// `date` of the most recent tracking note in the folder, or
    /// `None` when none exists yet.
    pub last_tracking_date: Option<NaiveDate>,
    /// Days from `today` (passed into [`list_stewardships`]) back to
    /// `last_tracking_date`. `None` when there's no tracking to
    /// measure against. Negative for future-dated tracking (rare;
    /// catches typos).
    pub staleness_days: Option<i64>,
}

impl Vault {
    /// Create a flat stewardship dashboard at `stewardships/<slug>.md`
    /// from the stewardship template. The slug is derived from
    /// `name`; the H1 carries the human-readable name verbatim.
    ///
    /// `at` is unused today but accepted in the signature so the
    /// shape is consistent with the other create ops and future
    /// auto-stamped fields (`created`, `last_reviewed`) don't change
    /// the public surface.
    ///
    /// Errors:
    /// - [`DomainError::EmptyField`] — name is whitespace-only.
    /// - [`StoreError::AlreadyExists`] — the flat path or the
    ///   expanded path for the same slug already exists.
    pub fn create_stewardship_flat(
        &self,
        _at: NaiveDateTime,
        name: &str,
        context: Context,
    ) -> Result<VaultPath, DomainError> {
        let (slug, flat_path, expanded_path) = resolve_paths(name)?;
        if self.store.exists(&flat_path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                flat_path.to_string(),
            )));
        }
        if self.store.exists(&expanded_path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                expanded_path.to_string(),
            )));
        }

        let content = self.render_stewardship(name, context)?;
        write_stewardship(self, &slug, &flat_path, &content)
    }

    /// Create an expanded stewardship at
    /// `stewardships/<slug>/_index.md`. The slug, name handling, and
    /// error contract mirror [`create_stewardship_flat`](Self::create_stewardship_flat).
    /// `tracking/` and `routines/` are created lazily by the first
    /// write into them — no placeholder files.
    pub fn create_stewardship_expanded(
        &self,
        _at: NaiveDateTime,
        name: &str,
        context: Context,
    ) -> Result<VaultPath, DomainError> {
        let (slug, flat_path, expanded_path) = resolve_paths(name)?;
        if self.store.exists(&expanded_path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                expanded_path.to_string(),
            )));
        }
        if self.store.exists(&flat_path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                flat_path.to_string(),
            )));
        }

        let content = self.render_stewardship(name, context)?;
        write_stewardship(self, &slug, &expanded_path, &content)
    }

    /// Render the stewardship template (custom or built-in): `{{name}}`
    /// carried to the H1 verbatim, `{{context}}` the life context.
    fn render_stewardship(&self, name: &str, context: Context) -> Result<String, DomainError> {
        let mut ctx = VariableContext::new();
        ctx.set_contextual("name", name);
        ctx.set_contextual("context", context.as_str());
        self.scaffold("stewardship", None, &mut ctx)
    }

    /// Append a periodic commitment to the stewardship's `## Periodic
    /// Commitments` section. The line takes the canonical wire format
    /// from design §5.6:
    ///
    /// ```text
    /// - {title} \u{2014} {recurrence} \u{2014} next: YYYY-MM-DD
    /// ```
    ///
    /// `stewardship` is the slug; both flat
    /// (`stewardships/<slug>.md`) and expanded
    /// (`stewardships/<slug>/_index.md`) variants are accepted —
    /// looked up in that order. The `at` parameter is in the
    /// signature for consistency with other vault ops; it is unused
    /// today because the line itself carries the next-due date.
    ///
    /// Errors:
    /// - [`DomainError::EmptyField`] — `title` is whitespace-only.
    /// - [`StoreError::NotFound`] — no stewardship matches the slug in
    ///   either variant.
    /// - [`DomainError::AmbiguousSlug`] — both variants exist for the
    ///   slug (defensive; `create_stewardship_*` prevents this).
    /// - [`ManipulationError::SectionNotFound`] (via
    ///   [`DomainError::Manipulation`]) — the dashboard has no
    ///   `## Periodic Commitments` section. The default template
    ///   includes it; this fires only on hand-edited dashboards.
    pub fn add_periodic_commitment(
        &self,
        _at: NaiveDateTime,
        stewardship: &str,
        title: &str,
        recurrence: Recurrence,
        next_date: NaiveDate,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let title = title.trim();
        if title.is_empty() {
            return Err(DomainError::EmptyField { field: "title" });
        }
        let path = self.resolve_stewardship_by_slug(stewardship)?;

        let raw = self.store.read_file(&path)?;
        let mut doc = MarkdownDocument::parse(raw)?;
        let line = format!(
            "- {title} \u{2014} {recurrence} \u{2014} next: {date}\n",
            date = next_date.format("%Y-%m-%d"),
        );
        doc.append_to_section(PERIODIC_COMMITMENTS_SECTION, &line)?;
        let new_content = doc.render().to_owned();
        let entry_meta =
            build_index_entry_for(&path, &new_content, NoteType::Stewardship.as_str())?;

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        tx.commit()?;

        Ok(path)
    }

    /// Locate the stewardship file for `slug`, trying flat first then
    /// expanded. Surfaces an unambiguous error when both exist —
    /// `create_stewardship_*` already forbids that case, but a
    /// hand-edited vault could.
    pub(in crate::vault) fn resolve_stewardship_by_slug(
        &self,
        slug: &str,
    ) -> Result<VaultPath, DomainError> {
        let (resolved, _variant) = self.resolve_stewardship_with_variant(slug)?;
        Ok(resolved)
    }

    /// Like [`resolve_stewardship_by_slug`](Self::resolve_stewardship_by_slug)
    /// but also returns which variant matched — needed by the
    /// tracking-note op (#43) and by `cdno stewardship show` (#44).
    pub(in crate::vault) fn resolve_stewardship_with_variant(
        &self,
        slug: &str,
    ) -> Result<(VaultPath, StewardshipVariant), DomainError> {
        let (_slug, flat_path, expanded_path) = resolve_paths(slug)?;
        let flat_exists = self.store.exists(&flat_path)?;
        let expanded_exists = self.store.exists(&expanded_path)?;
        match (flat_exists, expanded_exists) {
            (true, true) => Err(DomainError::AmbiguousSlug(slug.to_owned())),
            (true, false) => Ok((flat_path, StewardshipVariant::Flat)),
            (false, true) => Ok((expanded_path, StewardshipVariant::Expanded)),
            (false, false) => Err(DomainError::Store(StoreError::NotFound(format!(
                "stewardships/{slug}(.md or /_index.md){hint}",
                hint = self.available_stewardships_hint(),
            )))),
        }
    }

    /// A human-readable " — available stewardships: …" suffix listing
    /// the slugs that *do* exist, expanded ones flagged (only those
    /// accept tracking notes). Appended to the not-found error so a
    /// caller — or an agent driving the MCP server — sees the valid
    /// set and can self-correct instead of guessing again (the failure
    /// mode that motivated this: a client invented `fitness` when the
    /// real slug was `gym`).
    ///
    /// Best-effort and index-derived: it reflects the last reconcile, so a
    /// not-yet-indexed file could be omitted; an index read error yields an
    /// empty suffix rather than masking the original not-found. Startup
    /// reconciliation normally keeps this current.
    fn available_stewardships_hint(&self) -> String {
        // Expanded stewardships are flagged — only they accept tracking
        // notes — but both sort by the bare slug.
        crate::vault::slug_hint::available_slugs_hint(
            self.index.as_ref(),
            NoteType::Stewardship.as_str(),
            "stewardships",
            |path| {
                let slug = stewardship_slug_from_path(path);
                let display = match stewardship_variant_from_path(path) {
                    StewardshipVariant::Expanded => format!("{slug} (expanded)"),
                    StewardshipVariant::Flat => slug.clone(),
                };
                Some((slug, display))
            },
        )
    }

    /// Read a single stewardship's dashboard. Returns the typed
    /// frontmatter, the body markdown (everything after the closing
    /// `---`), and the variant. Used by detail views (`cdno
    /// stewardship show`) that need the body sections in addition to
    /// the summary [`list_stewardships`](Self::list_stewardships)
    /// already returns. Errors with `Store(NotFound)` when no
    /// stewardship exists at the slug in either variant.
    pub fn get_stewardship(
        &self,
        slug: &str,
    ) -> Result<(StewardshipFrontmatter, String, StewardshipVariant), DomainError> {
        let (path, variant) = self.resolve_stewardship_with_variant(slug)?;
        let raw = self.store.read_file(&path)?;
        let (fm, body) = Frontmatter::parse(&raw)?;
        let typed = StewardshipFrontmatter::try_from(fm)?;
        Ok((typed, body.to_owned(), variant))
    }

    /// One [`StewardshipSummary`] per indexed stewardship, sorted by
    /// slug. Counts tracking notes and finds the most recent `date`
    /// in a single pass over the tracking index — each tracking file
    /// is read once even when several stewardships share the scan.
    ///
    /// `today` lets the function stay pure (no `Local::now`); pass
    /// `Local::now().date_naive()` at the CLI boundary.
    ///
    /// A malformed stewardship or tracking note propagates its parse
    /// error rather than being silently skipped — lint is the place
    /// to surface partial-coverage warnings.
    pub fn list_stewardships(
        &self,
        today: NaiveDate,
    ) -> Result<Vec<StewardshipSummary>, DomainError> {
        let stewardship_entries = self.index.list_by_type(NoteType::Stewardship.as_str())?;
        let tracking_entries = self.index.list_by_type(NoteType::Tracking.as_str())?;

        // Single pass over tracking: bucket by the `stewardship`
        // field of each note's frontmatter, which is the canonical
        // grouping key (robust against hand-edited filenames).
        let mut by_stewardship: HashMap<String, (usize, Option<NaiveDate>)> = HashMap::new();
        for entry in &tracking_entries {
            let raw = self.store.read_file(&entry.path)?;
            let (fm, _body) = Frontmatter::parse(&raw)?;
            let tf = TrackingFrontmatter::try_from(fm)?;
            let bucket = by_stewardship
                .entry(tf.stewardship.clone())
                .or_insert((0, None));
            bucket.0 += 1;
            bucket.1 = Some(match bucket.1 {
                Some(prev) => prev.max(tf.date),
                None => tf.date,
            });
        }

        let mut out = Vec::with_capacity(stewardship_entries.len());
        for s_entry in stewardship_entries {
            let raw = self.store.read_file(&s_entry.path)?;
            let (fm, body) = Frontmatter::parse(&raw)?;
            let sf = StewardshipFrontmatter::try_from(fm)?;
            let slug = stewardship_slug_from_path(&s_entry.path);
            let variant = stewardship_variant_from_path(&s_entry.path);
            // Flat stewardships have no tracking subdir; report zero
            // even if a defensive bucket somehow exists.
            let (tracking_count, last_tracking_date) = match variant {
                StewardshipVariant::Flat => (0, None),
                StewardshipVariant::Expanded => {
                    by_stewardship.get(&slug).copied().unwrap_or((0, None))
                }
            };
            let staleness_days = last_tracking_date.map(|d| (today - d).num_days());
            out.push(StewardshipSummary {
                slug,
                name: extract_h1(body),
                context: sf.context,
                variant,
                tracking_count,
                last_tracking_date,
                staleness_days,
            });
        }
        out.sort_by(|a, b| a.slug.cmp(&b.slug));
        Ok(out)
    }
}

/// Extract the slug from a stewardship's path. Handles both flat
/// (`stewardships/<slug>.md`) and expanded
/// (`stewardships/<slug>/_index.md`) layouts.
pub(in crate::vault) fn stewardship_slug_from_path(path: &VaultPath) -> String {
    let p = path.as_path();
    if p.file_name().and_then(|s| s.to_str()) == Some("_index.md") {
        return p
            .parent()
            .and_then(|d| d.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_owned();
    }
    p.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_owned()
}

/// Infer the variant from a stewardship's path: `_index.md` filename
/// signals expanded; anything else signals flat.
pub(in crate::vault) fn stewardship_variant_from_path(path: &VaultPath) -> StewardshipVariant {
    if path.as_path().file_name().and_then(|s| s.to_str()) == Some("_index.md") {
        StewardshipVariant::Expanded
    } else {
        StewardshipVariant::Flat
    }
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

/// Compute the canonical slug and both on-disk paths for a stewardship
/// `name`. Done once so the two create ops share the slug-derivation
/// and the empty-name check.
fn resolve_paths(name: &str) -> Result<(String, VaultPath, VaultPath), DomainError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(DomainError::EmptyField { field: "name" });
    }
    let slug = slugify(name);
    let flat = VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::STEWARDSHIPS))?;
    let expanded = VaultPath::new(format!(
        "{}/{slug}/_index.md",
        cdno_core::paths::STEWARDSHIPS
    ))?;
    Ok((slug, flat, expanded))
}

/// Stage the file write + index upsert for a stewardship and commit
/// the transaction. Shared between flat and expanded — they differ
/// only in the path passed in.
fn write_stewardship(
    vault: &Vault,
    _slug: &str,
    path: &VaultPath,
    content: &str,
) -> Result<VaultPath, DomainError> {
    let entry = build_index_entry_for(path, content, NoteType::Stewardship.as_str())?;
    let mut tx = vault.transaction()?;
    tx.write_file(path.clone(), content.to_owned());
    tx.upsert_note(entry);
    tx.commit()?;
    Ok(path.clone())
}
