//! `add_waiting_on` / `resolve_waiting_on`: mutate the
//! `## Waiting On` section of an active project. Waiting-on items
//! are informational blockers (no checkbox), not actionable; the
//! `(nothing yet)` placeholder is round-tripped on add and remove.

use chrono::NaiveDateTime;

use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::index_entry::build_index_entry_for;
use super::WAITING_ON_SECTION;

/// Placeholder body for an empty `## Waiting On` section. Treated as
/// equivalent to "no items" so `add_waiting_on` replaces rather than
/// appending below it.
const WAITING_ON_PLACEHOLDER: &str = "(nothing yet)";

impl Vault {
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
