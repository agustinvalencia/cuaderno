//! `weekly_context`: the composed snapshot the weekly review reads.
//!
//! Stitches together existing domain queries over the ISO week (Mon–Sun)
//! containing `today` — the week's daily-log lines, what got done, the
//! projects closed, the project state changes — plus a two-week
//! commitments lookahead, so the review has one home for its fields
//! (RFC 0006 §8.0). Distinct from `weekly`, which is the weekly review
//! *note*.
//!
//! Nothing here is bounded: the token-safety caps an interface applies
//! (the MCP server's log cap and state gist) are interface concerns and
//! stay with the interface.

use chrono::{Duration, NaiveDate};

use crate::error::DomainError;

use super::context::monday_of_iso_week;
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
        })
    }
}
