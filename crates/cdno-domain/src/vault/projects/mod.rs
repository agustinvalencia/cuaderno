//! Project queries and operations on [`Vault`].
//!
//! Each public surface lives in its own submodule so that any single
//! file stays small enough to hold in one's head:
//!
//! - [`lifecycle`] — `active_projects`, `create_project`,
//!   `park_project`, `activate_project`. The "where does this project
//!   live, and what's its status?" operations.
//! - [`state`] — `update_project_state`, with auto-logging of the
//!   previous state to today's daily note.
//! - [`actions`] — `add_action`, `complete_action` for the
//!   `## Next Actions` section, plus the energy-tag parsing helpers.
//! - [`milestones`] — `add_milestone`, `complete_milestone` for the
//!   `## Milestones` section. Hard milestones feed the commitments
//!   aggregation query (#32).
//! - [`waiting`] — `add_waiting_on`, `resolve_waiting_on` for the
//!   `## Waiting On` section, with `(nothing yet)` placeholder
//!   round-tripping.
//!
//! This file holds the things every submodule needs: the section-name
//! constants, the shared `resolve_active_project` lookup, the
//! `rewrite_field_in_frontmatter` helper used by park/activate, and
//! the slug helpers shared across error paths.

use cdno_core::error::StoreError;
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{ProjectFrontmatter, ProjectStatus};
use crate::note_type::NoteType;

use super::Vault;

mod actions;
mod lifecycle;
mod milestones;
mod state;
mod summary;
mod waiting;

pub use actions::{ActionListEntry, AttachedAction};
pub use summary::{ProjectSummary, TopAction};

/// The heading whose body holds the project's narrative state.
/// Rewritten by `update_project_state`; the previous body is
/// auto-logged to the daily note before being replaced.
pub(super) const CURRENT_STATE_SECTION: &str = "Current State";

/// The heading whose body holds the project's open action checklist.
/// Mutated by `add_action` (append) and `complete_action` (remove).
pub(super) const NEXT_ACTIONS_SECTION: &str = "Next Actions";

/// The heading whose body holds project blockers awaiting external
/// resolution. Mutated by `add_waiting_on` and `resolve_waiting_on`.
pub(super) const WAITING_ON_SECTION: &str = "Waiting On";

/// The heading whose body holds project milestones with their target
/// or hard-deadline dates. Mutated by `add_milestone` and
/// `complete_milestone`. Hard milestones in this section feed the
/// commitments aggregation query (#32).
pub(super) const MILESTONES_SECTION: &str = "Milestones";

impl Vault {
    /// Resolve a project slug to its active file plus parsed
    /// markdown, or surface the right error when it isn't active.
    /// Used by every mutation that operates on the project body.
    pub(super) fn resolve_active_project(
        &self,
        slug: &str,
    ) -> Result<(VaultPath, MarkdownDocument), DomainError> {
        let active_path = VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS))?;
        let parked_path =
            VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS_PARKED))?;

        let path = if self.store.exists(&active_path)? {
            active_path
        } else if self.store.exists(&parked_path)? {
            return Err(DomainError::ProjectNotActive(slug.to_owned()));
        } else {
            return Err(DomainError::Store(StoreError::NotFound(format!(
                "{active_path}{}",
                self.available_projects_hint()
            ))));
        };

        let raw = self.store.read_file(&path)?;
        let doc = MarkdownDocument::parse(raw)?;
        // Defensive frontmatter check — manual edits could put a
        // non-active project under projects/. Frontmatter wins.
        let project = ProjectFrontmatter::try_from(doc.frontmatter().clone())?;
        if project.status != ProjectStatus::Active {
            return Err(DomainError::ProjectNotActive(slug.to_owned()));
        }

        Ok((path, doc))
    }

    /// Resolve a project slug to its file plus parsed markdown plus
    /// frontmatter, regardless of status. Use this for read-only
    /// queries (summary, orientation peek-ins) that want to operate
    /// on parked or completed projects too — gatekeeping by status
    /// belongs in the caller.
    ///
    /// Errors only when the slug doesn't resolve to either folder
    /// (`Store(NotFound)`) or when the file's frontmatter is
    /// malformed.
    pub(in crate::vault) fn resolve_any_project(
        &self,
        slug: &str,
    ) -> Result<(VaultPath, MarkdownDocument, ProjectFrontmatter), DomainError> {
        let active_path = VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS))?;
        let parked_path =
            VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS_PARKED))?;

        let path = if self.store.exists(&active_path)? {
            active_path
        } else if self.store.exists(&parked_path)? {
            parked_path
        } else {
            return Err(DomainError::Store(StoreError::NotFound(format!(
                "{active_path}{}",
                self.available_projects_hint()
            ))));
        };

        let raw = self.store.read_file(&path)?;
        let doc = MarkdownDocument::parse(raw)?;
        let project = ProjectFrontmatter::try_from(doc.frontmatter().clone())?;

        Ok((path, doc, project))
    }

    /// " — available projects: …" suffix for a project slug not-found,
    /// listing every indexed project (parked ones flagged) so a caller can
    /// self-correct. Shared by the resolvers, state update, and activate.
    /// See [`slug_hint::available_slugs_hint`](super::slug_hint::available_slugs_hint).
    pub(in crate::vault) fn available_projects_hint(&self) -> String {
        super::slug_hint::available_slugs_hint(
            self.index.as_ref(),
            NoteType::Project.as_str(),
            "projects",
            |path| {
                let slug = project_slug_from_path(path);
                let display = if path
                    .as_path()
                    .starts_with(cdno_core::paths::PROJECTS_PARKED)
                {
                    format!("{slug} (parked)")
                } else {
                    slug.clone()
                };
                Some((slug, display))
            },
        )
    }
}

/// Pull the slug (filename stem) out of a project path for surfacing
/// in `ProjectCapReached.active_projects` — readable without leaking
/// the folder structure into error messages.
pub(super) fn project_slug_from_path(path: &VaultPath) -> String {
    path.as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| path.to_string())
}

/// Rewrite a single field within the YAML frontmatter region of a
/// note's raw markdown. Operates only between the opening and closing
/// `---` markers so a body line containing the same prefix is
/// unaffected. Preserves the original formatting of every other
/// line — comments, key order, spacing.
///
/// `field` is the field name as it appears in YAML (e.g. `"status"`,
/// `"completed"`). `new_value` is rendered verbatim after `field: `;
/// callers needing typed values must convert to a YAML-safe string
/// first (`as_str()` for kebab-case enums, ISO-formatted dates, etc.).
///
/// Errors with [`DomainError::MissingSection`] if the frontmatter block
/// itself is missing, or [`DomainError::MissingFrontmatterField`] if the
/// block is present but doesn't contain the requested field — those
/// situations should never happen for a note that parsed via the
/// appropriate `*Frontmatter::try_from`, but the helper surfaces them
/// loudly rather than silently emitting a file with no value.
///
/// Public so the integration tests in
/// `tests/unit/projects_tests.rs` and
/// `tests/unit/commitments_tests.rs` can hit the defensive error
/// branches directly — the public `Vault::*` callers always feed it
/// pre-validated input, so those branches are unreachable through
/// the higher-level API. External callers other than tests should
/// not depend on this; treat it as a domain-internal helper.
pub fn rewrite_field_in_frontmatter(
    raw: &str,
    field: &str,
    new_value: &str,
) -> Result<String, DomainError> {
    // Locate the frontmatter region. The opening `---\n` must be at
    // the very start; the closing `\n---\n` (or `\n---` at EOF)
    // marks the end.
    let opening = "---\n";
    if !raw.starts_with(opening) {
        return Err(DomainError::MissingSection("frontmatter"));
    }
    let body_after_open = opening.len();
    let closing_offset = raw[body_after_open..]
        .find("\n---")
        .ok_or(DomainError::MissingSection("frontmatter"))?;
    let yaml_end = body_after_open + closing_offset + 1; // include the trailing \n

    let yaml = &raw[body_after_open..yaml_end];

    let prefix_compact = format!("{field}:");
    let prefix_spaced = format!("{field} :");

    let mut new_yaml = String::with_capacity(yaml.len());
    let mut found = false;
    for line in yaml.split_inclusive('\n') {
        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with(&prefix_compact) || trimmed_start.starts_with(&prefix_spaced) {
            new_yaml.push_str(field);
            new_yaml.push_str(": ");
            new_yaml.push_str(new_value);
            new_yaml.push('\n');
            found = true;
        } else {
            new_yaml.push_str(line);
        }
    }
    if !found {
        // The field name is a runtime `&str` (the setter passes a
        // config-declared key), so it can't ride the `&'static str`
        // `MissingSection` variant — carry it as an owned string instead.
        return Err(DomainError::MissingFrontmatterField(field.to_owned()));
    }

    let mut result = String::with_capacity(raw.len());
    result.push_str(&raw[..body_after_open]);
    result.push_str(&new_yaml);
    result.push_str(&raw[yaml_end..]);
    Ok(result)
}
