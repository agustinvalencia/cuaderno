//! `add_milestone` / `complete_milestone`: mutate the
//! `## Milestones` checklist of an active project. Hard milestones
//! emitted here are wire-compatible with
//! `cdno_core::markdown::extract_hard_deadlines`, so the commitments
//! aggregation query (#32) picks them up automatically.

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::index::MilestoneEntry;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::index_entry::build_index_entry_for;
use super::MILESTONES_SECTION;

/// Date marker for a milestone gated by a condition rather than a
/// date. Matches the placeholder `crates/cdno-domain/templates/project.md`
/// already seeds, so an undated milestone written by `add_milestone`
/// and one sitting in a fresh project's template read identically.
///
/// Deliberately not an ISO date. The commitments aggregation reads
/// `index.milestones_between`, whose rows come from
/// `extract_milestones_from_body` via reconciliation; that parser
/// yields `date: None` for a non-date marker, and both index
/// implementations drop a null-dated row from the range query. So an
/// undated milestone stays out of the aggregation with no
/// special-casing. (`extract_hard_deadlines` fills the separate
/// `deadlines` table, which has no reader in this crate — do not
/// reason about the aggregation from it.)
pub const UNDATED_TARGET: &str = "TBD";

impl Vault {
    /// Append a milestone bullet to `## Milestones`, logging the
    /// addition to today's daily note in a single committed
    /// transaction. The section is auto-created if missing.
    ///
    /// Format: `- [ ] <title> — hard: YYYY-MM-DD` when `is_hard` is
    /// true, otherwise `- [ ] <title> — target: YYYY-MM-DD`. Hard
    /// milestones with ISO dates are picked up by the commitments
    /// aggregation query (see `cdno_core::markdown::extract_hard_deadlines`).
    ///
    /// `target_date` is optional. Some milestones are gated by a
    /// condition rather than a date ("all Round-1 replies received"),
    /// and inventing an estimate pollutes the milestone list with
    /// commitments nobody made. `None` renders the [`UNDATED_TARGET`]
    /// marker the project template already seeds (`- [ ] <title> —
    /// target: TBD`), a shape the read path already tolerates:
    /// `extract_milestones_from_body` yields `date: None`, and the
    /// range query behind the commitments aggregation drops a
    /// null-dated row, so an undated milestone stays out of it with no
    /// special-casing (#521). See [`UNDATED_TARGET`] for why that, and
    /// not `extract_hard_deadlines`, is the mechanism.
    ///
    /// `is_hard` with no date is rejected
    /// ([`DomainError::HardMilestoneRequiresDate`]) rather than
    /// quietly downgraded — a hard deadline with no date is not a
    /// thing, and erroring is clearer than guessing which half of the
    /// call the user meant.
    pub fn add_milestone(
        &self,
        at: NaiveDateTime,
        slug: &str,
        title: &str,
        target_date: Option<NaiveDate>,
        is_hard: bool,
    ) -> Result<VaultPath, DomainError> {
        let title = title.trim();
        // Checked before the transaction: nothing is written on a
        // rejected call, and the lock is never taken to fail.
        if is_hard && target_date.is_none() {
            return Err(DomainError::HardMilestoneRequiresDate {
                slug: slug.to_owned(),
                title: title.to_owned(),
            });
        }

        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let date_str = match target_date {
            Some(date) => date.format("%Y-%m-%d").to_string(),
            None => UNDATED_TARGET.to_owned(),
        };
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
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
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

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(path)
    }

    /// Pending (uncompleted) milestones for a project, in source
    /// order — the candidate set for the `cdno project milestone done`
    /// fuzzy picker. Thin filter over [`milestones_for_project`].
    pub fn open_milestones(&self, slug: &str) -> Result<Vec<MilestoneEntry>, DomainError> {
        let all = self.index.milestones_for_project(slug)?;
        Ok(all.into_iter().filter(|m| !m.completed).collect())
    }
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
