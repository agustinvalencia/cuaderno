//! Project queries and operations on [`Vault`].
//!
//! `active_projects` is the foundation for the cap rule and
//! orientation summaries (#29). It reads each project's frontmatter to
//! filter by [`ProjectStatus::Active`], rather than peeking at the
//! cached JSON in the index — keeping the typed-frontmatter contract
//! (`ProjectFrontmatter::try_from`) as the single source of truth for
//! "is this project well-formed".
//!
//! `create_project` scaffolds the project file from a built-in
//! template and writes it plus its index row in a single committed
//! transaction. It seeds the new project as active when below the
//! configurable cap, or as parked when at the cap — the cap is
//! enforced on activation (#28), not creation, so the user can capture
//! a future project without having to park one first.

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{Context, ProjectFrontmatter, ProjectStatus};
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;

/// The heading whose body holds the project's narrative state.
/// Rewritten by `update_project_state`; the previous body is
/// auto-logged to the daily note before being replaced.
const CURRENT_STATE_SECTION: &str = "Current State";

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

    /// Replace an active project's `Current State` section, auto-logging
    /// the previous state to today's daily note in a single committed
    /// transaction.
    ///
    /// `slug` identifies the project (matching the CLI surface,
    /// `cdno project state <slug> "..."`). Lookup is unambiguous
    /// because slug uniqueness spans `projects/` and
    /// `projects/_parked/`. Resolves errors as:
    /// - file at `projects/_parked/<slug>.md` (parked) or frontmatter
    ///   `status` not `active` → [`DomainError::ProjectNotActive`].
    ///   Folder and frontmatter are checked independently because the
    ///   frontmatter is the source of truth — manual edits could put a
    ///   non-active project under `projects/`.
    /// - file at neither location → [`StoreError::NotFound`].
    ///
    /// When `new_state.trim()` equals the existing trimmed state, the
    /// call is a silent no-op — no log entry, no project rewrite —
    /// because logging "was X, now X" is just noise.
    ///
    /// `at` is taken as a parameter so tests can pin the log timestamp
    /// and the daily-note date; production callers pass
    /// `chrono::Local::now().naive_local()`.
    pub fn update_project_state(
        &self,
        at: NaiveDateTime,
        slug: &str,
        new_state: &str,
    ) -> Result<VaultPath, DomainError> {
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
        let mut doc = MarkdownDocument::parse(raw)?;
        // Defensive frontmatter check: the file lives under projects/
        // but a manual edit could have set status to parked or
        // completed. Trust the frontmatter, not the folder.
        let project = ProjectFrontmatter::try_from(doc.frontmatter().clone())?;
        if project.status != ProjectStatus::Active {
            return Err(DomainError::ProjectNotActive(slug.to_owned()));
        }

        let old_state = doc.section(CURRENT_STATE_SECTION)?.trim().to_owned();
        let new_trimmed = new_state.trim();
        if old_state == new_trimmed {
            return Ok(path);
        }

        // Normalise so the section ends with a blank line — preserves
        // readability between Current State and the next heading even
        // when the caller passes unterminated prose.
        let normalised_section = format!("{new_trimmed}\n\n");
        doc.replace_section(CURRENT_STATE_SECTION, &normalised_section)?;
        let new_content = doc.render().to_owned();
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format_state_change_log_entry(slug, &old_state, new_trimmed);

        let mut tx = self.transaction();
        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(path)
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

/// Build the daily-log entry recording a state change. The entry
/// becomes the body of one bullet under `## Logs`: a header line
/// identifying the project, then indented `was:` / `now:`
/// continuation lines so multiline state bodies survive without
/// breaking the line-oriented log format. Whitespace runs in
/// `old_state` and `new_state` (including newlines) collapse to
/// single spaces so each becomes one log line.
fn format_state_change_log_entry(slug: &str, old_state: &str, new_state: &str) -> String {
    format!(
        "state on [[{slug}]]\n  was: {}\n  now: {}",
        flatten_for_log(old_state),
        flatten_for_log(new_state),
    )
}

fn flatten_for_log(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
