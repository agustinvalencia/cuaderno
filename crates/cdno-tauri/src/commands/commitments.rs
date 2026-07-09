//! Commitments Timeline commands (plan §1.3): the read that feeds the
//! chronological timeline, and the two completion writes it offers.
//!
//! `get_commitments` returns a small view-model that stamps `today`
//! alongside the entries (mirroring `OrientationView`) so the frontend
//! groups past/upcoming without ever computing a domain date itself.
//! The two writes follow the established pattern (`actions.rs`): run
//! the domain call on the blocking pool, journal the touched paths so
//! the watcher suppresses the echo, then emit a precise `vault:changed`.

use chrono::{Local, NaiveDate};

use cdno_core::path::VaultPath;
use cdno_domain::Vault;
use cdno_domain::vault::CommitmentEntry;

use crate::error::CmdError;
use crate::events::VaultArea;
use crate::state::AppState;
use crate::with_vault::with_vault;

use super::actions::{daily_path_for, record_and_emit};

/// Default look-ahead window when the caller passes none — the plan's
/// 90-day timeline default (§1.3).
const DEFAULT_LOOKAHEAD_DAYS: i64 = 90;

/// The Commitments Timeline's data bundle: the aggregated entries plus
/// the date they were computed for. `today` lets the frontend split
/// "slipped past" from "upcoming" and label months without touching a
/// clock — the same discipline as `OrientationView.today`.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct CommitmentsView {
    /// The date the aggregation was computed for (stamped in Rust).
    pub today: NaiveDate,
    pub entries: Vec<CommitmentEntry>,
}

/// Compose the timeline as of `today`. Public and synchronous — the
/// test seam, exercised directly over the Memory doubles.
pub fn get_commitments_impl(
    vault: &Vault,
    today: NaiveDate,
    lookahead_days: i64,
) -> Result<CommitmentsView, CmdError> {
    let entries = vault.commitments(today, lookahead_days)?;
    Ok(CommitmentsView { today, entries })
}

/// Every dated commitment from `today` through `today + lookahead_days`
/// (90 by default), aggregated across all four sources and sorted
/// chronologically. The read behind the Commitments Timeline.
#[tauri::command]
pub async fn get_commitments(
    state: tauri::State<'_, AppState>,
    lookahead_days: Option<i64>,
) -> Result<CommitmentsView, CmdError> {
    let today = Local::now().date_naive();
    let lookahead = lookahead_days.unwrap_or(DEFAULT_LOOKAHEAD_DAYS);
    with_vault(&state.vault(), move |vault| {
        get_commitments_impl(vault, today, lookahead)
    })
    .await?
}

/// The vault-relative path of the *active* standalone commitment
/// `slug` — `commitments/<slug>.md`. Reconstructed from the slug the
/// command received so its deletion can be journalled without the
/// domain having to hand it back.
fn active_commitment_path(slug: &str) -> Result<VaultPath, CmdError> {
    VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::COMMITMENTS))
        .map_err(|e| CmdError::Invalid(e.to_string()))
}

/// Complete a standalone commitment: the domain moves the note to
/// `commitments/_done/<year>/<slug>.md` and logs the completion to
/// today's daily, all in one transaction.
#[tauri::command]
pub async fn complete_commitment<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    slug: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    // Journal the daily for the SAME instant the domain call received,
    // not a fresh now afterwards, so a completion straddling midnight
    // records the day it wrote to (the midnight TOCTOU — see
    // actions.rs::complete_action).
    let date = now.date();
    let slug_for_call = slug.clone();
    let done_path = with_vault(&state.vault(), move |vault| {
        vault.complete_commitment(now, &slug_for_call)
    })
    .await??;
    // The transaction touched three files: the new `_done/<year>/`
    // destination (returned above), the deletion of the active
    // `commitments/<slug>.md` (deterministic from the slug argument, so
    // reconstructed here), and the daily-log line the domain staged.
    // All three are ours to journal — unlike complete_action's archived
    // action note (#315), every path is reachable from the inputs.
    let active_path = active_commitment_path(&slug)?;
    let daily = daily_path_for(date);
    record_and_emit(
        &app,
        &state,
        vec![done_path, active_path, daily],
        vec![VaultArea::Commitments, VaultArea::Daily],
    );
    Ok(())
}

/// Tick an open milestone (`- [ ] <title> — hard: <date>`) on
/// `project` to done. The domain rewrites the bullet in-place and logs
/// the completion to today's daily in one transaction. An ambiguous
/// `milestone` query comes back as `CmdError::Ambiguous`; the UI
/// currently surfaces the candidates in a toast (a picker is a later
/// milestone).
#[tauri::command]
pub async fn complete_milestone<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    project: String,
    milestone: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let date = now.date();
    let project_path = with_vault(&state.vault(), move |vault| {
        vault.complete_milestone(now, &project, &milestone)
    })
    .await??;
    // complete_milestone stages the daily log line in the same
    // transaction, so both the project map and the daily are ours to
    // journal. Completing a milestone drops it from the timeline, so
    // Commitments invalidates too.
    let daily = daily_path_for(date);
    record_and_emit(
        &app,
        &state,
        vec![project_path, daily],
        vec![
            VaultArea::Projects,
            VaultArea::Commitments,
            VaultArea::Daily,
        ],
    );
    Ok(())
}
