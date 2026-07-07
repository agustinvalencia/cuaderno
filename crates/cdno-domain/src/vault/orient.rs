//! `orientation_context`: the composed daily-orientation snapshot the
//! `orient` CLI (and later the Tauri home view) renders. It stitches
//! together three existing domain queries rather than computing
//! anything new — commitments, active-project summaries, and lapsed
//! stewardship habits — so the morning view is a single call.

use chrono::NaiveDate;

use cdno_core::markdown::MarkdownDocument;

use crate::error::DomainError;
use crate::note_type::NoteType;

use super::stewardships::stewardship_slug_from_path;
use super::{CommitmentEntry, ProjectSummary, Vault};

/// Commitments lookahead for orientation: 48 hours. The underlying
/// `commitments` query also folds in its standing 30-day overdue
/// look-back, so the morning view shows both "due soon" and "missed".
const ORIENTATION_LOOKAHEAD_DAYS: i64 = 2;

/// Heading of the section that holds habit status lines on a
/// stewardship dashboard (design §5.6).
const ACTIVE_HABITS_SECTION: &str = "Active Habits";

/// The composed snapshot for the daily orient flow.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OrientationContext {
    /// Commitments due within 48h, plus anything overdue in the last
    /// 30 days, date-sorted.
    pub commitments: Vec<CommitmentEntry>,
    /// One summary per active project (state snippet + top next action).
    pub projects: Vec<ProjectSummary>,
    /// Stewardship habits whose dashboard line declares them lapsed.
    pub lapsed_habits: Vec<LapsedHabit>,
}

/// A stewardship habit whose `## Active Habits` line declares a lapse
/// (design §5.6 — e.g. "Swimming 1x/week — lapsed since March").
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
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
    /// project, and the lapsed-habit scan over stewardship
    /// dashboards. A malformed project propagates the error rather
    /// than being dropped — orientation should surface vault
    /// problems, not hide them.
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

        let lapsed_habits = self.lapsed_habits()?;

        Ok(OrientationContext {
            commitments,
            projects,
            lapsed_habits,
        })
    }

    /// Habit lines that declare themselves lapsed, gathered from every
    /// stewardship dashboard's `## Active Habits` section.
    ///
    /// The dashboard is the source of truth for habit health: each
    /// line carries a prose status after an em-dash ("on track",
    /// "lapsed since March", "inconsistent" — design §5.6), maintained
    /// by the weekly review. The scan surfaces the lines whose status
    /// starts with "lapsed" rather than inferring lapses from tracking
    /// cadence — free-text cadences ("3x/week", "before midnight")
    /// don't parse reliably, and a wrong inference here would be a
    /// guilt-generator, the failure mode the orient surface exists to
    /// avoid.
    ///
    /// Dashboards without an `## Active Habits` section, and lines
    /// that don't fit the `- {habit} — {status}` shape, are skipped
    /// silently — lint is the place to surface malformed dashboards.
    pub fn lapsed_habits(&self) -> Result<Vec<LapsedHabit>, DomainError> {
        let mut out = Vec::new();
        for entry in self.index.list_by_type(NoteType::Stewardship.as_str())? {
            let raw = self.store.read_file(&entry.path)?;
            let slug = stewardship_slug_from_path(&entry.path);
            let Ok(doc) = MarkdownDocument::parse(raw) else {
                continue;
            };
            let Ok(section) = doc.section(ACTIVE_HABITS_SECTION) else {
                continue;
            };
            for line in section.lines() {
                if let Some(detail) = parse_lapsed_habit_line(line) {
                    out.push(LapsedHabit {
                        stewardship: slug.clone(),
                        detail,
                    });
                }
            }
        }
        out.sort_by(|a, b| {
            a.stewardship
                .cmp(&b.stewardship)
                .then_with(|| a.detail.cmp(&b.detail))
        });
        Ok(out)
    }
}

/// Parse one `## Active Habits` line, returning its full text when the
/// status segment declares a lapse. The canonical shape is
/// `- {habit} — {status}`; a line is lapsed when any em-dash-separated
/// segment after the first starts with "lapsed" (case-insensitive), so
/// "lapsed since March" and "Lapsed (2w)" both match while a habit
/// *named* "lapsed-thing" does not.
fn parse_lapsed_habit_line(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("- ")?.trim();
    let mut segments = rest.split('\u{2014}');
    segments.next()?; // the habit text itself; status segments follow
    let lapsed = segments.any(|segment| {
        segment
            .trim()
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("lapsed"))
    });
    lapsed.then(|| rest.to_owned())
}
