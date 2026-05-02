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
use crate::frontmatter::{Context, EnergyLevel, ProjectFrontmatter, ProjectStatus};
use crate::note_type::NoteType;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;

/// The heading whose body holds the project's narrative state.
/// Rewritten by `update_project_state`; the previous body is
/// auto-logged to the daily note before being replaced.
const CURRENT_STATE_SECTION: &str = "Current State";

/// The heading whose body holds the project's open action checklist.
/// Mutated by `add_action` (append) and `complete_action` (remove).
const NEXT_ACTIONS_SECTION: &str = "Next Actions";

/// The heading whose body holds project blockers awaiting external
/// resolution. Mutated by `add_waiting_on` and `resolve_waiting_on`.
const WAITING_ON_SECTION: &str = "Waiting On";

/// The heading whose body holds project milestones with their target
/// or hard-deadline dates. Mutated by `add_milestone` and
/// `complete_milestone`. Hard milestones in this section feed the
/// commitments aggregation query (#32).
const MILESTONES_SECTION: &str = "Milestones";

/// Placeholder body for an empty `## Waiting On` section. Treated as
/// equivalent to "no items" so `add_waiting_on` replaces rather than
/// appending below it.
const WAITING_ON_PLACEHOLDER: &str = "(nothing yet)";

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

    /// Append a next action to an active project, also recording the
    /// addition in today's daily log so a planning session leaves a
    /// trace.
    ///
    /// The new line takes the form `- [ ] <action> (<energy>)`, placed
    /// at the end of the `## Next Actions` section. Section formatting
    /// is normalised — a single newline separates the new bullet from
    /// the previous content, and the section ends with a blank line so
    /// the next heading stays cleanly separated.
    ///
    /// Errors mirror `update_project_state`: parked → `ProjectNotActive`,
    /// missing → `Store(NotFound)`, missing section → `Manipulation`.
    pub fn add_action(
        &self,
        at: NaiveDateTime,
        slug: &str,
        action: &str,
        energy: EnergyLevel,
    ) -> Result<VaultPath, DomainError> {
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let action_text = action.trim();
        let bullet = format!("- [ ] {action_text} ({})", energy.as_str());

        // Auto-create the section if a drifted project is missing it
        // (migration imports, hand-edited files). The user's intent on
        // "add an action" is unambiguous; refusing would force them to
        // edit the file by hand first.
        doc.ensure_section(NEXT_ACTIONS_SECTION)?;
        let existing = doc.section(NEXT_ACTIONS_SECTION)?.trim_end();
        let new_section = if existing.is_empty() {
            format!("{bullet}\n\n")
        } else {
            format!("{existing}\n{bullet}\n\n")
        };
        doc.replace_section(NEXT_ACTIONS_SECTION, &new_section)?;

        let new_content = doc.render().to_owned();
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format_action_added_log_entry(slug, action_text, energy);

        let mut tx = self.transaction();
        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(path)
    }

    /// Remove an open action from an active project, logging the
    /// completion to today's daily note. Closed `- [x]` lines are
    /// ignored — only `- [ ]` bullets are candidates, because a
    /// closed line was already manually checked and shouldn't be
    /// silently swept away by a substring query.
    ///
    /// `query` is matched case-insensitively as a substring against
    /// each open action's text (the `(<energy>)` suffix is stripped
    /// before matching). Zero matches → `ActionNotFound`. More than
    /// one match → `AmbiguousAction` carrying the candidate texts so
    /// the user can re-query with enough context to disambiguate.
    pub fn complete_action(
        &self,
        at: NaiveDateTime,
        slug: &str,
        query: &str,
    ) -> Result<VaultPath, DomainError> {
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let section = doc.section(NEXT_ACTIONS_SECTION)?;
        let lines: Vec<&str> = section.split('\n').collect();
        let needle = query.trim().to_lowercase();

        let mut matches: Vec<usize> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if let Some(text) = parse_open_action_text(line)
                && strip_energy_suffix(text).to_lowercase().contains(&needle)
            {
                matches.push(i);
            }
        }

        if matches.is_empty() {
            return Err(DomainError::ActionNotFound {
                slug: slug.to_owned(),
                query: query.to_owned(),
            });
        }
        if matches.len() > 1 {
            let candidates = matches
                .iter()
                .map(|&i| parse_open_action_text(lines[i]).unwrap_or("").to_owned())
                .collect();
            return Err(DomainError::AmbiguousAction {
                slug: slug.to_owned(),
                query: query.to_owned(),
                candidates,
            });
        }

        let removed_idx = matches[0];
        let removed_full_text = parse_open_action_text(lines[removed_idx])
            .expect("matched line was previously parseable")
            .to_owned();

        let kept: Vec<&str> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| if i == removed_idx { None } else { Some(*l) })
            .collect();
        let new_section = kept.join("\n");
        doc.replace_section(NEXT_ACTIONS_SECTION, &new_section)?;

        let new_content = doc.render().to_owned();
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format_action_done_log_entry(slug, &removed_full_text);

        let mut tx = self.transaction();
        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(path)
    }

    /// Resolve a project slug to its active file plus parsed
    /// markdown, or surface the right error when it isn't active.
    /// Used by every mutation that operates on the project body.
    fn resolve_active_project(
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

    /// Append a milestone bullet to `## Milestones`, logging the
    /// addition to today's daily note in a single committed
    /// transaction. The section is auto-created if missing.
    ///
    /// Format: `- [ ] <title> — hard: YYYY-MM-DD` when `is_hard` is
    /// true, otherwise `- [ ] <title> — target: YYYY-MM-DD`. Hard
    /// milestones with ISO dates are picked up by the commitments
    /// aggregation query (see `cdno_core::markdown::extract_hard_deadlines`).
    pub fn add_milestone(
        &self,
        at: NaiveDateTime,
        slug: &str,
        title: &str,
        target_date: NaiveDate,
        is_hard: bool,
    ) -> Result<VaultPath, DomainError> {
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let title = title.trim();
        let date_str = target_date.format("%Y-%m-%d").to_string();
        let keyword = if is_hard { "hard" } else { "target" };
        let bullet = format!("- [ ] {title} \u{2014} {keyword}: {date_str}");

        doc.ensure_section(MILESTONES_SECTION)?;
        let existing = doc.section(MILESTONES_SECTION)?.trim_end();
        let new_section = if existing.is_empty() {
            format!("{bullet}\n\n")
        } else {
            format!("{existing}\n{bullet}\n\n")
        };
        doc.replace_section(MILESTONES_SECTION, &new_section)?;

        let new_content = doc.render().to_owned();
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry =
            format!("milestone added to [[{slug}]] \u{2014} {title} ({keyword}: {date_str})");

        let mut tx = self.transaction();
        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(path)
    }

    /// Mark an open milestone as completed in-place: the matched
    /// `- [ ] <title> — <keyword>: <value>` line becomes
    /// `- [x] <title> — YYYY-MM-DD` (today's date), preserving the
    /// surrounding section. The completion is logged to today's
    /// daily note in the same transaction.
    ///
    /// Match strategy mirrors `complete_action`: case-insensitive
    /// substring on the title portion only (the `— <keyword>: <date>`
    /// suffix is stripped before comparison). Closed `- [x]` bullets
    /// are skipped — they were already manually completed.
    pub fn complete_milestone(
        &self,
        at: NaiveDateTime,
        slug: &str,
        query: &str,
    ) -> Result<VaultPath, DomainError> {
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let section = doc.section(MILESTONES_SECTION)?;
        let lines: Vec<&str> = section.split('\n').collect();
        let needle = query.trim().to_lowercase();

        let mut matches: Vec<usize> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if let Some(text) = parse_open_milestone_title(line)
                && text.to_lowercase().contains(&needle)
            {
                matches.push(i);
            }
        }

        if matches.is_empty() {
            return Err(DomainError::MilestoneNotFound {
                slug: slug.to_owned(),
                query: query.to_owned(),
            });
        }
        if matches.len() > 1 {
            let candidates = matches
                .iter()
                .map(|&i| {
                    parse_open_milestone_title(lines[i])
                        .unwrap_or("")
                        .to_owned()
                })
                .collect();
            return Err(DomainError::AmbiguousMilestone {
                slug: slug.to_owned(),
                query: query.to_owned(),
                candidates,
            });
        }

        let matched_idx = matches[0];
        let title = parse_open_milestone_title(lines[matched_idx])
            .expect("matched line was previously parseable")
            .to_owned();
        let completion_date = at.date().format("%Y-%m-%d").to_string();
        let new_line = format!("- [x] {title} \u{2014} {completion_date}");

        let mut new_lines: Vec<String> = lines.iter().map(|s| (*s).to_owned()).collect();
        new_lines[matched_idx] = new_line;
        let new_section = new_lines.join("\n");
        doc.replace_section(MILESTONES_SECTION, &new_section)?;

        let new_content = doc.render().to_owned();
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format!("milestone done on [[{slug}]] \u{2014} {title}");

        let mut tx = self.transaction();
        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(path)
    }

    /// Append a waiting-on item to `## Waiting On`, logging the
    /// addition to today's daily note. The section is auto-created
    /// if missing. The `(nothing yet)` placeholder is treated as an
    /// empty section — the new bullet replaces it instead of stacking
    /// below it.
    ///
    /// Waiting-on lines have no checkbox: they're informational
    /// blockers (`- Compute allocation — requested 500 GPU-hours`),
    /// not actionable items.
    pub fn add_waiting_on(
        &self,
        at: NaiveDateTime,
        slug: &str,
        description: &str,
    ) -> Result<VaultPath, DomainError> {
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let description = description.trim();
        let bullet = format!("- {description}");

        doc.ensure_section(WAITING_ON_SECTION)?;
        let existing = doc.section(WAITING_ON_SECTION)?.trim();
        let is_placeholder = existing == WAITING_ON_PLACEHOLDER;
        let new_section = if existing.is_empty() || is_placeholder {
            format!("{bullet}\n\n")
        } else {
            format!("{existing}\n{bullet}\n\n")
        };
        doc.replace_section(WAITING_ON_SECTION, &new_section)?;

        let new_content = doc.render().to_owned();
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format!("waiting added on [[{slug}]] \u{2014} {description}");

        let mut tx = self.transaction();
        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(path)
    }

    /// Remove a waiting-on item from `## Waiting On`, logging the
    /// resolution to today's daily note.
    ///
    /// Match strategy mirrors `complete_action`: case-insensitive
    /// substring on the bullet text. If removing the last item leaves
    /// the section empty, the `(nothing yet)` placeholder is restored
    /// so the section reads consistently.
    pub fn resolve_waiting_on(
        &self,
        at: NaiveDateTime,
        slug: &str,
        query: &str,
    ) -> Result<VaultPath, DomainError> {
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let section = doc.section(WAITING_ON_SECTION)?;
        let lines: Vec<&str> = section.split('\n').collect();
        let needle = query.trim().to_lowercase();

        let mut matches: Vec<usize> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if let Some(text) = parse_waiting_on_text(line)
                && text.to_lowercase().contains(&needle)
            {
                matches.push(i);
            }
        }

        if matches.is_empty() {
            return Err(DomainError::WaitingOnNotFound {
                slug: slug.to_owned(),
                query: query.to_owned(),
            });
        }
        if matches.len() > 1 {
            let candidates = matches
                .iter()
                .map(|&i| parse_waiting_on_text(lines[i]).unwrap_or("").to_owned())
                .collect();
            return Err(DomainError::AmbiguousWaitingOn {
                slug: slug.to_owned(),
                query: query.to_owned(),
                candidates,
            });
        }

        let removed_idx = matches[0];
        let removed_text = parse_waiting_on_text(lines[removed_idx])
            .expect("matched line was previously parseable")
            .to_owned();
        let kept: Vec<&str> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| if i == removed_idx { None } else { Some(*l) })
            .collect();
        // If the removal leaves no bullets behind, restore the
        // `(nothing yet)` placeholder so the section reads cleanly.
        let new_section = if kept
            .iter()
            .all(|l| parse_waiting_on_text(l).is_none() && l.trim().is_empty())
        {
            format!("{WAITING_ON_PLACEHOLDER}\n\n")
        } else {
            kept.join("\n")
        };
        doc.replace_section(WAITING_ON_SECTION, &new_section)?;

        let new_content = doc.render().to_owned();
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format!("waiting resolved on [[{slug}]] \u{2014} {removed_text}");

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

/// Build the daily-log entry recording an action addition.
fn format_action_added_log_entry(slug: &str, action: &str, energy: EnergyLevel) -> String {
    format!(
        "action added to [[{slug}]] — {action} ({})",
        energy.as_str()
    )
}

/// Build the daily-log entry recording an action completion.
/// `action_text` is the raw text from the project line, including
/// any `(<energy>)` suffix, so the historical record preserves what
/// energy bucket the action sat in.
fn format_action_done_log_entry(slug: &str, action_text: &str) -> String {
    format!("action done on [[{slug}]] — {action_text}")
}

/// If `line` is an open action bullet (`- [ ] <text>`), return the
/// `<text>` verbatim — including any trailing `(<energy>)` suffix.
/// Closed bullets (`- [x]`), blanks, and non-bullet content return
/// `None`. Substring matching strips the suffix separately via
/// [`strip_energy_suffix`]; the verbatim form is what gets logged
/// on completion so the daily log preserves the energy tag.
fn parse_open_action_text(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("- [ ] ").map(str::trim)
}

/// Trim a trailing `(deep)`, `(medium)`, or `(light)` suffix —
/// matching is case-sensitive because `add_action` always emits
/// lowercase.
fn strip_energy_suffix(text: &str) -> &str {
    for suffix in [" (deep)", " (medium)", " (light)"] {
        if let Some(stripped) = text.strip_suffix(suffix) {
            return stripped;
        }
    }
    text
}

/// If `line` is an open milestone bullet (`- [ ] <title> — <keyword>:
/// <value>`), return the `<title>` portion with the trailing
/// keyword/value section stripped. Closed bullets, blanks, and
/// non-bullet content return `None`.
///
/// Both em-dash (`\u{2014}`) and ASCII hyphen-minus separators are
/// recognised — same forgiveness as
/// [`cdno_core::markdown::extract_hard_deadlines`].
fn parse_open_milestone_title(line: &str) -> Option<&str> {
    let after_box = line.trim_start().strip_prefix("- [ ] ")?;
    Some(strip_milestone_target_suffix(after_box.trim()))
}

fn strip_milestone_target_suffix(text: &str) -> &str {
    for separator in [
        " \u{2014} hard:",
        " \u{2014} target:",
        " - hard:",
        " - target:",
    ] {
        if let Some(idx) = text.rfind(separator) {
            return text[..idx].trim_end();
        }
    }
    text
}

/// If `line` is a waiting-on bullet (`- <description>`), return the
/// description trimmed. Blank lines, the `(nothing yet)` placeholder,
/// and non-bullet content return `None`.
fn parse_waiting_on_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- [") {
        // Checkbox bullets aren't waiting-on items; ignore so the
        // placeholder logic and substring match don't accidentally
        // touch a stray task.
        return None;
    }
    let body = trimmed.strip_prefix("- ")?.trim();
    if body.is_empty() {
        return None;
    }
    Some(body)
}
