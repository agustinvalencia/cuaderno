//! `project_summary`: read-only per-project view used by the daily
//! orientation flow (#34) and review surfaces. Tolerates drifted
//! sections — a project missing `## Current State` or
//! `## Next Actions` returns an empty snippet / no top action rather
//! than erroring, so a single broken project can't break the orient
//! command.
//!
//! Operates on projects of any status (active, parked, completed).
//! Filtering by status is the caller's job — orientation wants
//! actives, weekly review wants parked too, strategic planning may
//! span everything.

use crate::error::DomainError;
use crate::frontmatter::{EnergyLevel, ProjectStatus};

use super::super::Vault;
use super::{CURRENT_STATE_SECTION, NEXT_ACTIONS_SECTION};

/// Compact view of a project for orientation displays and reviews.
///
/// `state_snippet` is the first two non-blank lines of `## Current
/// State`, joined with `\n`. Empty when the section is missing or
/// has no content. `top_action` is the first open `- [ ]` bullet in
/// `## Next Actions` (closed bullets and blanks skipped); `None`
/// when nothing is open.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
pub struct ProjectSummary {
    pub slug: String,
    pub status: ProjectStatus,
    pub state_snippet: String,
    pub top_action: Option<TopAction>,
}

/// One next-action line, with its energy bucket parsed when the
/// `(deep)` / `(medium)` / `(light)` suffix is present. `energy`
/// is `None` for hand-edited bullets without a suffix.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
pub struct TopAction {
    pub text: String,
    pub energy: Option<EnergyLevel>,
}

impl Vault {
    /// Build a [`ProjectSummary`] for the project identified by
    /// `slug`, regardless of its status. Errors only on
    /// [`Store(NotFound)`] (no file at either active or parked
    /// path) or when the frontmatter is malformed; missing body
    /// sections degrade gracefully.
    ///
    /// [`Store(NotFound)`]: cdno_core::error::StoreError::NotFound
    pub fn project_summary(&self, slug: &str) -> Result<ProjectSummary, DomainError> {
        let (_path, doc, project) = self.resolve_any_project(slug)?;

        // Tolerate drift: a section being absent (or ambiguous) just
        // returns an empty snippet / no top action, rather than
        // erroring. The summary surface is read-only and called by
        // orientation flows that shouldn't break on one drifted
        // project.
        let state_snippet = doc
            .section(CURRENT_STATE_SECTION)
            .map(extract_state_snippet)
            .unwrap_or_default();

        let top_action = doc
            .section(NEXT_ACTIONS_SECTION)
            .ok()
            .and_then(find_top_open_action);

        Ok(ProjectSummary {
            slug: slug.to_owned(),
            status: project.status,
            state_snippet,
            top_action,
        })
    }
}

/// Take the first two non-blank lines of `section` and join them
/// with a single newline. Predictable across single-paragraph and
/// short-multi-paragraph states; empty for sections that have only
/// whitespace.
fn extract_state_snippet(section: &str) -> String {
    section
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Walk the lines of `## Next Actions` and return the first open
/// `- [ ] <text>` bullet, parsing the `(deep)` / `(medium)` /
/// `(light)` suffix into a typed [`EnergyLevel`] when present.
fn find_top_open_action(section: &str) -> Option<TopAction> {
    section.lines().find_map(parse_top_action)
}

fn parse_top_action(line: &str) -> Option<TopAction> {
    let after_box = line.trim_start().strip_prefix("- [ ] ")?.trim();
    for (suffix, energy) in [
        (" (deep)", EnergyLevel::Deep),
        (" (medium)", EnergyLevel::Medium),
        (" (light)", EnergyLevel::Light),
    ] {
        if let Some(stripped) = after_box.strip_suffix(suffix) {
            return Some(TopAction {
                text: stripped.trim_end().to_owned(),
                energy: Some(energy),
            });
        }
    }
    Some(TopAction {
        text: after_box.to_owned(),
        energy: None,
    })
}
