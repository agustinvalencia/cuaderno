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
//! `rewrite_status_in_frontmatter` helper used by park/activate, and
//! the slug helpers shared across error paths.

use cdno_core::error::StoreError;
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{ProjectFrontmatter, ProjectStatus};

use super::Vault;

mod actions;
mod lifecycle;
mod milestones;
mod state;
mod waiting;

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
            return Err(DomainError::Store(StoreError::NotFound(
                active_path.to_string(),
            )));
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

/// Rewrite the `status:` line within the YAML frontmatter region of a
/// project's raw markdown. Operates only between the opening and
/// closing `---` markers so a body line containing `status:` (e.g.
/// inside Current State prose) is unaffected. Preserves the original
/// formatting of every other line — comments, key order, spacing.
///
/// Errors with a missing-section error if the frontmatter doesn't
/// have a `status:` line at all (we'd be rewriting nothing) — that
/// situation should never happen for a project that parsed via
/// `ProjectFrontmatter::try_from`, but we surface it loudly rather
/// than silently emitting a file with no status.
pub(super) fn rewrite_status_in_frontmatter(
    raw: &str,
    new_status: ProjectStatus,
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

    let mut new_yaml = String::with_capacity(yaml.len());
    let mut found = false;
    for line in yaml.split_inclusive('\n') {
        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with("status:") || trimmed_start.starts_with("status :") {
            new_yaml.push_str("status: ");
            new_yaml.push_str(new_status.as_str());
            new_yaml.push('\n');
            found = true;
        } else {
            new_yaml.push_str(line);
        }
    }
    if !found {
        return Err(DomainError::MissingSection("status"));
    }

    let mut result = String::with_capacity(raw.len());
    result.push_str(&raw[..body_after_open]);
    result.push_str(&new_yaml);
    result.push_str(&raw[yaml_end..]);
    Ok(result)
}

#[cfg(test)]
mod tests {
    //! Direct tests for the defensive error branches of
    //! `rewrite_status_in_frontmatter` — branches the public Vault
    //! API can never reach (the surrounding `MarkdownDocument::parse`
    //! and `ProjectFrontmatter::try_from` validate first), but real
    //! code paths nonetheless. Calling the helper directly here keeps
    //! coverage honest.

    use super::*;

    #[test]
    fn rewrite_status_errors_when_frontmatter_missing() {
        let raw = "# Just a heading\nno frontmatter here\n";
        let err = rewrite_status_in_frontmatter(raw, ProjectStatus::Parked).unwrap_err();
        match err {
            DomainError::MissingSection(name) => assert_eq!(name, "frontmatter"),
            other => panic!("expected MissingSection(frontmatter), got {other:?}"),
        }
    }

    #[test]
    fn rewrite_status_errors_when_status_line_missing() {
        // Valid `---`...`---` frontmatter, but no `status:` key inside.
        // Real projects always have one (validated by ProjectFrontmatter::try_from)
        // but the helper itself doesn't trust that and refuses loudly.
        let raw = "---\ntype: project\ncontext: work\n---\n\n# Body\n";
        let err = rewrite_status_in_frontmatter(raw, ProjectStatus::Active).unwrap_err();
        match err {
            DomainError::MissingSection(name) => assert_eq!(name, "status"),
            other => panic!("expected MissingSection(status), got {other:?}"),
        }
    }
}
