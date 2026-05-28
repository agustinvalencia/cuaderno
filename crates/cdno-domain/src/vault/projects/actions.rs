//! `add_action` / `complete_action`: mutate the `## Next Actions`
//! checklist of an active project, with daily-note logging on both
//! the addition (planning trace) and the completion.

use chrono::NaiveDateTime;

use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::EnergyLevel;
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::index_entry::build_index_entry_for;
use super::NEXT_ACTIONS_SECTION;

impl Vault {
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
        // If the completed bullet wikilinks an action note, archive the
        // note in the same transaction — its move to `_done/<year>/`
        // and the bullet removal are then atomic. A plain bullet skips
        // this and behaves exactly as before. Still one daily-log line,
        // not two.
        if let Some(action_slug) = parse_attached_action_slug(&removed_full_text) {
            self.stage_action_archival(at, action_slug, &mut tx)?;
        }
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(path)
    }

    /// Promote an open bullet to a manifest action note (design §5.11).
    /// Finds the bullet via case-insensitive substring (same matching
    /// as `complete_action`), spins a new action note inheriting the
    /// bullet's title and energy, and rewrites the bullet to wikilink
    /// the note — all atomic, in a single transaction.
    ///
    /// Errors:
    /// - `ActionAlreadyPromoted` — the matched bullet already
    ///   wikilinks an action note.
    /// - `BulletMissingEnergy` — the bullet has no
    ///   `(deep|medium|light)` suffix to inherit; surfaced rather than
    ///   guessed so an authoring bug is visible.
    /// - `ActionNotFound` / `AmbiguousAction` — same disambiguation as
    ///   `complete_action`.
    /// - parked project → `ProjectNotActive`; missing project →
    ///   `Store(NotFound)`; slug collision on the new note →
    ///   `Store(AlreadyExists)`.
    pub fn promote_action(
        &self,
        at: NaiveDateTime,
        slug: &str,
        query: &str,
    ) -> Result<VaultPath, DomainError> {
        let (project_path, mut doc) = self.resolve_active_project(slug)?;

        let section = doc.section(NEXT_ACTIONS_SECTION)?;
        let lines: Vec<&str> = section.split('\n').collect();
        let needle = query.trim().to_lowercase();

        // Find / disambiguate using the same rules as complete_action.
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

        let bullet_idx = matches[0];
        let bullet_text = parse_open_action_text(lines[bullet_idx])
            .expect("matched line was previously parseable")
            .to_owned();

        if parse_attached_action_slug(&bullet_text).is_some() {
            return Err(DomainError::ActionAlreadyPromoted {
                slug: slug.to_owned(),
                line: bullet_text,
            });
        }

        let energy =
            parse_bullet_energy(&bullet_text).ok_or_else(|| DomainError::BulletMissingEnergy {
                slug: slug.to_owned(),
                line: bullet_text.clone(),
            })?;
        let title = strip_energy_suffix(&bullet_text).trim().to_owned();

        // Spin the note (uncommitted tx) and continue staging on it so
        // note write + bullet rewrite + daily log commit together.
        let (note_path, mut tx) = self.create_action_note(at, slug, &title, energy, None, None)?;
        let action_slug = note_path
            .as_path()
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let new_bullet = format!(
            "- [ ] [[{}/{action_slug}]] ({})",
            cdno_core::paths::ACTIONS,
            energy.as_str()
        );
        let mut new_lines: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
        new_lines[bullet_idx] = new_bullet;
        let new_section = new_lines.join("\n");
        doc.replace_section(NEXT_ACTIONS_SECTION, &new_section)?;

        let new_content = doc.render().to_owned();
        let project_entry =
            build_index_entry_for(&project_path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format!(
            "action promoted on [[{slug}]] — \"{title}\" -> [[{}/{action_slug}]]",
            cdno_core::paths::ACTIONS,
        );

        tx.write_file(project_path, new_content);
        tx.upsert_note(project_entry);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(note_path)
    }
}

/// If `text` is a wikilink to an action note — `[[actions/<slug>]]`,
/// optionally followed by a `(<energy>)` suffix — return the slug.
/// Plain action bullets, links carrying a `|label`, and anything that
/// isn't exactly an `actions/` wikilink return `None`, so completion
/// falls through to the unchanged plain-bullet path.
fn parse_attached_action_slug(text: &str) -> Option<&str> {
    let inner = strip_energy_suffix(text.trim())
        .trim()
        .strip_prefix("[[")?
        .strip_suffix("]]")?;
    let slug = inner.strip_prefix("actions/")?;
    if slug.is_empty() || slug.contains(['[', ']', '|']) {
        None
    } else {
        Some(slug)
    }
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

/// Recover the [`EnergyLevel`] from a bullet's trailing
/// `(deep|medium|light)` suffix; `None` for any other shape. Callers
/// decide whether the absence is an error (promote needs it) or
/// silently OK (completion just logs the raw text).
fn parse_bullet_energy(text: &str) -> Option<EnergyLevel> {
    if text.ends_with(" (deep)") {
        Some(EnergyLevel::Deep)
    } else if text.ends_with(" (medium)") {
        Some(EnergyLevel::Medium)
    } else if text.ends_with(" (light)") {
        Some(EnergyLevel::Light)
    } else {
        None
    }
}
