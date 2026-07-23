//! `get_orientation` / `get_today` — the Home view's data.

use chrono::{Local, NaiveDate};

use cdno_domain::Context;
use cdno_domain::Vault;
use cdno_domain::vault::{ActionListEntry, CommitmentEntry, LapsedHabit, ProjectSummary};

use crate::error::CmdError;
use crate::state::AppState;
use crate::with_vault::with_vault;

/// The Home view's bundle (plan §1.1): `orientation_context` plus,
/// per project, the life context (for the colour dot) and *every*
/// open action bullet — the energy selector filters client-side, so
/// one invoke carries the whole morning.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct OrientationView {
    /// The date the snapshot was computed for (stamped in Rust — the
    /// frontend never computes domain dates).
    pub today: NaiveDate,
    pub commitments: Vec<CommitmentEntry>,
    pub projects: Vec<OrientationProject>,
    pub lapsed_habits: Vec<LapsedHabit>,
    /// The configured active-project cap (`max_active_projects`, default
    /// 5), read from the vault config rather than hardcoded.
    ///
    /// The five-slot rule is load-bearing in the method — it is the whole
    /// reason a sixth project has to displace one — so the shell's project
    /// list says "3 of 5 slots" rather than leaving the cap to be
    /// discovered by hitting it. The strategic bundle already carried this
    /// for its allocator; the sidebar needs it on every page.
    pub max_active: usize,
}

#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct OrientationProject {
    #[serde(flatten)]
    pub summary: ProjectSummary,
    /// All open bullets from `## Next Actions`, for the energy
    /// filter's no-match rule (a card never blanks).
    pub actions: Vec<ActionListEntry>,
}

/// Compose the orientation bundle as of `today`. Public and
/// synchronous — the test seam.
pub fn get_orientation_impl(vault: &Vault, today: NaiveDate) -> Result<OrientationView, CmdError> {
    let ctx = vault.orientation_context(today)?;

    let mut projects = Vec::with_capacity(ctx.projects.len());
    for summary in ctx.projects {
        let actions = vault.list_actions(&summary.slug)?;
        projects.push(OrientationProject { actions, summary });
    }

    Ok(OrientationView {
        today,
        commitments: ctx.commitments,
        projects,
        lapsed_habits: ctx.lapsed_habits,
        max_active: vault.config().vault.max_active_projects as usize,
    })
}

#[tauri::command]
pub async fn get_orientation(
    state: tauri::State<'_, AppState>,
) -> Result<OrientationView, CmdError> {
    let today = Local::now().date_naive();
    with_vault(&state.vault(), move |vault| {
        get_orientation_impl(vault, today)
    })
    .await?
}

/// What the most recent reconciliation left out of the index (#440).
///
/// Reads the recorded counts rather than re-reconciling — the notice is
/// about the pass that built the index the app is currently running on.
/// Every reconcile replaces them: the startup pass, a config rebuild, and
/// each of the watcher's.
#[tauri::command]
pub fn get_index_exclusions(state: tauri::State<'_, AppState>) -> crate::events::IndexExclusions {
    **state.exclusions.load()
}

/// What the user is in the middle of, for the Today page's Now band
/// (#442).
///
/// Reconstructed from today's daily log — starting an action writes a
/// line, completing it writes another — so it needs no state of its own
/// and picks up a start made from the CLI or by an agent over MCP, not
/// only one clicked in the app.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct NowView {
    /// The project slug the action belongs to; routes to its map.
    pub project: String,
    /// Life context of that project, for the colour dot. Absent if the
    /// project has since been renamed or removed — the band still shows
    /// what the log says rather than dropping the entry.
    pub context: Option<Context>,
    /// The action text as logged.
    pub action: String,
    /// `HH:MM` the action was started, for the "since" label. The elapsed
    /// time is computed in the frontend so it can tick without a refetch.
    pub started: String,
}

/// Compose the Now band's data as of `today`. Public and synchronous —
/// the test seam.
pub fn get_now_impl(vault: &Vault, today: NaiveDate) -> Result<Option<NowView>, CmdError> {
    let Some(focus) = vault.current_focus(today)? else {
        return Ok(None);
    };
    // The project may have been parked or renamed since the log line was
    // written. That is not a reason to hide what the log says, so the
    // context is best-effort.
    let context = vault
        .project_summary(&focus.project)
        .ok()
        .map(|summary| summary.context);
    Ok(Some(NowView {
        project: focus.project,
        context,
        action: focus.action,
        started: focus.started.format("%H:%M").to_string(),
    }))
}

#[tauri::command]
pub async fn get_now(state: tauri::State<'_, AppState>) -> Result<Option<NowView>, CmdError> {
    let today = Local::now().date_naive();
    with_vault(&state.vault(), move |vault| get_now_impl(vault, today)).await?
}

/// Today's date for display only (headers, date labels). Domain
/// calls never take a frontend-computed date.
#[tauri::command]
pub fn get_today() -> String {
    Local::now().date_naive().format("%Y-%m-%d").to_string()
}
