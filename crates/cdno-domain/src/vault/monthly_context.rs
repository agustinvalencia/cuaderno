//! `monthly_context`: the composed snapshot the monthly review reads.
//!
//! The strategic scan of design §11, stitched together from existing
//! domain queries: the past 30 days' completions and closures, the
//! parked shelf, the active questions, portfolio health, stuck-detection,
//! every stewardship, a six-week commitments lookahead and the slot
//! usage against the cap — so the review has one home for its fields
//! (RFC 0006 §8.0). Distinct from `monthly`, which is the monthly review
//! *note*.

use chrono::{Duration, NaiveDate};

use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::frontmatter::ProjectFrontmatter;

use super::{
    ClosedProjectEntry, CommitmentEntry, CompletedActionEntry, PortfolioSummary, ProjectSummary,
    QuestionSummary, SlotUsage, StewardshipSummary, Vault,
};

/// The "past month" window: `today` minus 30 days.
const MONTHLY_WINDOW_DAYS: i64 = 30;

/// Design §11: "project stuck-check, unchanged >2 weeks" — 14 days.
const STUCK_AFTER_DAYS: i64 = 14;

/// Commitments lookahead for the monthly review: six weeks (design §11).
const MONTHLY_COMMITMENTS_LOOKAHEAD_DAYS: i64 = 42;

/// The composed snapshot for the monthly review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthlyContext {
    /// Start of the window: `today` minus 30 days.
    pub since: NaiveDate,
    /// Actions completed in the window, in both forms (note and inline
    /// bullet), oldest first.
    pub completed_actions: Vec<CompletedActionEntry>,
    /// Projects completed or dropped in the window, by closing date.
    pub closed_projects: Vec<ClosedProjectEntry>,
    /// Every parked project map, with its path and frontmatter.
    pub parked_projects: Vec<(VaultPath, ProjectFrontmatter)>,
    /// Every question with `status: active`.
    pub active_questions: Vec<QuestionSummary>,
    /// Every portfolio with its evidence count and staleness.
    pub portfolios: Vec<PortfolioSummary>,
    /// Active projects whose map has not changed in 14 days.
    pub stuck_projects: Vec<ProjectSummary>,
    /// Every stewardship dashboard.
    pub stewardships: Vec<StewardshipSummary>,
    /// Commitments due in the next six weeks, plus anything overdue.
    pub commitments: Vec<CommitmentEntry>,
    /// Active-project slots in use against the cap.
    pub slots: SlotUsage,
}

impl Vault {
    /// Compose the monthly-review snapshot as of `today`.
    pub fn monthly_context(&self, today: NaiveDate) -> Result<MonthlyContext, DomainError> {
        let since = today - Duration::days(MONTHLY_WINDOW_DAYS);
        Ok(MonthlyContext {
            since,
            completed_actions: self.completed_actions_between(since, today)?,
            closed_projects: self.closed_projects_between(since, today)?,
            parked_projects: self.parked_projects()?,
            active_questions: self.active_questions()?,
            portfolios: self.list_portfolios(today)?,
            stuck_projects: self.stuck_projects(today, STUCK_AFTER_DAYS)?,
            stewardships: self.list_stewardships(today)?,
            commitments: self.commitments(today, MONTHLY_COMMITMENTS_LOOKAHEAD_DAYS)?,
            slots: self.slot_usage()?,
        })
    }
}
