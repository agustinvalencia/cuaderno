//! Project queries and operations on [`Vault`].
//!
//! `active_projects` is the foundation for the 5-cap enforcement and
//! orientation summaries (#29). It reads each project's frontmatter to
//! filter by [`ProjectStatus::Active`], rather than peeking at the
//! cached JSON in the index — keeping the typed-frontmatter contract
//! (`ProjectFrontmatter::try_from`) as the single source of truth for
//! "is this project well-formed".
//!
//! `create_project` enforces the configurable max-active cap, scaffolds
//! the project file from a built-in template, and writes the file plus
//! its index row in a single committed transaction.

use chrono::NaiveDate;

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{Context, ProjectFrontmatter, ProjectStatus};
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;

/// Built-in project map template. Custom templates from
/// `.cuaderno/templates/project.md` will override this once the
/// TemplateEngine integration lands; for now `create_project` does
/// straight `{{var}}` substitution against this string.
const PROJECT_TEMPLATE: &str = include_str!("../../templates/project.md");

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

    /// Scaffold a new active project at `projects/<slug>.md`.
    ///
    /// Refuses if the active-project count is already at the
    /// configurable cap (`config.max_active_projects`, default 5),
    /// returning [`DomainError::ProjectCapReached`] with the names of
    /// the projects already active so the caller can suggest one to
    /// park.
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
        if active.len() >= cap {
            return Err(DomainError::ProjectCapReached {
                current: active.len(),
                max: cap,
                active_projects: active
                    .iter()
                    .map(|(path, _)| project_name_from_path(path))
                    .collect(),
            });
        }

        let path = project_path(title)?;
        if self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                path.to_string(),
            )));
        }

        let content = render_project_template(today, title, context, core_question);
        let entry_meta = build_index_entry_for(&path, &content, NoteType::Project.as_str())?;

        let mut tx = self.transaction();
        tx.write_file(path.clone(), content);
        tx.upsert_note(entry_meta);
        tx.commit()?;

        Ok(path)
    }
}

/// Vault-relative path for a new project derived from `title`:
/// `projects/<slug>.md`. The slug is the same one [`capture_to_inbox`]
/// uses, so titles map predictably.
///
/// [`capture_to_inbox`]: super::Vault::capture_to_inbox
fn project_path(title: &str) -> Result<VaultPath, DomainError> {
    let slug = slugify(title);
    Ok(VaultPath::new(format!(
        "{}/{}.md",
        cdno_core::paths::PROJECTS,
        slug
    ))?)
}

/// Pull a human-readable name out of a project path for the
/// cap-error message. The filename stem is what `slugify(title)`
/// produced when the project was created — readable enough to
/// disambiguate which one to park.
fn project_name_from_path(path: &VaultPath) -> String {
    path.as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Render the built-in project template by substituting `{{title}}`,
/// `{{context}}`, `{{created}}`, and `{{core_question}}`. `None` for
/// `core_question` substitutes `null`; `Some(target)` substitutes
/// `"[[target]]"` (quoted to keep YAML from parsing the brackets as a
/// flow sequence).
fn render_project_template(
    today: NaiveDate,
    title: &str,
    context: Context,
    core_question: Option<&str>,
) -> String {
    let core_question_yaml = match core_question {
        Some(target) => format!("\"[[{target}]]\""),
        None => "null".to_owned(),
    };

    PROJECT_TEMPLATE
        .replace("{{title}}", title)
        .replace("{{context}}", context.as_str())
        .replace("{{created}}", &today.format("%Y-%m-%d").to_string())
        .replace("{{core_question}}", &core_question_yaml)
}
