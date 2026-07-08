//! Calendar-view reads (#340): the month grid's note-bearing-day marks
//! and the embedded daily/weekly/monthly panel behind `/calendar`.
//!
//! Every date the panel navigates to is stamped in Rust and handed to
//! the frontend, so the webview never does domain date arithmetic (plan
//! §3.7). `read_daily` returns the day's note *plus* the identities the
//! quick-nav needs — the previous and next day, the Monday of the day's
//! week, and its calendar month — so prev/next/week/month are follow-up
//! reads keyed on values the backend computed, not client-side maths.
//! `read_weekly` and `read_monthly` are the raw note reads the week and
//! month jumps land on (the weekly one is deliberately distinct from the
//! composed `get_weekly_bundle` that backs the guided review). All four
//! are pure reads — no journal, no events.

use chrono::{Datelike, Duration, NaiveDate};

use cdno_domain::Vault;

use crate::error::CmdError;
use crate::state::AppState;
use crate::with_vault::with_vault;

/// A daily note plus the neighbour identities the calendar panel's
/// quick-nav needs, so the frontend never computes a domain date for a
/// read (plan §3.7). Absence is a normal answer (`exists: false`, empty
/// `markdown`) the panel renders as a calm empty state, not an error.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct DailyView {
    /// The note's own date (`YYYY-MM-DD`), echoed back so the panel can
    /// label itself without re-parsing the request.
    pub date: NaiveDate,
    /// Whether a note file exists for the day — the panel branches on
    /// this to show the note or the empty state.
    pub exists: bool,
    /// The daily note's markdown (empty when `exists` is false).
    pub markdown: String,
    /// The note's vault-relative path — always resolved, even when the
    /// note doesn't exist yet, so "Open in editor" works from the empty
    /// state too.
    pub path: String,
    /// The day before `date` — the prev-day quick-nav target.
    pub prev_date: NaiveDate,
    /// The day after `date` — the next-day quick-nav target.
    pub next_date: NaiveDate,
    /// The Monday of `date`'s ISO week — the week quick-nav target,
    /// echoed straight into `read_weekly`.
    pub week_of: NaiveDate,
    /// `date`'s calendar month as `YYYY-MM` — the month quick-nav target,
    /// echoed straight into `read_monthly`.
    pub month: String,
}

/// A weekly note's raw content for the calendar panel's week jump.
/// Distinct from the composed `WeeklyBundle` (which stitches the guided
/// review): this is the note itself, absence-tolerant like `DailyView`.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct WeeklyView {
    /// The Monday of the ISO week the note covers, echoed back.
    pub week_of: NaiveDate,
    pub exists: bool,
    pub markdown: String,
    /// The note's vault-relative path (resolved even when absent, for
    /// "Open in editor").
    pub path: String,
}

/// A monthly note's raw content for the calendar panel's month jump
/// (#228). Absence-tolerant like `DailyView`.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct MonthlyView {
    /// The month the note covers, as `YYYY-MM`, echoed back.
    pub month: String,
    pub exists: bool,
    pub markdown: String,
    /// The note's vault-relative path (resolved even when absent, for
    /// "Open in editor").
    pub path: String,
}

/// The Monday of the ISO week containing `date` (locale-independent,
/// matching the domain's own week keying). Recomputed here because the
/// domain's helper is private; kept trivially correct via the ISO
/// weekday arithmetic (offset is 0 on a Monday).
fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

/// Parse an `YYYY-MM-DD` wire string into a date, mapping a malformed
/// value to a user-visible `Invalid` rather than a silent fallback.
fn parse_date(s: &str) -> Result<NaiveDate, CmdError> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .map_err(|e| CmdError::Invalid(format!("date must be an ISO date (YYYY-MM-DD): {e}")))
}

/// Parse an `YYYY-MM` wire string into the first day of that month, so
/// the domain's month-keyed read can resolve it. A malformed value is a
/// user-visible `Invalid`.
fn parse_month(s: &str) -> Result<NaiveDate, CmdError> {
    NaiveDate::parse_from_str(&format!("{}-01", s.trim()), "%Y-%m-%d")
        .map_err(|e| CmdError::Invalid(format!("month must be YYYY-MM: {e}")))
}

/// Compose the daily view for `date`: the note (or its absence) plus the
/// neighbour identities. Public and synchronous — the test seam,
/// exercised directly over the Memory doubles.
pub fn read_daily_impl(vault: &Vault, date: NaiveDate) -> Result<DailyView, CmdError> {
    let note = vault.read_daily_note(date)?;
    Ok(DailyView {
        date,
        exists: note.exists,
        markdown: note.markdown,
        path: note.path.to_string(),
        prev_date: date - Duration::days(1),
        next_date: date + Duration::days(1),
        week_of: monday_of(date),
        month: date.format("%Y-%m").to_string(),
    })
}

/// Compose the weekly view for the ISO week containing `week_of` (any day
/// in the week resolves to the same note). Public and synchronous — the
/// test seam.
pub fn read_weekly_impl(vault: &Vault, week_of: NaiveDate) -> Result<WeeklyView, CmdError> {
    let note = vault.read_weekly_note(week_of)?;
    Ok(WeeklyView {
        week_of: monday_of(week_of),
        exists: note.exists,
        markdown: note.markdown,
        path: note.path.to_string(),
    })
}

/// Compose the monthly view for the calendar month containing `month_of`
/// (any day in the month resolves to the same note). Public and
/// synchronous — the test seam.
pub fn read_monthly_impl(vault: &Vault, month_of: NaiveDate) -> Result<MonthlyView, CmdError> {
    let note = vault.read_monthly_note(month_of)?;
    Ok(MonthlyView {
        month: month_of.format("%Y-%m").to_string(),
        exists: note.exists,
        markdown: note.markdown,
        path: note.path.to_string(),
    })
}

/// The dates in `year`/`month` that already have a daily note. Public and
/// synchronous — the test seam. Validates the month here (the domain
/// method treats an out-of-range month as matching nothing) so a bad
/// argument is a clear `Invalid` at the boundary.
pub fn list_daily_dates_impl(
    vault: &Vault,
    year: i32,
    month: u32,
) -> Result<Vec<NaiveDate>, CmdError> {
    if !(1..=12).contains(&month) {
        return Err(CmdError::Invalid(format!(
            "month must be 1..=12, got {month}"
        )));
    }
    Ok(vault.daily_dates_in_month(year, month)?)
}

/// Read the daily note for `date` (an `YYYY-MM-DD` string) plus its
/// neighbour identities, for the calendar panel.
#[tauri::command]
pub async fn read_daily(
    state: tauri::State<'_, AppState>,
    date: String,
) -> Result<DailyView, CmdError> {
    let date = parse_date(&date)?;
    with_vault(&state.vault, move |vault| read_daily_impl(vault, date)).await?
}

/// Read the weekly note covering `week_of` (an `YYYY-MM-DD` string naming
/// any day in the week), for the calendar panel's week jump.
#[tauri::command]
pub async fn read_weekly(
    state: tauri::State<'_, AppState>,
    week_of: String,
) -> Result<WeeklyView, CmdError> {
    let week_of = parse_date(&week_of)?;
    with_vault(&state.vault, move |vault| read_weekly_impl(vault, week_of)).await?
}

/// Read the monthly note covering `month` (a `YYYY-MM` string), for the
/// calendar panel's month jump.
#[tauri::command]
pub async fn read_monthly(
    state: tauri::State<'_, AppState>,
    month: String,
) -> Result<MonthlyView, CmdError> {
    let month_of = parse_month(&month)?;
    with_vault(&state.vault, move |vault| {
        read_monthly_impl(vault, month_of)
    })
    .await?
}

/// The dates in `year`/`month` that already have a daily note, so the
/// calendar grid can mark them. Rides the wire as an array of ISO date
/// strings.
#[tauri::command]
pub async fn list_daily_dates(
    state: tauri::State<'_, AppState>,
    year: i32,
    month: u32,
) -> Result<Vec<NaiveDate>, CmdError> {
    with_vault(&state.vault, move |vault| {
        list_daily_dates_impl(vault, year, month)
    })
    .await?
}
