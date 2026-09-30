//! The closed-project archive under `projects/_done/<year>/` (RFC 0004):
//! the list the CLI picker and `list_projects` offer, and the window the
//! weekly and monthly reviews ask "what ended" of.

use chrono::NaiveDate;

use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{Context, ProjectFrontmatter, ProjectStatus};
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::commitments::body_title_or_slug;
use super::{is_closed_project_path, project_slug_from_path};

/// One project closed inside a review window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedProjectEntry {
    pub slug: String,
    /// The map's body heading, or the slug when it has none.
    pub title: String,
    pub context: Context,
    /// `Completed` or `Dropped`.
    pub outcome: ProjectStatus,
    /// The map's `closed:` date.
    pub closed_on: NaiveDate,
}

impl Vault {
    /// Every closed project: `(path, frontmatter)` for each map under
    /// `projects/_done/<year>/` whose status is `completed` or `dropped`,
    /// in path order (so by year, then slug).
    ///
    /// A map that does not parse is skipped rather than failing the call.
    /// The archive grows for years and is edited by hand, and one broken
    /// closed map must not blank a picker or a review; the active scans
    /// skip `_done/` unread for the same reason. `cdno lint` reports it.
    pub fn closed_projects(&self) -> Result<Vec<(VaultPath, ProjectFrontmatter)>, DomainError> {
        Ok(self
            .closed_maps()?
            .into_iter()
            .map(|(path, frontmatter, _title)| (path, frontmatter))
            .collect())
    }

    /// The projects closed between `from` and `to` inclusive, by their
    /// `closed:` date, sorted by that date then slug. A closed map with no
    /// `closed:` (hand-made) has no date to place it by and is left out,
    /// as are the maps [`closed_projects`](Self::closed_projects) skips.
    pub fn closed_projects_between(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<ClosedProjectEntry>, DomainError> {
        let mut out: Vec<ClosedProjectEntry> = self
            .closed_maps()?
            .into_iter()
            .filter_map(|(path, frontmatter, title)| {
                let closed_on = frontmatter.closed?;
                (from <= closed_on && closed_on <= to).then(|| ClosedProjectEntry {
                    slug: project_slug_from_path(&path),
                    title,
                    context: frontmatter.context,
                    outcome: frontmatter.status,
                    closed_on,
                })
            })
            .collect();
        out.sort_by(|a, b| {
            a.closed_on
                .cmp(&b.closed_on)
                .then_with(|| a.slug.cmp(&b.slug))
        });
        Ok(out)
    }

    /// The parseable closed maps with their titles, in path order.
    fn closed_maps(&self) -> Result<Vec<(VaultPath, ProjectFrontmatter, String)>, DomainError> {
        let mut out = Vec::new();
        for entry in self.index.list_by_type(NoteType::Project.as_str())? {
            if !is_closed_project_path(&entry.path) {
                continue;
            }
            let raw = self.store.read_file(&entry.path)?;
            let Ok((fm, _body)) = Frontmatter::parse(&raw) else {
                continue;
            };
            let Ok(frontmatter) = ProjectFrontmatter::try_from(fm) else {
                continue;
            };
            if !matches!(
                frontmatter.status,
                ProjectStatus::Completed | ProjectStatus::Dropped
            ) {
                continue;
            }
            let slug = project_slug_from_path(&entry.path);
            let title = body_title_or_slug(&raw, &slug).to_owned();
            out.push((entry.path, frontmatter, title));
        }
        out.sort_by(|a, b| a.0.as_path().cmp(b.0.as_path()));
        Ok(out)
    }
}
