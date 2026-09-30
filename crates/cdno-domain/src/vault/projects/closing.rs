//! Closing a project: `complete_project` and `drop_project` (RFC 0004).
//!
//! Both move the map to `projects/_done/<year>/<slug>.md`, stamp `status`
//! and `closed:`, swap the index rows and write one line to today's daily
//! log, in one transaction. Both accept an active or a parked project and
//! never check the cap, since closing never adds an active project
//! (§5.1, D8).
//!
//! A project with an open action or milestone is refused with the open
//! items listed (§5.3). `complete_project` never lets them go: completion is
//! a claim that the work is done, so each item is completed or dropped
//! first (D11). `drop_project` may, on explicit request (`OpenItems::Drop`),
//! drop them with it: the cascade, in the same transaction, or nothing.

use std::collections::HashMap;

use chrono::{Datelike, NaiveDateTime};
use serde_json::Value;

use cdno_core::error::StoreError;
use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::{ActionStatus, ProjectStatus};
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::closure::Closure;
use super::super::commitments::body_title_or_slug;
use super::super::frontmatter_edit::merge_fields_into_frontmatter;
use super::super::index_entry::build_index_entry_for;
use super::super::normalise::reorder_frontmatter;
use super::super::write_outcome::WriteOutcome;
use super::actions::{
    LOG_REASON_KEY, flatten_reason, format_action_dropped_log_entry, remove_action_line,
};
use super::lifecycle::stage_moved_milestone_rows;
use super::milestones::{format_milestone_dropped_log_entry, remove_milestone_block};
use super::open_items::{LinkedCommitment, OpenItems, OpenItemsReport};
use super::{MILESTONES_SECTION, NEXT_ACTIONS_SECTION};

/// What closing a project did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectClosureOutcome {
    /// `primary` is the closed map at `projects/_done/<year>/<slug>.md`;
    /// `paths` is every file the commit wrote.
    pub outcome: WriteOutcome,
    /// The open action bullets the cascade let go, as they read.
    pub dropped_actions: Vec<String>,
    /// The open milestones the cascade let go, by title.
    pub dropped_milestones: Vec<String>,
    /// Active standalone commitments that name the project. Never touched
    /// (RFC 0004 D4); returned so the caller can say they are still open.
    pub untouched_commitments: Vec<LinkedCommitment>,
}

/// The marker opening the daily-log line for a completed project.
pub(in crate::vault) const LOG_PROJECT_COMPLETED_PREFIX: &str = "project completed ";
/// The marker opening the daily-log line for a dropped project.
pub(in crate::vault) const LOG_PROJECT_DROPPED_PREFIX: &str = "project dropped on ";

impl Vault {
    /// Complete a project: the work is done.
    ///
    /// Refused with [`DomainError::ProjectHasOpenItems`] while any action
    /// or milestone is open, always: completion is a claim about work, so
    /// each open item is completed or dropped first (RFC 0004 D11). Linked
    /// commitments do not block and are never touched; they come back in
    /// the outcome.
    ///
    /// Errors: `ProjectNotActive` (with the project's status) for a map that
    /// is neither active at `projects/` nor parked at `projects/_parked/`,
    /// `ProjectHasOpenItems`, `AmbiguousProject` (also when a closed map
    /// already holds the slug, so the destination is occupied),
    /// `Store(NotFound)`, and `Store(AlreadyExists)` when a file appears at
    /// the destination behind the lock's back. Nothing is written on any of
    /// those. `IndexStale` means the files were written and the index lags;
    /// the next startup reconciles it.
    pub fn complete_project(
        &self,
        at: NaiveDateTime,
        slug: &str,
    ) -> Result<ProjectClosureOutcome, DomainError> {
        self.close_project(at, slug, Closure::Completed, None, OpenItems::Refuse)
    }

    /// Drop a project: it is not going to happen, from active or parked.
    ///
    /// `reason` rides an indented continuation line under the log entry.
    /// With open items and `OpenItems::Refuse` (the default on every
    /// surface) the call is refused with them listed, like
    /// [`complete_project`](Self::complete_project).
    ///
    /// `OpenItems::Drop` cascades, in the same transaction: every open
    /// bullet and milestone the report lists leaves the map, each attached
    /// action note not already closed is archived as dropped (blocked
    /// included, RFC 0004 D12), and each gets its own `… dropped on` line
    /// with `reason: project dropped` (plus the project's reason in
    /// parentheses) before the project's line. The call is refused with
    /// the fresh report instead when `expected` differs from its hash, or
    /// is absent while `hash_required` (D10).
    ///
    /// Errors: as `complete_project`, plus `Store(AlreadyExists)` when an
    /// attached note's `actions/_done/<year>/` path is occupied.
    pub fn drop_project(
        &self,
        at: NaiveDateTime,
        slug: &str,
        reason: Option<&str>,
        open_items: OpenItems,
    ) -> Result<ProjectClosureOutcome, DomainError> {
        self.close_project(at, slug, Closure::Dropped, reason, open_items)
    }

    fn close_project(
        &self,
        at: NaiveDateTime,
        slug: &str,
        closure: Closure,
        reason: Option<&str>,
        open_items: OpenItems,
    ) -> Result<ProjectClosureOutcome, DomainError> {
        // The lock is held from before the first read to the commit, so the
        // report the refusal shows, and the hash a cascade is checked
        // against, is the state the close acts on.
        let mut tx = self.transaction()?;
        let (path, mut doc) = self.resolve_closable_project(slug)?;

        let report = self.open_items_report(&doc, slug)?;
        if !report.is_empty() && !cascade_confirmed(closure, &open_items, &report) {
            return Err(DomainError::ProjectHasOpenItems {
                slug: slug.to_owned(),
                report,
            });
        }
        let today = at.date();

        // The cascade (a confirmed drop with open items; a no-op otherwise).
        // Each open line the report listed leaves the map, and each attached
        // note still in play is archived as dropped (RFC 0004 D12: blocked
        // included). A note already closed by hand stays where it is: its
        // bullet goes, but archiving it would restamp a completed note as
        // dropped.
        remove_open_items(&mut doc, &report)?;
        let mut archived: Vec<&str> = Vec::new();
        for action in &report.actions {
            let (Some(note), Some(status)) = (action.note.as_deref(), action.note_status) else {
                continue;
            };
            if matches!(status, ActionStatus::Completed | ActionStatus::Dropped)
                || archived.contains(&note)
            {
                continue;
            }
            archived.push(note);
        }

        // A collision anywhere refuses with nothing written: staging only
        // buffers, and `stage_action_archival` refuses an occupied
        // `actions/_done/` path itself, so an error before `commit` leaves
        // every file and index row as it was. The locator already refuses a
        // slug with a map at the map's destination (`AmbiguousProject`), so
        // the check below only guards a file written behind the lock's back.
        let dest = VaultPath::new(format!(
            "{}/{slug}.md",
            cdno_core::paths::projects_done_dir(today.year())
        ))?;
        if self.store.exists(&dest)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                dest.to_string(),
            )));
        }

        // Stamp the document already in hand, not a fresh read: the cascade
        // edited it above, and re-reading would discard the edits.
        let status = match closure {
            Closure::Completed => ProjectStatus::Completed,
            Closure::Dropped => ProjectStatus::Dropped,
        };
        let mut fields = serde_json::Map::new();
        fields.insert("status".to_owned(), Value::from(status.as_str()));
        fields.insert(
            "closed".to_owned(),
            Value::from(today.format("%Y-%m-%d").to_string()),
        );
        let stamped = merge_fields_into_frontmatter(doc.render(), &fields)?;
        // The merge appends a missing `closed:` last; the effective
        // template (a customised one may list it mid-block) decides where
        // it belongs, so a pre-RFC map leaves in canonical order.
        let order = self.canonical_frontmatter_order(
            NoteType::Project.as_str(),
            &stamped,
            &mut HashMap::new(),
        )?;
        let new_content = reorder_frontmatter(&stamped, &order).unwrap_or(stamped);

        let entry_meta = build_index_entry_for(&dest, &new_content, NoteType::Project.as_str())?;
        let title = body_title_or_slug(&new_content, slug).to_owned();

        // One line per dropped child, children first, each carrying why it
        // went, then the project's own line; one daily-note write.
        let child_reason = match reason.map(str::trim).filter(|r| !r.is_empty()) {
            Some(reason) => format!("{CASCADE_REASON} ({reason})"),
            None => CASCADE_REASON.to_owned(),
        };
        let mut log_entries: Vec<String> = Vec::new();
        for action in &report.actions {
            log_entries.push(format_action_dropped_log_entry(
                slug,
                &action.text,
                Some(&child_reason),
            ));
        }
        for milestone in &report.milestones {
            log_entries.push(format_milestone_dropped_log_entry(
                slug,
                &milestone.title,
                Some(&child_reason),
            ));
        }
        log_entries.push(format_project_closed_log_entry(
            closure, slug, &title, reason,
        ));
        let log_refs: Vec<&str> = log_entries.iter().map(String::as_str).collect();

        for note in &archived {
            self.stage_action_archival(at, note, Closure::Dropped, &mut tx)?;
        }
        tx.write_file(dest.clone(), new_content);
        tx.delete_file(path.clone());
        tx.upsert_note(entry_meta);
        stage_moved_milestone_rows(&dest, &doc, &mut tx);
        tx.remove_note(path);
        self.stage_daily_logs(at, &log_refs, &mut tx)?;
        let touched = tx.commit()?;

        Ok(ProjectClosureOutcome {
            outcome: WriteOutcome::written(dest, touched),
            dropped_actions: report.actions.into_iter().map(|a| a.text).collect(),
            dropped_milestones: report.milestones.into_iter().map(|m| m.title).collect(),
            untouched_commitments: report.untouched_commitments,
        })
    }
}

/// The reason each cascaded child's log line carries, with the project's
/// own reason appended in parentheses when there is one.
const CASCADE_REASON: &str = "project dropped";

/// Whether a close may go ahead over the open items in `report`. Only a
/// drop cascades (a completion never does, RFC 0004 D11), and only on an
/// explicit `OpenItems::Drop` whose hash, when present, matches the fresh
/// report, and is present when the caller said it must be (D10).
fn cascade_confirmed(closure: Closure, open_items: &OpenItems, report: &OpenItemsReport) -> bool {
    match (closure, open_items) {
        (
            Closure::Dropped,
            OpenItems::Drop {
                expected,
                hash_required,
            },
        ) => match expected {
            Some(hash) => *hash == report.hash(),
            None => !hash_required,
        },
        _ => false,
    }
}

/// Remove every open line `report` lists from `doc`, through the same
/// section edits `drop_action` and `drop_milestone` make, at the index each
/// entry carries (the report picked them with the verbs' own predicates).
///
/// Each section is edited from its highest listed index down, so a removal
/// never shifts a line still to be removed. That order also settles nested
/// milestones: an indented open `- [ ]` under an open parent sits below it,
/// so it goes (as its own listed item) before its parent's block is cut,
/// and the parent never takes it along as a continuation line.
fn remove_open_items(
    doc: &mut MarkdownDocument,
    report: &OpenItemsReport,
) -> Result<(), DomainError> {
    if !report.actions.is_empty() {
        let mut lines: Vec<usize> = report.actions.iter().map(|a| a.line).collect();
        lines.sort_unstable_by(|a, b| b.cmp(a));
        let mut section = doc.section(NEXT_ACTIONS_SECTION)?.to_owned();
        for idx in lines {
            section = remove_action_line(&section, idx);
        }
        doc.replace_section(NEXT_ACTIONS_SECTION, &section)?;
    }
    if !report.milestones.is_empty() {
        let mut lines: Vec<usize> = report.milestones.iter().map(|m| m.line).collect();
        lines.sort_unstable_by(|a, b| b.cmp(a));
        let mut section = doc.section(MILESTONES_SECTION)?.to_owned();
        for idx in lines {
            section = remove_milestone_block(&section, idx);
        }
        doc.replace_section(MILESTONES_SECTION, &section)?;
    }
    Ok(())
}

/// The daily-log line recording a closure (RFC 0004 D6):
/// `project completed [[<slug>]] — <title>` or
/// `project dropped on [[<slug>]] — <title>`, matching the commitment
/// verbs. The bare slug is linked, so the project-mention matchers find
/// it. A reason rides an indented continuation line, flattened so one
/// closure stays one entry.
fn format_project_closed_log_entry(
    closure: Closure,
    slug: &str,
    title: &str,
    reason: Option<&str>,
) -> String {
    let prefix = match closure {
        Closure::Completed => LOG_PROJECT_COMPLETED_PREFIX,
        Closure::Dropped => LOG_PROJECT_DROPPED_PREFIX,
    };
    let base = format!("{prefix}[[{slug}]] \u{2014} {}", flatten_reason(title));
    match reason.map(flatten_reason).filter(|r| !r.is_empty()) {
        Some(reason) => format!("{base}\n  {LOG_REASON_KEY}{reason}"),
        None => base,
    }
}
