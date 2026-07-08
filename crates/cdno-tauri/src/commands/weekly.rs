//! Weekly Review commands (plan §1.4, #55): the one composed read that
//! feeds the guided 5-step flow, and the single section-write it drives.
//!
//! `get_weekly_bundle` stitches together everything the review needs in
//! one invoke — the existing weekly note's sections, the week's wins
//! seed (completed actions + daily logs), the project/stewardship scans,
//! the commitments lookahead, and the stuck-project staleness set — so
//! the frontend never fans out a handful of reads to paint one page.
//! `save_weekly_section` wraps `upsert_weekly_section`: each step's save
//! is complete in itself (the anti-chore rule), so there is one write,
//! not a submit-the-whole-review commit.
//!
//! Wire-shape note: the review-content types the frontend renders
//! (`CompletedActionView`, `WeeklyLogLine`, `StuckProject`, the parsed
//! `WeeklyContent`) are thin view-models owned here, mirroring
//! `projects.rs`'s `MilestoneView` / `LogMentionView`. The domain
//! summaries that already carry `ts-rs` derives — `ProjectSummary`,
//! `StewardshipSummary`, `CommitmentEntry` — ride the wire directly.

use std::str::FromStr;

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime};

use cdno_core::markdown::MarkdownDocument;
use cdno_core::path::VaultPath;
use cdno_domain::Vault;
use cdno_domain::vault::{
    CommitmentEntry, ProjectSummary, StewardshipSummary, WeeklyNoteView, WeeklySection,
};

use crate::error::CmdError;
use crate::events::VaultArea;
use crate::state::AppState;
use crate::with_vault::with_vault;

use super::actions::record_and_emit;

/// A project map is flagged "stuck" once its file has sat unmodified
/// for at least this many days — the plan's staleness threshold (§1.4,
/// "state untouched for N days"). Informative, never accusatory.
const STUCK_THRESHOLD_DAYS: i64 = 7;

/// The forward window of the commitments query the review's step 4
/// shows: the next two weeks (plan §1.4). The domain query additionally
/// folds in its fixed overdue look-back, so the result is not purely
/// forward-looking. Reuses the shared timeline component.
const COMMITMENTS_LOOKAHEAD_DAYS: i64 = 14;

/// Upper bound on log lines carried into the wins seed. A busy week can
/// accumulate hundreds of `## Logs` entries; the seed only needs a
/// representative handful, so the bundle stays bounded rather than
/// shipping the whole week's log to the webview. When a week overflows
/// this cap the *most recent* lines are kept (see the seed composition),
/// because a memory jog wants the recent past, not Monday morning.
const MAX_LOG_LINES: usize = 100;

/// The composed Weekly Review data (plan §1.4). One read, every panel:
/// the resolved week anchor (and the following week's, for the focus
/// save), the existing note's sections, next week's goal, the wins-seed
/// sources, the project and stewardship scans, the stuck set, and the
/// commitments lookahead (14 days forward plus the overdue look-back).
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct WeeklyBundle {
    /// The Monday of the reviewed ISO week — a stable identity the
    /// frontend labels the week with and echoes back into
    /// `save_weekly_section`. Any day in the week resolves here.
    pub week_of: NaiveDate,
    /// The Monday of the week *after* the reviewed one. The Focus step's
    /// "next week's focus" belongs to next week's note (weekly.rs: the
    /// goal is "carried into the next week's note by the review"), not
    /// the week under review — so the frontend echoes this back into
    /// `save_weekly_section` for the focus save. Exposed here so the
    /// frontend never does date arithmetic (plan §3.7).
    pub next_week_of: NaiveDate,
    /// The real current date the lookahead was computed against
    /// (stamped in Rust, like `CommitmentsView.today`), so the shared
    /// timeline can label months and split past/upcoming without
    /// touching a clock.
    pub today: NaiveDate,
    /// The weekly note's existing section content, so step 1 (Wins) can
    /// prefer what's already written over the seed.
    pub weekly: WeeklyContent,
    /// Next week's existing goal, read from the note at `next_week_of`
    /// (`None` when that note doesn't exist yet or its goal section is
    /// empty). The Focus step seeds from this so it edits an
    /// already-planned goal rather than blindly overwriting it — and
    /// crucially reads NEXT week's goal, not the reviewed week's.
    pub next_week_goal: Option<String>,
    /// Action notes completed within the reviewed week — the primary
    /// wins-seed source ("Completed: {title} ({project})").
    pub completed_actions: Vec<CompletedActionView>,
    /// The week's daily-log lines (capped), the secondary wins-seed
    /// source and a memory jog for "what felt like progress anyway?".
    pub logs: Vec<WeeklyLogLine>,
    /// Active projects, each with its context and current-state snippet,
    /// for the step-2 inline scan. `context` rides along in the summary.
    pub projects: Vec<ProjectSummary>,
    /// Active projects whose map has sat untouched past the staleness
    /// threshold, paired with how many days — the grey step-2 hint.
    pub stuck: Vec<StuckProject>,
    /// Dated commitments for the step-4 lookahead: the next 14 days
    /// forward, plus anything overdue within the domain's 30-day
    /// look-back (the `commitments` query folds both together, so this
    /// is not purely forward-looking).
    pub commitments: Vec<CommitmentEntry>,
    /// Every stewardship with its tracking count and staleness, for the
    /// read-only step-3 scan.
    pub stewardships: Vec<StewardshipSummary>,
}

/// The writable sections of the weekly note, parsed from an existing
/// note (or all `None` when the week has no note yet). `None` also
/// stands in for a present-but-empty section — the scaffold seeds four
/// empty sections, and an empty section must not beat a fresh seed, so
/// the frontend treats "empty" and "absent" identically.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct WeeklyContent {
    /// Whether a note file exists for the week at all — distinguishes a
    /// never-started review from one whose sections are simply empty.
    pub exists: bool,
    pub wins: Option<String>,
    pub challenges: Option<String>,
    pub one_improvement: Option<String>,
    pub this_weeks_goal: Option<String>,
}

/// One completed action in the reviewed week, flattened from the domain
/// `CompletedActionEntry` (which carries a `VaultPath` that can't hold a
/// `ts-rs` derive). The seed line needs only title + project.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompletedActionView {
    pub slug: String,
    pub project: String,
    pub title: String,
    pub completed: NaiveDate,
}

/// One daily-log line in the reviewed week, flattened from the domain
/// `DailyLogLine` (which carries no serialisation derives). Mirrors
/// `projects.rs`'s `LogMentionView`.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct WeeklyLogLine {
    pub date: NaiveDate,
    pub time: NaiveTime,
    pub text: String,
}

/// A stuck active project: its slug and how many days its map has sat
/// unmodified. The frontend cross-references the slug against
/// `projects` to hang the grey "state untouched for N days" hint on the
/// matching card.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct StuckProject {
    pub slug: String,
    pub days_unchanged: i64,
}

/// Compose the Weekly Review bundle. Public and synchronous — the test
/// seam, exercised directly over the Memory doubles.
///
/// Two dates flow in, deliberately distinct: `anchor` is any day in the
/// ISO week under review (drives the week's note, wins, and logs), while
/// `today` is the real current date (drives the forward-looking
/// commitments lookahead, stewardship staleness, and the stuck scan).
/// For a normal Sunday review they coincide; keeping them separate lets
/// a review of a past week still show an accurate "as of now" lookahead.
/// `stuck_threshold_days` is a parameter (not the command's constant) so
/// the impl test can force a fresh project into the stuck set — the
/// Memory store stamps mtime at construction, so only a zero-day
/// threshold makes a just-written project register as stuck.
pub fn get_weekly_bundle_impl(
    vault: &Vault,
    today: NaiveDate,
    anchor: NaiveDate,
    stuck_threshold_days: i64,
) -> Result<WeeklyBundle, CmdError> {
    let monday = monday_of(anchor);
    let sunday = monday + Duration::days(6);
    // The focus save targets NEXT week's note (weekly.rs: the goal is
    // "carried into the next week's note by the review"), so the review
    // must never write its goal into the week it just looked back on.
    let next_monday = monday + Duration::days(7);

    // Existing sections first: the frontend prefers what's already
    // written over the seed for Wins.
    let weekly = parse_weekly_content(&vault.read_weekly_note(monday)?)?;

    // Next week's goal (if planning already set one) seeds the Focus
    // step. read_weekly_note tolerates a not-yet-created note — it
    // returns exists: false, which parse_weekly_content maps to all-None
    // — so a missing next-week note simply yields None here.
    let next_week_goal =
        parse_weekly_content(&vault.read_weekly_note(next_monday)?)?.this_weeks_goal;

    let completed_actions = vault
        .completed_actions_between(monday, sunday)?
        .into_iter()
        .map(|c| CompletedActionView {
            slug: c.slug,
            project: c.project,
            title: c.title,
            completed: c.completed,
        })
        .collect();

    // weekly_logs returns Monday-first chronological order. Cap the
    // count so a log-heavy week can't bloat the bundle, but keep the
    // MOST RECENT lines (drop from the front) rather than the earliest:
    // the seed is a memory jog, and the recent past is what needs
    // jogging. Skipping from the front preserves chronological order in
    // what survives.
    let all_logs = vault.weekly_logs(monday)?;
    let skip = all_logs.len().saturating_sub(MAX_LOG_LINES);
    let logs = all_logs
        .into_iter()
        .skip(skip)
        .map(|l| WeeklyLogLine {
            date: l.date,
            time: l.time,
            text: l.text,
        })
        .collect();

    // The step-2 scan wants the active set with context + state snippet;
    // project_summary carries both in one read (context rides along).
    let mut projects = Vec::new();
    for (path, _fm) in vault.active_projects()? {
        projects.push(vault.project_summary(&slug_of(&path))?);
    }

    let stuck = vault
        .stuck_project_days(today, stuck_threshold_days)?
        .into_iter()
        .map(|(slug, days_unchanged)| StuckProject {
            slug,
            days_unchanged,
        })
        .collect();

    let commitments = vault.commitments(today, COMMITMENTS_LOOKAHEAD_DAYS)?;
    let stewardships = vault.list_stewardships(today)?;

    Ok(WeeklyBundle {
        week_of: monday,
        next_week_of: next_monday,
        today,
        weekly,
        next_week_goal,
        completed_actions,
        logs,
        projects,
        stuck,
        commitments,
        stewardships,
    })
}

/// Parse the four writable sections out of a weekly note. A missing or
/// whitespace-only section maps to `None` (see [`WeeklyContent`]) so the
/// frontend's "prefer existing over seed" rule keys on real content
/// only. A note that exists but doesn't parse as markdown is a
/// user-fixable problem surfaced as `Invalid` rather than a hidden
/// internal fault.
fn parse_weekly_content(view: &WeeklyNoteView) -> Result<WeeklyContent, CmdError> {
    if !view.exists {
        return Ok(WeeklyContent::default());
    }
    let doc = MarkdownDocument::parse(view.markdown.clone())
        .map_err(|e| CmdError::Invalid(format!("weekly note is not valid markdown: {e}")))?;
    let section = |s: WeeklySection| -> Option<String> {
        doc.section(s.heading())
            .ok()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    Ok(WeeklyContent {
        exists: true,
        wins: section(WeeklySection::Wins),
        challenges: section(WeeklySection::Challenges),
        one_improvement: section(WeeklySection::OneImprovement),
        this_weeks_goal: section(WeeklySection::ThisWeeksGoal),
    })
}

/// The Monday of the ISO week containing `date` (locale-independent,
/// matching the domain's own week keying). Recomputed here because the
/// domain's helper is private; kept trivially in sync via the ISO
/// weekday arithmetic.
fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

/// The slug (file stem) of an active project's map path.
fn slug_of(path: &VaultPath) -> String {
    path.as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned()
}

/// Resolve the optional ISO-date argument into the anchor day for the
/// week under review — `None` means today. A malformed date is a
/// user-visible `Invalid`, never a silent fallback to today.
fn resolve_anchor(week_of: Option<String>, today: NaiveDate) -> Result<NaiveDate, CmdError> {
    match week_of {
        None => Ok(today),
        Some(s) => NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").map_err(|e| {
            CmdError::Invalid(format!("week_of must be an ISO date (YYYY-MM-DD): {e}"))
        }),
    }
}

/// The composed Weekly Review read behind `/weekly`. `week_of` is an
/// optional ISO date naming any day in the week to review; omitted, it
/// reviews the current week.
#[tauri::command]
pub async fn get_weekly_bundle(
    state: tauri::State<'_, AppState>,
    week_of: Option<String>,
) -> Result<WeeklyBundle, CmdError> {
    let today = Local::now().date_naive();
    let anchor = resolve_anchor(week_of, today)?;
    with_vault(&state.vault, move |vault| {
        get_weekly_bundle_impl(vault, today, anchor, STUCK_THRESHOLD_DAYS)
    })
    .await?
}

/// Write one section of the week's note (compose/overwrite — each save
/// replaces the section, the review-pass default). `section` is the
/// kebab wire string (`"wins" | "challenges" | "one-improvement" |
/// "this-weeks-goal"`); the domain's tolerant parser also accepts snake
/// and spaced forms, and an unrecognised value is a `CmdError::Invalid`
/// whose message names the valid sections.
///
/// Unlike the project/daily writes, `upsert_weekly_section` has no
/// daily-log side effect (the weekly note is self-contained review
/// content), so only the weekly path — the one the domain hands back —
/// is journalled, and only the `Weekly` area is invalidated.
#[tauri::command]
pub async fn save_weekly_section<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    week_of: Option<String>,
    section: String,
    content: String,
) -> Result<(), CmdError> {
    let today = Local::now().date_naive();
    let anchor = resolve_anchor(week_of, today)?;
    // Parse before crossing the blocking-pool boundary: a bad section
    // name is a fast, cheap Invalid, no vault work needed.
    let parsed = WeeklySection::from_str(&section).map_err(CmdError::Invalid)?;
    let path = with_vault(&state.vault, move |vault| {
        vault.upsert_weekly_section(anchor, parsed, &content, false)
    })
    .await??;
    record_and_emit(&app, &state, vec![path], vec![VaultArea::Weekly]);
    Ok(())
}
