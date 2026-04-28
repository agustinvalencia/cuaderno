//! Project queries on [`Vault`].
//!
//! `active_projects` is the foundation for the 5-cap enforcement (#24)
//! and orientation summaries (#29). It reads each project's frontmatter
//! to filter by [`ProjectStatus::Active`], rather than peeking at the
//! cached JSON in the index — keeping the typed-frontmatter contract
//! (`ProjectFrontmatter::try_from`) as the single source of truth for
//! "is this project well-formed".

use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{ProjectFrontmatter, ProjectStatus};
use crate::note_type::NoteType;

use super::Vault;

impl Vault {
    /// Return every active project: pairs of `(path, frontmatter)`
    /// for files of type `project` whose `status` is `active`.
    ///
    /// Errors propagate. If a project file's frontmatter fails to
    /// parse, the query fails — silently skipping a malformed project
    /// would let the user write a sixth active project under a broken
    /// file and bypass the cap.
    pub fn active_projects(&self) -> Result<Vec<(VaultPath, ProjectFrontmatter)>, DomainError> {
        let mut out = Vec::new();
        for entry in self.index.list_by_type(NoteType::Project.as_str())? {
            let raw = self.store.read_file(&entry.path)?;
            let (fm, _body) = Frontmatter::parse(&raw)?;
            let project = ProjectFrontmatter::try_from(fm)?;
            if project.status == ProjectStatus::Active {
                out.push((entry.path, project));
            }
        }
        Ok(out)
    }
}
