//! `weekly_context`: the composed snapshot the weekly review reads.
//!
//! Stitches together existing domain queries over the ISO week (Mon–Sun)
//! containing `today` — the week's daily-log lines, what got done, the
//! projects closed, the project state changes — plus a two-week
//! commitments lookahead, so the review has one home for its fields
//! (RFC 0006 §8.0). Distinct from `weekly`, which is the weekly review
//! *note*.
//!
//! It also lists every active project map for the review (RFC 0006
//! §8.5), each with what its relationship to the person implies asking:
//! who holds it, whether it counts, whether its Waiting On says anything,
//! whether the map changed this week, and, for a handed-off map, on how
//! many days this week a focus was started on it. That count is evidence
//! for the person to weigh, never a verdict: no log line has an author.
//!
//! Nothing here is bounded: the token-safety caps an interface applies
//! (the MCP server's log cap and state gist) are interface concerns and
//! stay with the interface.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, NaiveDate};

use cdno_core::frontmatter::Frontmatter;

use crate::error::DomainError;
use crate::frontmatter::{ProjectFrontmatter, ProjectStatus};
use crate::note_type::NoteType;

use super::context::{
    monday_of_iso_week, mtime_threshold_ns, normalise_project_target, parse_focus_marker, path_stem,
};
use super::projects::actions::{LOG_RESUMED_PREFIX, LOG_STARTED_PREFIX};
use super::projects::is_under_projects_done;
use super::{
    ClosedProjectEntry, CommitmentEntry, CompletedActionEntry, DailyLogLine, ProjectStateChange,
    Vault,
};

/// Commitments lookahead for the weekly review: two weeks (design §11).
/// The underlying `commitments` query also folds in its standing 30-day
/// overdue look-back.
const WEEKLY_COMMITMENTS_LOOKAHEAD_DAYS: i64 = 14;

/// The composed snapshot for the weekly review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeeklyContext {
    /// The Monday of the ISO week the other fields cover.
    pub week_of: NaiveDate,
    /// Every daily-log line of the week, oldest first, unbounded.
    pub logs: Vec<DailyLogLine>,
    /// Actions completed during the week, in both forms (note and
    /// inline bullet), oldest first.
    pub completed_actions: Vec<CompletedActionEntry>,
    /// Projects completed or dropped during the week, by closing date.
    pub closed_projects: Vec<ClosedProjectEntry>,
    /// `## Current State` changes logged during the week, with their
    /// full before and after bodies.
    pub state_changes: Vec<ProjectStateChange>,
    /// Commitments due in the next two weeks, plus anything overdue.
    pub commitments: Vec<CommitmentEntry>,
    /// Every active project map, held ones included, sorted by slug.
    /// Parked maps are read at the monthly review, not here.
    pub projects: Vec<ReviewProject>,
}

/// One active project as the weekly review reads it (RFC 0006 §8.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewProject {
    pub slug: String,
    /// The frontmatter `held_by:` as the lenient reader keeps it; see
    /// [`ProjectSummary::held_by`](super::ProjectSummary::held_by).
    pub held_by: Option<String>,
    /// Whether the map holds one of the `max_active_projects` slots.
    pub counts: bool,
    /// Whether `## Waiting On` says nothing (missing, empty, or only the
    /// placeholder).
    pub waiting_on_empty: bool,
    /// The frontmatter `core_question:` exactly as written.
    pub core_question: Option<String>,
    /// Whether the map file was modified since this ISO week began:
    /// its index `mtime_ns` falls after the end of the Sunday before
    /// `week_of`. A tool that touches the file without changing it also
    /// counts, so this over-reports; it prompts a question, never a
    /// conclusion.
    pub changed_this_week: bool,
    /// For a handed-off map (`counts` false: an active map that names
    /// someone else as its holder), the
    /// number of distinct days from the ISO Monday to today with a
    /// `started` or `resumed` log head naming it; zero for any other map.
    /// Days, not lines: three starts on one day are one day.
    pub focus_days_while_held: usize,
}

impl Vault {
    /// Compose the weekly-review snapshot for the ISO week containing
    /// `today`.
    pub fn weekly_context(&self, today: NaiveDate) -> Result<WeeklyContext, DomainError> {
        let monday = monday_of_iso_week(today);
        let sunday = monday + Duration::days(6);
        Ok(WeeklyContext {
            week_of: monday,
            logs: self.weekly_logs(today)?,
            completed_actions: self.completed_actions_between(monday, sunday)?,
            closed_projects: self.closed_projects_between(monday, sunday)?,
            state_changes: self.project_state_changes_between(monday, sunday)?,
            commitments: self.commitments(today, WEEKLY_COMMITMENTS_LOOKAHEAD_DAYS)?,
            projects: self.review_projects(monday, today)?,
        })
    }

    /// Every active project map as a [`ReviewProject`], sorted by slug.
    /// Mirrors the stuck scan's walk: closed maps are skipped unread, the
    /// rest are read to confirm `status: active`, and the summary fields
    /// come from [`Vault::project_summary`].
    fn review_projects(
        &self,
        monday: NaiveDate,
        today: NaiveDate,
    ) -> Result<Vec<ReviewProject>, DomainError> {
        // A map modified after the last instant of the Sunday before
        // `monday` changed this week.
        let week_start_ns = mtime_threshold_ns(monday, 1);
        let focus_days = self.focus_start_days(monday, today)?;
        let mut out = Vec::new();
        for entry in self.index.list_by_type(NoteType::Project.as_str())? {
            if is_under_projects_done(&entry.path) {
                continue;
            }
            let raw = self.store.read_file(&entry.path)?;
            let (fm, _body) = Frontmatter::parse(&raw)?;
            let pf = ProjectFrontmatter::try_from(fm)?;
            if pf.status != ProjectStatus::Active {
                continue;
            }
            let summary = self.project_summary(&path_stem(&entry.path))?;
            // Every map here is active, so not counting means it names
            // someone else as its holder: it is handed off.
            let focus_days_while_held = if !summary.counts {
                focus_days.get(&summary.slug).map_or(0, BTreeSet::len)
            } else {
                0
            };
            out.push(ReviewProject {
                changed_this_week: entry.mtime_ns > week_start_ns,
                focus_days_while_held,
                slug: summary.slug,
                held_by: summary.held_by,
                counts: summary.counts,
                waiting_on_empty: summary.waiting_on_empty,
                core_question: summary.core_question,
            });
        }
        out.sort_by(|a, b| a.slug.cmp(&b.slug));
        Ok(out)
    }

    /// For each project slug, the distinct days in `from..=to` whose daily
    /// log has a `started` or `resumed` head naming it, read through
    /// [`Vault::focus_heads`].
    fn focus_start_days(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<BTreeMap<String, BTreeSet<NaiveDate>>, DomainError> {
        let mut days: BTreeMap<String, BTreeSet<NaiveDate>> = BTreeMap::new();
        for day in from.iter_days().take_while(|d| *d <= to) {
            for (_stamp, text) in self.focus_heads(day)? {
                if let Some((project, _action)) = parse_focus_marker(&text, LOG_STARTED_PREFIX)
                    .or_else(|| parse_focus_marker(&text, LOG_RESUMED_PREFIX))
                {
                    days.entry(normalise_project_target(&project))
                        .or_default()
                        .insert(day);
                }
            }
        }
        Ok(days)
    }
}
