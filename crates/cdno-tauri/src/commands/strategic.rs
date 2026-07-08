//! Strategic / Monthly view (M9, plan §1.5; #57): the one composed
//! read behind `/strategic`. Every panel of the monthly review — the
//! questions grid, the portfolio-health table, the 5-slot project
//! allocator, the stewardship overview with habit sparklines, and the
//! six-week commitments timeline — is painted from a single invoke, so
//! the frontend never fans out a handful of reads for one page.
//!
//! The allocator's park / activate writes are not here: they reuse the
//! existing `park_project` / `activate_project` commands (projects.rs,
//! M5), whose `ProjectCapReached` error already carries the active
//! slugs the "room for five" modal lists. This module is a pure read.
//!
//! Date maths stays in Rust (plan §3.7): the habit sparklines are
//! **entries-per-ISO-week counts computed here**, handed to the
//! frontend as a plain `Vec<u32>` it only has to draw — the webview
//! never buckets dates itself.

use chrono::{Datelike, Duration, Local, NaiveDate};

use cdno_core::path::VaultPath;
use cdno_domain::vault::{
    CommitmentEntry, PortfolioSummary, QuestionSummary, StewardshipSummary, StewardshipVariant,
};
use cdno_domain::{Context, Vault};

use crate::error::CmdError;
use crate::state::AppState;
use crate::with_vault::with_vault;

/// How many ISO weeks of habit history each stewardship sparkline
/// spans (plan §1.5: "entries/week"). Twelve weeks is a quarter — long
/// enough to read a rhythm, short enough to stay a glanceable spark.
const SPARKLINE_WEEKS: usize = 12;

/// The forward window of the strategic view's commitments timeline: the
/// next six weeks (plan §1.5, "6-week commitments timeline"). The
/// `commitments` query folds in its own overdue look-back on top, so
/// the result is not purely forward-looking — the same shared timeline
/// component the daily and weekly surfaces use renders it.
const COMMITMENTS_LOOKAHEAD_DAYS: i64 = 42;

/// The composed Strategic / Monthly data (plan §1.5). One read, every
/// panel: the resolved "today" (stamped in Rust, for the timeline and
/// staleness lines), the active questions to grid by domain, the
/// portfolio-health rows, the active / parked project slots plus the
/// configured cap the allocator lays out against, the stewardship
/// overview with a precomputed habit sparkline each, and the six-week
/// commitments lookahead.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct StrategicBundle {
    /// The real current date, stamped in Rust (plan §3.7) — the shared
    /// timeline labels months and splits past/upcoming against it, and
    /// the portfolio-health tiers read their staleness from it.
    pub today: NaiveDate,
    /// Every `status: active` question, sorted `(domain, slug)`. The
    /// frontend groups by domain (research / life) for the grid — the
    /// domain rides along on each row so no second query is needed.
    pub questions: Vec<QuestionSummary>,
    /// Per-question evidence dossiers with their counts and staleness,
    /// for the portfolio-health table. Reused verbatim from the M8
    /// browser so the two surfaces can't disagree on the numbers.
    pub portfolios: Vec<PortfolioSummary>,
    /// The active project slots the allocator fills, in index order.
    /// `active.len()` is at most `max_active`; the frontend renders the
    /// remaining `max_active - active.len()` slots as soft dashed "open
    /// slot" placeholders (breathing room, not vacancy).
    pub active: Vec<ProjectSlot>,
    /// The parked shelf beneath the slots — each activatable back into
    /// an open slot (or, at the cap, via the "park one to make space"
    /// modal).
    pub parked: Vec<ProjectSlot>,
    /// The configured active-project cap (`max_active_projects`,
    /// default 5) — the number of slots the allocator draws, read
    /// straight from the vault config rather than hardcoded, so a vault
    /// that raised or lowered the cap lays out the right count.
    pub max_active: usize,
    /// The stewardship overview rows: each carries its summary (context
    /// dot, name, staleness) and a precomputed 12-week habit sparkline.
    pub stewardships: Vec<StewardshipStrategicRow>,
    /// Dated commitments for the six-week timeline: the next 42 days
    /// forward, plus anything overdue within the domain's look-back
    /// (the `commitments` query folds both together). Fed to the shared
    /// `CommitmentsTimeline` in read-only mode.
    pub commitments: Vec<CommitmentEntry>,
}

/// One project in the allocator or on the parked shelf — a deliberately
/// thin view over the project frontmatter. The slug identifies the note
/// (and routes to `/projects/:slug`); the context drives the colour
/// dot. The project's display name is its map H1, not a frontmatter
/// field, so it isn't carried here — the slug reads well enough for the
/// allocator's compact chips, and skipping the H1 keeps the bundle to
/// one read per project rather than two.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectSlot {
    pub slug: String,
    pub context: Context,
}

/// One row of the stewardship overview: the summary the list surface
/// already computes, paired with a precomputed habit sparkline.
///
/// The sparkline is a `Vec<u32>` of tracking-entry counts, one per ISO
/// week over the last [`SPARKLINE_WEEKS`], oldest week first and the
/// current week last. Empty for a flat stewardship (no `tracking/`
/// subdir to count) — the frontend draws the spark only when it's
/// non-empty, mirroring the detail view's "charts pane only when
/// there's data" rule.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct StewardshipStrategicRow {
    pub summary: StewardshipSummary,
    pub sparkline: Vec<u32>,
}

/// Compose the Strategic bundle as of `today`. Public and synchronous —
/// the test seam, exercised directly over the Memory doubles.
///
/// Every field is a straight domain read except the sparklines, which
/// bucket each expanded stewardship's recent tracking dates into
/// per-ISO-week counts here (plan §3.7 keeps date maths off the
/// frontend). Flat stewardships get an empty sparkline without a
/// tracking scan — they have no `tracking/` subdir, so the scan would
/// return empty anyway.
pub fn get_strategic_bundle_impl(
    vault: &Vault,
    today: NaiveDate,
) -> Result<StrategicBundle, CmdError> {
    let questions = vault.active_questions()?;
    let portfolios = vault.list_portfolios(today)?;

    let active = vault
        .active_projects()?
        .into_iter()
        .map(|(path, fm)| ProjectSlot {
            slug: slug_of(&path),
            context: fm.context,
        })
        .collect();
    let parked = vault
        .parked_projects()?
        .into_iter()
        .map(|(path, fm)| ProjectSlot {
            slug: slug_of(&path),
            context: fm.context,
        })
        .collect();

    // The cap the allocator lays out against — read from the vault
    // config the same way the domain's park/activate check does
    // (`config.vault.max_active_projects`, lifecycle.rs), never a
    // hardcoded 5. A vault that changed the cap gets the right slots.
    let max_active = vault.config().vault.max_active_projects as usize;

    // Only look back far enough to fill the sparkline window: the Monday
    // SPARKLINE_WEEKS-1 weeks before this week's Monday, inclusive.
    let window_start = monday_of(today) - Duration::days(7 * (SPARKLINE_WEEKS as i64 - 1));
    let mut stewardships = Vec::new();
    for summary in vault.list_stewardships(today)? {
        let sparkline = match summary.variant {
            // Expanded stewardships have a tracking/ subdir; bucket the
            // window's entries into per-week counts.
            StewardshipVariant::Expanded => {
                let dates: Vec<NaiveDate> = vault
                    .list_tracking(&summary.slug, None, window_start, today)?
                    .into_iter()
                    .map(|entry| entry.date)
                    .collect();
                entries_per_week(&dates, today, SPARKLINE_WEEKS)
            }
            // Flat stewardships track nothing — no spark to draw.
            StewardshipVariant::Flat => Vec::new(),
        };
        stewardships.push(StewardshipStrategicRow { summary, sparkline });
    }

    let commitments = vault.commitments(today, COMMITMENTS_LOOKAHEAD_DAYS)?;

    Ok(StrategicBundle {
        today,
        questions,
        portfolios,
        active,
        parked,
        max_active,
        stewardships,
        commitments,
    })
}

/// Bucket tracking-note `dates` into per-ISO-week counts over the last
/// `weeks` weeks ending in the week containing `today`. Index 0 is the
/// oldest week in the window; index `weeks - 1` is the current week —
/// so the frontend draws left-to-right as time moving forward.
///
/// A date older than the window, or dated in the future (a typo), falls
/// outside `0..weeks` weeks-ago and is dropped rather than clamped into
/// an edge bucket — a stray future entry must not inflate this week's
/// count. Public (`#[doc(hidden)]`) so the unit test can exercise the
/// bucketing edges directly.
#[doc(hidden)]
pub fn entries_per_week(dates: &[NaiveDate], today: NaiveDate, weeks: usize) -> Vec<u32> {
    let this_monday = monday_of(today);
    let mut counts = vec![0u32; weeks];
    for &date in dates {
        // Whole weeks between the entry's Monday and this week's Monday.
        // Using each date's Monday (not a raw day delta) keeps every day
        // of a given ISO week in the same bucket regardless of weekday.
        let weeks_ago = (this_monday - monday_of(date)).num_days() / 7;
        if (0..weeks as i64).contains(&weeks_ago) {
            // weeks_ago 0 (this week) → last bucket; the oldest in-window
            // week → bucket 0.
            let index = weeks - 1 - weeks_ago as usize;
            counts[index] += 1;
        }
    }
    counts
}

/// The Monday of the ISO week containing `date` (locale-independent,
/// matching the domain's own week keying). Mirrors `weekly.rs`'s helper;
/// kept trivially in sync via the ISO weekday arithmetic.
fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

/// The slug (file stem) of a project's map path. Mirrors `weekly.rs`.
fn slug_of(path: &VaultPath) -> String {
    path.as_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned()
}

/// The composed Strategic / Monthly read behind `/strategic`. Pure
/// read: no journal, no events (the allocator's writes reuse the M5
/// `park_project` / `activate_project` commands).
#[tauri::command]
pub async fn get_strategic_bundle(
    state: tauri::State<'_, AppState>,
) -> Result<StrategicBundle, CmdError> {
    let today = Local::now().date_naive();
    with_vault(&state.vault, move |vault| {
        get_strategic_bundle_impl(vault, today)
    })
    .await?
}
