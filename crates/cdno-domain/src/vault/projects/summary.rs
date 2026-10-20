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
use crate::frontmatter::{Context, EnergyLevel, ProjectStatus};

use super::super::Vault;
use super::super::context::LastPause;
use super::waiting::WAITING_ON_PLACEHOLDER;
use super::{CURRENT_STATE_SECTION, NEXT_ACTIONS_SECTION, WAITING_ON_SECTION};

/// Compact view of a project for orientation displays and reviews.
///
/// `state_snippet` is the first two non-blank lines of `## Current
/// State`, joined with `\n`. Empty when the section is missing or
/// has no content. `top_action` is the first open `- [ ]` bullet in
/// `## Next Actions` (closed bullets and blanks skipped); `None`
/// when nothing is open.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProjectSummary {
    pub slug: String,
    pub status: ProjectStatus,
    /// Life context from the frontmatter — drives the UI's colour
    /// dot. Carried here so orientation consumers get it in the same
    /// pass that reads the map (a separate lookup could race a
    /// concurrent park/rename into a silently wrong colour).
    pub context: Context,
    /// The frontmatter `core_question:` exactly as written (a wikilink
    /// such as `"[[questions/research/foo]]"`), or `None` when the map
    /// sets none. Orientation groups the projects that share one by this
    /// field alone (RFC 0006 §8.0).
    pub core_question: Option<String>,
    pub state_snippet: String,
    pub top_action: Option<TopAction>,
    /// Who else is moving the map (RFC 0006 §8.2): the frontmatter
    /// `held_by:` name, trimmed, only when it names someone else under the
    /// vault's configured `[projects] holder_sentinels`; `None` when the
    /// map is the person's own (no holder, or a sentinel such as `me`) or
    /// the value is unreadable (not a string, or an unquoted date). The raw
    /// text stays on [`ProjectFrontmatter::held_by`].
    ///
    /// [`ProjectFrontmatter::held_by`]: crate::frontmatter::ProjectFrontmatter::held_by
    pub held_by: Option<String>,
    /// Whether the map holds one of the `max_active_projects` slots: it is
    /// active and names no one else as its holder
    /// ([`ProjectFrontmatter::counts_toward_cap`]).
    ///
    /// [`ProjectFrontmatter::counts_toward_cap`]: crate::frontmatter::ProjectFrontmatter::counts_toward_cap
    pub counts: bool,
    /// Whether `## Waiting On` says nothing: the section is missing (or
    /// duplicated, so unreadable), empty, or holds only the `(nothing
    /// yet)` placeholder. A held map over an empty Waiting On is §7.2's
    /// "parked map wearing a name".
    pub waiting_on_empty: bool,
    /// The most recent paused action for this project, when one exists
    /// within `paused_lookback_days`. Carried here so orientation
    /// consumers get it in the same pass that reads the log. Not exposed
    /// in CLI JSON output: MCP exposes it, and `cdno orient` leaves it unrendered
    /// (`cdno now` shows the last pause).
    #[serde(skip)]
    pub last_paused: Option<LastPause>,
}

/// One next-action line, with its energy bucket parsed when the
/// `(deep)` / `(medium)` / `(light)` suffix is present. `energy`
/// is `None` for hand-edited bullets without a suffix.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TopAction {
    pub text: String,
    pub energy: Option<EnergyLevel>,
}

impl Vault {
    /// Build a [`ProjectSummary`] for the project identified by
    /// `slug`, regardless of its status. Errors only on
    /// [`Store(NotFound)`] (no file at the active, parked or
    /// `projects/_done/<year>/` location), [`AmbiguousProject`] (the
    /// slug exists at more than one of them) or when the frontmatter
    /// is malformed; missing body sections degrade gracefully.
    ///
    /// [`AmbiguousProject`]: crate::error::DomainError::AmbiguousProject
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

        let waiting_on_empty = doc
            .section(WAITING_ON_SECTION)
            .map(waiting_on_says_nothing)
            .unwrap_or(true);
        let counts = project.counts_toward_cap(&self.holder_sentinels);
        let held_by = project.holder_name(&self.holder_sentinels);

        Ok(ProjectSummary {
            slug: slug.to_owned(),
            status: project.status,
            context: project.context,
            core_question: project.core_question,
            state_snippet,
            top_action,
            held_by,
            counts,
            waiting_on_empty,
            last_paused: None,
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

/// Whether a `## Waiting On` body is empty or only the `(nothing yet)`
/// placeholder.
fn waiting_on_says_nothing(section: &str) -> bool {
    let body = section.trim();
    body.is_empty() || body == WAITING_ON_PLACEHOLDER
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
