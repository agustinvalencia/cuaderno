//! `orientation_context`: the composed daily-orientation snapshot the
//! `orient` CLI (and later the Tauri home view) renders. It stitches
//! together three existing domain queries rather than computing
//! anything new — commitments, active-project summaries, and lapsed
//! stewardship habits — so the morning view is a single call.

use chrono::NaiveDate;

use crate::error::DomainError;

use super::{CommitmentEntry, ProjectSummary, Vault};

/// Commitments lookahead for orientation: 48 hours. The underlying
/// `commitments` query also folds in its standing 30-day overdue
/// look-back, so the morning view shows both "due soon" and "missed".
const ORIENTATION_LOOKAHEAD_DAYS: i64 = 2;

/// The composed snapshot for the daily orient flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrientationContext {
    /// Commitments due within 48h, plus anything overdue in the last
    /// 30 days, date-sorted.
    pub commitments: Vec<CommitmentEntry>,
    /// One summary per active project (state snippet + top next action).
    pub projects: Vec<ProjectSummary>,
    /// Stewardship habits that have fallen behind their cadence. Empty
    /// until the stewardship layer lands in Phase 3.
    pub lapsed_habits: Vec<LapsedHabit>,
}

/// A stewardship periodic habit that has lapsed past its cadence
/// (design §5.7 — e.g. "Swimming 1x/week — lapsed since March").
/// Produced only once stewardships exist (Phase 3); the field shape is
/// fixed now so the orient surface and its CLI don't churn later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LapsedHabit {
    /// Slug of the owning stewardship.
    pub stewardship: String,
    /// Human-readable description of the lapse.
    pub detail: String,
}

impl Vault {
    /// Compose the daily-orientation snapshot as of `today`.
    ///
    /// Pure composition over existing queries: `commitments` (48h
    /// window + overdue look-back), a `project_summary` per active
    /// project, and the lapsed-habit scan (empty until Phase 3). A
    /// malformed project propagates the error rather than being
    /// dropped — orientation should surface vault problems, not hide
    /// them.
    pub fn orientation_context(&self, today: NaiveDate) -> Result<OrientationContext, DomainError> {
        let commitments = self.commitments(today, ORIENTATION_LOOKAHEAD_DAYS)?;

        let mut projects = Vec::new();
        for (path, _frontmatter) in self.active_projects()? {
            let slug = path
                .as_path()
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            projects.push(self.project_summary(slug)?);
        }

        // Stewardship periodic habits arrive in Phase 3; until then the
        // lapsed-habit scan has nothing to inspect.
        let lapsed_habits = Vec::new();

        Ok(OrientationContext {
            commitments,
            projects,
            lapsed_habits,
        })
    }
}
