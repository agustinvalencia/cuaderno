//! Project lifecycle: queries and state-folder transitions.
//!
//! These operations decide *where* a project lives in the vault and
//! whether a slot in the active cap is consumed. The body of the
//! project file is unchanged by this module — that's the realm of
//! [`super::state`], [`super::actions`], etc.

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{Context, ProjectFrontmatter, ProjectStatus};
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::index_entry::build_index_entry_for;
use super::super::slug::slugify;
use super::{project_slug_from_path, rewrite_field_in_frontmatter};

/// Built-in project map template. Custom templates from
/// `.cuaderno/templates/project.md` will override this once the
/// TemplateEngine integration lands; for now `create_project` does
/// straight `{{var}}` substitution against this string.
const PROJECT_TEMPLATE: &str = include_str!("../../../templates/project.md");

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

    /// Scaffold a new project from the built-in template.
    ///
    /// Below the configurable cap (`config.max_active_projects`,
    /// default 5) the new project is seeded as `status: active` at
    /// `projects/<slug>.md`. At or above the cap it's seeded as
    /// `status: parked` at `projects/_parked/<slug>.md`, so the user
    /// can still capture a future project without having to park an
    /// existing one first. The cap is enforced on activation (#28),
    /// not creation.
    ///
    /// Errors only on slug collisions: if the slug already exists in
    /// either `projects/` or `projects/_parked/`, returns
    /// [`StoreError::AlreadyExists`].
    ///
    /// `core_question` is the wikilink target (e.g.
    /// `"questions/research/foo"`); `create_project` wraps it in
    /// `[[…]]` for the frontmatter. Pass `None` to write
    /// `core_question: null`.
    ///
    /// `today` is taken as a parameter so tests can pin the `created`
    /// date; production callers pass `chrono::Local::now().date_naive()`.
    pub fn create_project(
        &self,
        today: NaiveDate,
        title: &str,
        context: Context,
        core_question: Option<&str>,
    ) -> Result<VaultPath, DomainError> {
        let active = self.active_projects()?;
        let cap = self.config.vault.max_active_projects as usize;
        let status = if active.len() >= cap {
            ProjectStatus::Parked
        } else {
            ProjectStatus::Active
        };

        let slug = slugify(title);
        let active_path = VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS))?;
        let parked_path =
            VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS_PARKED))?;
        // Check both folders so a parked project can't shadow an
        // active one with the same slug, or vice versa. #28
        // (park/activate) will need the same invariant when moving
        // files between the two locations.
        let active_exists = self.store.exists(&active_path)?;
        let parked_exists = self.store.exists(&parked_path)?;
        if active_exists || parked_exists {
            let collision = if active_exists {
                &active_path
            } else {
                &parked_path
            };
            return Err(DomainError::Store(StoreError::AlreadyExists(
                collision.to_string(),
            )));
        }

        let path = if status == ProjectStatus::Active {
            active_path
        } else {
            parked_path
        };

        let content = render_project_template(today, title, context, status, core_question);
        let entry_meta = build_index_entry_for(&path, &content, NoteType::Project.as_str())?;

        let mut tx = self.transaction();
        tx.write_file(path.clone(), content);
        tx.upsert_note(entry_meta);
        tx.commit()?;

        Ok(path)
    }

    /// Move an active project to `projects/_parked/`, flipping its
    /// frontmatter `status` from `active` to `parked` in the same
    /// committed transaction. The previous active count is freed up
    /// so a new active project can take its slot.
    ///
    /// Errors:
    /// - `ProjectNotActive` — file lives at `projects/_parked/<slug>.md`
    ///   or its frontmatter `status` is anything other than `active`.
    /// - `Store(NotFound)` — slug doesn't resolve to either folder.
    /// - `Store(AlreadyExists)` — `projects/_parked/<slug>.md` is
    ///   already occupied (defensive guard against drift; under the
    ///   slug-uniqueness invariant from #24 this can't normally happen
    ///   but a manual edit or a rogue write could).
    pub fn park_project(&self, at: NaiveDateTime, slug: &str) -> Result<VaultPath, DomainError> {
        let (active_path, _doc) = self.resolve_active_project(slug)?;
        let parked_path =
            VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS_PARKED))?;
        if self.store.exists(&parked_path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                parked_path.to_string(),
            )));
        }

        let raw = self.store.read_file(&active_path)?;
        let new_content =
            rewrite_field_in_frontmatter(&raw, "status", ProjectStatus::Parked.as_str())?;
        let entry_meta =
            build_index_entry_for(&parked_path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format!("project [[{slug}]] parked");

        let mut tx = self.transaction();
        tx.write_file(parked_path.clone(), new_content);
        tx.delete_file(active_path.clone());
        tx.upsert_note(entry_meta);
        tx.remove_note(active_path);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(parked_path)
    }

    /// Move a parked project back to `projects/`, flipping its
    /// frontmatter `status` from `parked` to `active`. Enforces the
    /// active-project cap: if activating would exceed
    /// `config.max_active_projects` (default 5), returns
    /// [`DomainError::ProjectCapReached`] with the slugs of the
    /// projects already active so the caller can suggest one to park
    /// first.
    ///
    /// Errors:
    /// - `ProjectCapReached` — at or above the cap.
    /// - `ProjectNotParked` — file lives at `projects/<slug>.md` (the
    ///   active folder) or its frontmatter `status` is anything other
    ///   than `parked`.
    /// - `Store(NotFound)` — slug doesn't resolve to either folder.
    /// - `Store(AlreadyExists)` — `projects/<slug>.md` is already
    ///   occupied (defensive guard against drift).
    pub fn activate_project(
        &self,
        at: NaiveDateTime,
        slug: &str,
    ) -> Result<VaultPath, DomainError> {
        // Cap check first so a "you can't activate, you're at cap"
        // error fires before any file resolution work — clearer for
        // the user when they're at cap and trying to bring something
        // back from the parked drawer.
        let active = self.active_projects()?;
        let cap = self.config.vault.max_active_projects as usize;
        if active.len() >= cap {
            return Err(DomainError::ProjectCapReached {
                current: active.len(),
                max: cap,
                active_projects: active
                    .iter()
                    .map(|(p, _)| project_slug_from_path(p))
                    .collect(),
            });
        }

        let active_path = VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS))?;
        let parked_path =
            VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS_PARKED))?;

        let parked_exists = self.store.exists(&parked_path)?;
        if !parked_exists {
            // Distinguish "file lives at active path" (wrong state)
            // from "no such project" — Store(NotFound) versus
            // ProjectNotParked.
            if self.store.exists(&active_path)? {
                return Err(DomainError::ProjectNotParked(slug.to_owned()));
            }
            return Err(DomainError::Store(StoreError::NotFound(
                parked_path.to_string(),
            )));
        }
        if self.store.exists(&active_path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                active_path.to_string(),
            )));
        }

        let raw = self.store.read_file(&parked_path)?;
        // Defensive: the file is at projects/_parked/ but a manual
        // edit could have set status to active or completed. Trust
        // the frontmatter, refuse if it's not parked.
        let (fm, _body) = Frontmatter::parse(&raw)?;
        let project = ProjectFrontmatter::try_from(fm)?;
        if project.status != ProjectStatus::Parked {
            return Err(DomainError::ProjectNotParked(slug.to_owned()));
        }

        let new_content =
            rewrite_field_in_frontmatter(&raw, "status", ProjectStatus::Active.as_str())?;
        let entry_meta =
            build_index_entry_for(&active_path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format!("project [[{slug}]] activated");

        let mut tx = self.transaction();
        tx.write_file(active_path.clone(), new_content);
        tx.delete_file(parked_path.clone());
        tx.upsert_note(entry_meta);
        tx.remove_note(parked_path);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(active_path)
    }
}

/// Render the built-in project template by substituting `{{title}}`,
/// `{{context}}`, `{{status}}`, `{{created}}`, and `{{core_question}}`.
/// `None` for `core_question` substitutes `null`; `Some(target)`
/// substitutes `"[[target]]"` (quoted to keep YAML from parsing the
/// brackets as a flow sequence).
fn render_project_template(
    today: NaiveDate,
    title: &str,
    context: Context,
    status: ProjectStatus,
    core_question: Option<&str>,
) -> String {
    let core_question_yaml = match core_question {
        Some(target) => format!("\"[[{target}]]\""),
        None => "null".to_owned(),
    };

    PROJECT_TEMPLATE
        .replace("{{title}}", title)
        .replace("{{context}}", context.as_str())
        .replace("{{status}}", status.as_str())
        .replace("{{created}}", &today.format("%Y-%m-%d").to_string())
        .replace("{{core_question}}", &core_question_yaml)
}
