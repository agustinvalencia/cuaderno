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
//! - [`actions`] — `add_action`, `complete_action`, `drop_action` for
//!   the `## Next Actions` section, plus the energy-tag parsing helpers.
//! - [`core_question`] — `set_core_question`, changing the project's
//!   `core_question:` after creation and logging the previous value.
//! - [`milestones`] — `add_milestone`, `complete_milestone` for the
//!   `## Milestones` section. Hard milestones feed the commitments
//!   aggregation query (#32).
//! - [`waiting`] — `add_waiting_on`, `resolve_waiting_on` for the
//!   `## Waiting On` section, with `(nothing yet)` placeholder
//!   round-tripping.
//!
//! This file holds the things every submodule needs: the section-name
//! constants, the shared `resolve_active_project` lookup, the
//! `rewrite_field_in_frontmatter` helper used by park/activate, the
//! `core_question_yaml` renderer shared by create and set, and the
//! slug helpers shared across error paths.

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{ProjectFrontmatter, ProjectStatus};
use crate::note_type::NoteType;

use super::Vault;

pub(in crate::vault) mod actions;
mod closed;
mod closing;
mod core_question;
mod lifecycle;
mod milestones;
mod open_items;
pub(crate) mod state;
mod summary;
mod waiting;

pub use actions::{ActionListEntry, AttachedAction};
pub use closed::ClosedProjectEntry;
pub use closing::ProjectClosureOutcome;
pub use open_items::{
    LinkedCommitment, OpenAction, OpenItems, OpenItemsHash, OpenItemsReport, OpenMilestone,
};
pub use summary::{ProjectSummary, TopAction};

/// The heading whose body holds the project's narrative state.
/// Rewritten by `update_project_state`; the previous body is
/// auto-logged to the daily note before being replaced.
pub(super) const CURRENT_STATE_SECTION: &str = "Current State";

/// The heading whose body holds the project's open action checklist.
/// Mutated by `add_action` (append) and by `complete_action` /
/// `drop_action` (remove).
pub(super) const NEXT_ACTIONS_SECTION: &str = "Next Actions";

/// The heading whose body holds project blockers awaiting external
/// resolution. Mutated by `add_waiting_on` and `resolve_waiting_on`.
pub(super) const WAITING_ON_SECTION: &str = "Waiting On";

/// The heading whose body holds project milestones with their target
/// or hard-deadline dates. Mutated by `add_milestone` and
/// `complete_milestone`. Hard milestones in this section feed the
/// commitments aggregation query (#32).
pub(super) const MILESTONES_SECTION: &str = "Milestones";

/// Where a project map lives on disk and the frontmatter read from it.
/// Returned by [`Vault::locate_project`].
#[derive(Debug, Clone)]
pub struct ProjectLocation {
    pub path: VaultPath,
    pub frontmatter: ProjectFrontmatter,
    /// The text the locator read, so a caller that also needs the body
    /// parses the same bytes the frontmatter came from.
    pub(in crate::vault) raw: String,
}

/// The `ProjectNotActive` error for a located map.
fn not_active(slug: &str, frontmatter: &ProjectFrontmatter) -> DomainError {
    DomainError::ProjectNotActive {
        slug: slug.to_owned(),
        status: frontmatter.status,
        closed: frontmatter.closed,
    }
}

/// Whether a directory name is a 4-digit year, the shape of the
/// `projects/_done/<year>/` folders. Shared by the locator and the
/// not-found hint so they agree on what counts as a closed location.
fn is_year_dir_name(name: &str) -> bool {
    name.len() == 4 && name.bytes().all(|b| b.is_ascii_digit())
}

/// Whether `path` lies anywhere under `projects/_done/`. The
/// active-only scans skip these before reading the file, so a malformed
/// closed map can never fail a live-vault query (RFC 0004 §5.5).
pub(in crate::vault) fn is_under_projects_done(path: &VaultPath) -> bool {
    path.as_path().starts_with(cdno_core::paths::PROJECTS_DONE)
}

/// Whether `path` is a map under `projects/_done/<year>/`.
fn is_closed_project_path(path: &VaultPath) -> bool {
    let p = path.as_path();
    let Some(year_dir) = p.parent() else {
        return false;
    };
    year_dir.parent() == Some(std::path::Path::new(cdno_core::paths::PROJECTS_DONE))
        && year_dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(is_year_dir_name)
}

impl Vault {
    /// Find a project map by slug, from the **store** and never the
    /// index: `projects/<slug>.md`, `projects/_parked/<slug>.md`, then
    /// `projects/_done/<year>/<slug>.md` for each year directory. Where
    /// a verb writes must not depend on a cache that can be stale (an
    /// `IndexStale` commit, or a hand move the server has not yet
    /// reconciled), so this is the one answer to "where is project X
    /// and what state is it in" (RFC 0004 §5.5).
    ///
    /// Errors:
    /// - `Store(NotFound)` — the slug is at none of the locations, with
    ///   the available-projects hint.
    /// - `AmbiguousProject` — the slug is at two or more locations
    ///   (candidates sorted). Distinct from `Store(AlreadyExists)`,
    ///   which means "destination occupied".
    /// - `Store(NotFound)` also for a path-shaped slug (containing `/` or
    ///   `\`), which names no project and must not be probed as a path.
    /// - a parse error when the one file found has malformed frontmatter.
    pub(in crate::vault) fn locate_project(
        &self,
        slug: &str,
    ) -> Result<ProjectLocation, DomainError> {
        if slug.contains(['/', '\\']) {
            return Err(DomainError::Store(StoreError::NotFound(format!(
                "{}/{slug}.md{}",
                cdno_core::paths::PROJECTS,
                self.available_projects_hint()
            ))));
        }
        let active_path = VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS))?;
        let mut candidates = vec![
            active_path.clone(),
            VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS_PARKED))?,
        ];
        for entry in self
            .store
            .list_dir(&VaultPath::new(cdno_core::paths::PROJECTS_DONE)?)?
        {
            let Some(year) = entry
                .as_path()
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| is_year_dir_name(n))
            else {
                continue;
            };
            candidates.push(VaultPath::new(format!(
                "{}/{year}/{slug}.md",
                cdno_core::paths::PROJECTS_DONE
            ))?);
        }

        let mut hits = Vec::new();
        for candidate in candidates {
            if self.store.exists(&candidate)? {
                hits.push(candidate);
            }
        }

        match hits.len() {
            0 => Err(DomainError::Store(StoreError::NotFound(format!(
                "{active_path}{}",
                self.available_projects_hint()
            )))),
            1 => {
                let path = hits.remove(0);
                let raw = self.store.read_file(&path)?;
                let (fm, _body) = Frontmatter::parse(&raw)?;
                let frontmatter = ProjectFrontmatter::try_from(fm)?;
                Ok(ProjectLocation {
                    path,
                    frontmatter,
                    raw,
                })
            }
            _ => {
                hits.sort_by(|a, b| a.as_path().cmp(b.as_path()));
                Err(DomainError::AmbiguousProject {
                    slug: slug.to_owned(),
                    candidates: hits,
                })
            }
        }
    }

    /// Resolve a project slug to its active file plus parsed
    /// markdown, or surface the right error when it isn't active.
    /// Used by every mutation that operates on the project body.
    ///
    /// A project counts as active only when its frontmatter says so
    /// *and* it sits directly at `projects/<slug>.md`; anything else
    /// (parked, closed, or a misfiled map) is `ProjectNotActive`.
    pub(super) fn resolve_active_project(
        &self,
        slug: &str,
    ) -> Result<(VaultPath, MarkdownDocument), DomainError> {
        let location = self.locate_project(slug)?;
        let active_path = VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS))?;
        if location.frontmatter.status != ProjectStatus::Active || location.path != active_path {
            return Err(not_active(slug, &location.frontmatter));
        }

        let doc = MarkdownDocument::parse(location.raw)?;
        Ok((location.path, doc))
    }

    /// Resolve a project slug to a map a closing verb may act on, folder
    /// and frontmatter agreeing: an active map at `projects/<slug>.md`, a
    /// parked one at `projects/_parked/<slug>.md` (RFC 0004 §5.1), or a
    /// closed one under `projects/_done/<year>/`, whose outcome the caller
    /// may switch (§5.7). Anything else, such as a map hand-marked
    /// `completed` under `projects/`, is `ProjectNotActive` carrying its
    /// status: a verb that closed it would log, dated today, an ending it
    /// never saw (Q4).
    pub(super) fn resolve_closable_project(
        &self,
        slug: &str,
    ) -> Result<(VaultPath, MarkdownDocument, ProjectFrontmatter), DomainError> {
        let location = self.locate_project(slug)?;
        let in_place = match location.frontmatter.status {
            ProjectStatus::Active => {
                location.path
                    == VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS))?
            }
            ProjectStatus::Parked => {
                location.path
                    == VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS_PARKED))?
            }
            ProjectStatus::Completed | ProjectStatus::Dropped => {
                is_closed_project_path(&location.path)
            }
        };
        if !in_place {
            return Err(not_active(slug, &location.frontmatter));
        }

        let doc = MarkdownDocument::parse(location.raw)?;
        Ok((location.path, doc, location.frontmatter))
    }

    /// Resolve a project slug to its file plus parsed markdown plus
    /// frontmatter, regardless of status. Use this for read-only
    /// queries (summary, orientation peek-ins) that want to operate
    /// on parked or completed projects too — gatekeeping by status
    /// belongs in the caller.
    ///
    /// Errors only when the slug doesn't resolve to any location
    /// (`Store(NotFound)`), resolves to several
    /// (`AmbiguousProject`), or when the file's frontmatter is
    /// malformed.
    pub(in crate::vault) fn resolve_any_project(
        &self,
        slug: &str,
    ) -> Result<(VaultPath, MarkdownDocument, ProjectFrontmatter), DomainError> {
        let location = self.locate_project(slug)?;
        let doc = MarkdownDocument::parse(location.raw)?;
        Ok((location.path, doc, location.frontmatter))
    }

    /// " — available projects: …" suffix for a project slug not-found,
    /// listing every indexed project (parked and closed ones flagged) so
    /// a caller can self-correct. Shared by the resolvers, state update,
    /// and activate.
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
                } else if is_closed_project_path(path) {
                    // Status is not readable from the path; the index
                    // hint does not parse files.
                    format!("{slug} (closed)")
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
        // Top-level keys only. An indented line belongs to a nested value, and
        // since #481 frontmatter can carry one — a record sequence whose rows
        // have their own `weight:`/`minutes:` keys. Matching those would
        // rewrite a key *inside* a record and re-emit it at column zero,
        // silently moving it out of the record and, when the note also has a
        // top-level key of that name, leaving a duplicate the parser rejects.
        if line.starts_with(&prefix_compact) || line.starts_with(&prefix_spaced) {
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

/// Render a `core_question:` frontmatter value from a **bare** wikilink
/// target: `"[[questions/research/foo]]"` quoted for YAML, or `null`.
///
/// Shared by `create_project` and `set_core_question` so the two cannot
/// disagree about the wrapping — a project whose question was set after
/// creation must be byte-identical to one that carried it from the
/// start, or the no-op check in `set_core_question` and every consumer
/// that parses the path out of the link would see two shapes.
pub(in crate::vault) fn core_question_yaml(target: Option<&str>) -> String {
    match target {
        Some(t) => format!("\"[[{t}]]\""),
        None => "null".to_owned(),
    }
}
