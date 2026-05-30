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

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::Context;
use crate::note_type::NoteType;
use crate::recurrence::Recurrence;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;

const STEWARDSHIP_TEMPLATE: &str = include_str!("../../templates/stewardship.md");

/// Heading of the section that holds periodic commitment lines on a
/// stewardship dashboard (design §5.6).
pub(in crate::vault) const PERIODIC_COMMITMENTS_SECTION: &str = "Periodic Commitments";

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

        let content = render_stewardship_template(name, context);
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

        let content = render_stewardship_template(name, context);
        write_stewardship(self, &slug, &expanded_path, &content)
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

        let mut tx = self.transaction();
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
        let (_slug, flat_path, expanded_path) = resolve_paths(slug)?;
        let flat_exists = self.store.exists(&flat_path)?;
        let expanded_exists = self.store.exists(&expanded_path)?;
        match (flat_exists, expanded_exists) {
            (true, true) => Err(DomainError::AmbiguousSlug(slug.to_owned())),
            (true, false) => Ok(flat_path),
            (false, true) => Ok(expanded_path),
            (false, false) => Err(DomainError::Store(StoreError::NotFound(format!(
                "stewardships/{slug}(.md or /_index.md)",
            )))),
        }
    }
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
    let mut tx = vault.transaction();
    tx.write_file(path.clone(), content.to_owned());
    tx.upsert_note(entry);
    tx.commit()?;
    Ok(path.clone())
}

/// Render the built-in stewardship template with the name (carried
/// to the H1 verbatim) and the life context.
fn render_stewardship_template(name: &str, context: Context) -> String {
    STEWARDSHIP_TEMPLATE
        .replace("{{name}}", name)
        .replace("{{context}}", context.as_str())
}
