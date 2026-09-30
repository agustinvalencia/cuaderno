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
//! first (D11). `drop_project` may, on explicit request (`OpenItems::Drop`,
//! the cascade).

use std::collections::HashMap;

use chrono::{Datelike, NaiveDateTime};
use serde_json::Value;

use cdno_core::error::StoreError;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::ProjectStatus;
use crate::note_type::NoteType;

use super::super::Vault;
use super::super::closure::Closure;
use super::super::commitments::body_title_or_slug;
use super::super::frontmatter_edit::merge_fields_into_frontmatter;
use super::super::index_entry::build_index_entry_for;
use super::super::normalise::reorder_frontmatter;
use super::super::write_outcome::WriteOutcome;
use super::actions::{LOG_REASON_KEY, flatten_reason};
use super::lifecycle::stage_moved_milestone_rows;
use super::open_items::{LinkedCommitment, OpenItems};

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
    /// Errors: as `complete_project`.
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
        // report the refusal shows is the state the close acts on.
        let mut tx = self.transaction()?;
        let (path, doc) = self.resolve_closable_project(slug)?;

        let report = self.open_items_report(&doc, slug)?;
        if !report.is_empty() {
            // The cascade (`OpenItems::Drop` on a drop) is not implemented
            // yet, so any open item refuses, whatever was asked.
            let _ = open_items;
            return Err(DomainError::ProjectHasOpenItems {
                slug: slug.to_owned(),
                report,
            });
        }

        // Stamp the document already in hand, not a fresh read: the cascade
        // edits it before this point, and re-reading would discard them.
        let today = at.date();
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

        // The locator already refuses a slug with a map at the destination
        // (`AmbiguousProject`), so this only guards a file written behind
        // the lock's back. Checked before anything is staged.
        let dest = VaultPath::new(format!(
            "{}/{slug}.md",
            cdno_core::paths::projects_done_dir(today.year())
        ))?;
        if self.store.exists(&dest)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                dest.to_string(),
            )));
        }

        let entry_meta = build_index_entry_for(&dest, &new_content, NoteType::Project.as_str())?;
        let title = body_title_or_slug(&new_content, slug).to_owned();
        let log_entry = format_project_closed_log_entry(closure, slug, &title, reason);

        tx.write_file(dest.clone(), new_content);
        tx.delete_file(path.clone());
        tx.upsert_note(entry_meta);
        stage_moved_milestone_rows(&dest, &doc, &mut tx);
        tx.remove_note(path);
        self.stage_daily_log(at, &log_entry, &mut tx)?;
        let touched = tx.commit()?;

        Ok(ProjectClosureOutcome {
            outcome: WriteOutcome::written(dest, touched),
            dropped_actions: Vec::new(),
            dropped_milestones: Vec::new(),
            untouched_commitments: report.untouched_commitments,
        })
    }
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
