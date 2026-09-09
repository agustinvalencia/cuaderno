//! `add_milestone` / `complete_milestone` / `drop_milestone`: mutate
//! the `## Milestones` checklist of an active project. Hard milestones
//! emitted here are wire-compatible with
//! `cdno_core::markdown::extract_hard_deadlines`, so the commitments
//! aggregation query (#32) picks them up automatically.

use chrono::{NaiveDate, NaiveDateTime};

use cdno_core::index::{DeadlineEntry, MilestoneEntry};
use cdno_core::markdown::{extract_hard_deadlines, extract_milestones_from_body};
use cdno_core::path::VaultPath;
use cdno_core::transaction::VaultTransaction;

use crate::error::DomainError;
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::index_entry::build_index_entry_for;
use super::MILESTONES_SECTION;
use super::actions::{LOG_REASON_KEY, flatten_reason};

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

/// The exact `## Milestones` line `crates/cdno-domain/templates/project.md`
/// seeds a new project with.
///
/// It is scaffolding that looks like data: it renders as a real open
/// milestone, `complete_milestone` will happily tick it, and until #522
/// the only way to be rid of it was to hand-edit the file — the one
/// thing the design forbids, because it desyncs the index. So
/// [`Vault::add_milestone`] replaces it the first time a real milestone
/// is added.
///
/// It does still reach the user: on a project nobody has added a
/// milestone to, the placeholder is on disk from creation and is what
/// `open_milestones` offers the `done` and `drop` pickers. What the
/// replacement buys is that no project ends up with a fake milestone
/// sitting *above* a real one. [`Vault::drop_milestone`] is the way out
/// for the rest.
///
/// Compared after `trim`, and only when it is the whole section — not
/// byte-for-byte. The section arrives already `trim_end`-ed, so trailing
/// whitespace and a CRLF ending never distinguished anything; what the
/// `trim` adds is that an indented placeholder still counts as
/// untouched. An ASCII-hyphen spelling (`First milestone - target: TBD`)
/// is a different line and is kept. A project
/// whose placeholder was genuinely edited, or which has real milestones
/// beside it, is a project whose author meant something by that line;
/// guessing at intent there would delete work.
const TEMPLATE_PLACEHOLDER: &str = "- [ ] First milestone \u{2014} target: TBD";

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
        // The template placeholder is replaced rather than
        // appended to (#522). See [`TEMPLATE_PLACEHOLDER`]: it is
        // scaffolding, and leaving it above the user's first real
        // milestone means every fresh project accumulates a fake one.
        let new_section = if existing.is_empty() || existing.trim() == TEMPLATE_PLACEHOLDER {
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
        stage_milestone_index_rows(&path, &new_section, &mut tx);
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
        let (matched_idx, title) = resolve_open_milestone(&lines, slug, query)?;
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
        stage_milestone_index_rows(&path, &new_section, &mut tx);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        tx.commit()?;

        Ok(path)
    }

    /// Remove an open milestone from `## Milestones`, logging the drop
    /// to today's daily note in a single committed transaction.
    ///
    /// The counterpart to [`Vault::complete_milestone`], and the reason
    /// #522 exists: a milestone that was superseded, mis-typed or never
    /// real could previously only be got rid of by ticking it done —
    /// asserting work nobody did — or by hand-editing the file, which
    /// desyncs the index. Neither is a supported way to say "this is not
    /// happening".
    ///
    /// Match strategy is [`Vault::complete_milestone`]'s exactly, via the
    /// same resolver: case-insensitive substring on the title portion,
    /// with the `— <keyword>: <date>` suffix stripped before comparison.
    /// Ambiguity is an error carrying the candidates rather than a guess.
    ///
    /// A bullet's indented continuation lines go with it: sub-bullets
    /// describing a milestone that is not happening must not be
    /// re-parented to the milestone above.
    ///
    /// **Only open `- [ ]` bullets match.** A completed milestone is a
    /// record of something that happened, and dropping is for things that
    /// will not; removing a `- [x]` line would erase history rather than
    /// correct a plan. `complete_milestone` skips closed bullets for the
    /// same reason.
    ///
    /// `reason` is optional and free text. Present, it rides an indented
    /// continuation line under the log entry, the shape `drop_action`
    /// established — a correction ("typo") and a decision ("the funder
    /// withdrew") are different facts about the project, and only the
    /// second suggests revisiting the plan. Absent, the entry is bare:
    /// #564 settles that a correction is simply a drop with no reason,
    /// rather than a second verb.
    ///
    /// Errors mirror `complete_milestone`: parked → `ProjectNotActive`,
    /// missing project → `Store(NotFound)`, missing section →
    /// `Manipulation`, no match → `MilestoneNotFound`, several matches →
    /// `AmbiguousMilestone`.
    pub fn drop_milestone(
        &self,
        at: NaiveDateTime,
        slug: &str,
        query: &str,
        reason: Option<&str>,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)
        let (path, mut doc) = self.resolve_active_project(slug)?;

        let section = doc.section(MILESTONES_SECTION)?;
        let lines: Vec<&str> = section.split('\n').collect();
        let (matched_idx, title) = resolve_open_milestone(&lines, slug, query)?;

        // Take the bullet's indented continuation lines with it. Removing
        // the bullet alone would re-parent its sub-bullets to whichever
        // milestone happens to precede it — notes about a milestone that
        // is not happening, silently attached to one that is.
        // `complete_milestone` has no equivalent problem: it replaces the
        // line in place, so the children stay under their own bullet.
        let mut drop_end = matched_idx + 1;
        while drop_end < lines.len() {
            let line = lines[drop_end];
            if line.trim().is_empty() || !line.starts_with(char::is_whitespace) {
                break;
            }
            drop_end += 1;
        }
        let new_lines: Vec<String> = lines
            .iter()
            .enumerate()
            .filter(|(i, _)| !(matched_idx..drop_end).contains(i))
            .map(|(_, s)| (*s).to_owned())
            .collect();
        let new_section = new_lines.join("\n");
        doc.replace_section(MILESTONES_SECTION, &new_section)?;

        let new_content = doc.render().to_owned();
        let entry_meta = build_index_entry_for(&path, &new_content, NoteType::Project.as_str())?;

        let log_entry = format_milestone_dropped_log_entry(slug, &title, reason);

        tx.write_file(path.clone(), new_content);
        tx.upsert_note(entry_meta);
        stage_milestone_index_rows(&path, &new_section, &mut tx);
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

/// Stage the index rows a rewritten `## Milestones` section implies.
///
/// Reconciliation writes `milestones` and `deadlines` for a project
/// whenever it re-reads the file (`cdno_core::reconcile`), but the
/// milestone verbs did not, so every add, completion and drop left the
/// index describing the section as it was before. The rows do not heal
/// on their own: the same transaction's `upsert_note` records the *new*
/// content hash, so reconciliation's fast path classifies the file as
/// unchanged for ever after and only an explicit `cdno reindex` repairs
/// it.
///
/// That is not cosmetic. The commitments aggregation reads
/// `milestones_between`, and `open_milestones` — the candidate list
/// behind the `done` and `drop` pickers — reads `milestones_for_project`.
/// A dropped milestone left in the table keeps counting as a live
/// commitment and keeps being offered for completion, which is the exact
/// opposite of what the user just said about it.
///
/// `replace_*` is a whole-path replacement, so staging it from the new
/// section also repairs whatever the previous verbs left behind.
fn stage_milestone_index_rows(path: &VaultPath, new_section: &str, tx: &mut VaultTransaction) {
    tx.replace_milestones(path.clone(), extract_milestones_from_body(new_section));
    tx.replace_deadlines(
        path.clone(),
        extract_hard_deadlines(new_section)
            .into_iter()
            .map(|(title, due_date)| DeadlineEntry {
                source: "project_milestone".to_owned(),
                title,
                due_date,
                is_hard: true,
                // Mirrors `cdno_core::reconcile`: context derivation needs
                // the frontmatter field, the column accepts NULL, and the
                // commitments query filters client-side.
                context: None,
            })
            .collect(),
    );
}

/// Resolve a substring `query` to exactly one open milestone within a
/// project's `## Milestones` lines, returning its index and stripped
/// title.
///
/// Shared by [`Vault::complete_milestone`] and [`Vault::drop_milestone`]
/// so the two cannot drift: a query that names one milestone to complete
/// must name the same one to drop, and a query that is ambiguous for one
/// must be ambiguous for the other. Two copies of this loop would be two
/// chances to disagree about what a milestone is called.
fn resolve_open_milestone(
    lines: &[&str],
    slug: &str,
    query: &str,
) -> Result<(usize, String), DomainError> {
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

    let idx = matches[0];
    let title = parse_open_milestone_title(lines[idx])
        .expect("matched line was previously parseable")
        .to_owned();
    Ok((idx, title))
}

/// Build the daily-log entry recording a milestone being dropped.
///
/// `milestone dropped on [[slug]] — <title>`, deliberately parallel to
/// `action dropped on` (#564 settles the abandonment shape), and
/// deliberately distinct from `milestone done on` so a later reader can
/// tell a plan that changed from a plan that was met.
///
/// An optional reason rides an indented continuation line, whitespace
/// flattened so one drop stays one entry. Unlike the action case the
/// shape is not load-bearing for any reader — nothing parses milestone
/// log entries back — but writing it differently here would leave the
/// vault with two spellings of the same idea.
fn format_milestone_dropped_log_entry(slug: &str, title: &str, reason: Option<&str>) -> String {
    let base = format!("milestone dropped on [[{slug}]] \u{2014} {title}");
    match reason.map(flatten_reason).filter(|r| !r.is_empty()) {
        Some(reason) => format!("{base}\n  {LOG_REASON_KEY}{reason}"),
        None => base,
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
