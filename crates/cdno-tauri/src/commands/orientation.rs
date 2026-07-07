//! `get_orientation` / `get_today` — the Home view's data.

use chrono::{Local, NaiveDate};

use cdno_domain::vault::{ActionListEntry, CommitmentEntry, LapsedHabit, ProjectSummary};
use cdno_domain::{Context, Vault};

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
}

#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct OrientationProject {
    #[serde(flatten)]
    pub summary: ProjectSummary,
    pub context: Context,
    /// All open bullets from `## Next Actions`, for the energy
    /// filter's no-match rule (a card never blanks).
    pub actions: Vec<ActionListEntry>,
}

/// Compose the orientation bundle as of `today`. Public and
/// synchronous — the test seam.
pub fn get_orientation_impl(vault: &Vault, today: NaiveDate) -> Result<OrientationView, CmdError> {
    let ctx = vault.orientation_context(today)?;

    // Context per slug from the typed frontmatter. `active_projects`
    // re-reads the maps the summaries came from — index-backed and
    // personal-scale, not worth a domain-API change to dedupe.
    let mut contexts = std::collections::HashMap::new();
    for (path, fm) in vault.active_projects()? {
        let slug = path
            .as_path()
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        contexts.insert(slug, fm.context);
    }

    let mut projects = Vec::with_capacity(ctx.projects.len());
    for summary in ctx.projects {
        let actions = vault.list_actions(&summary.slug)?;
        let context = contexts
            .get(&summary.slug)
            .copied()
            .unwrap_or(Context::Personal);
        projects.push(OrientationProject {
            context,
            actions,
            summary,
        });
    }

    Ok(OrientationView {
        today,
        commitments: ctx.commitments,
        projects,
        lapsed_habits: ctx.lapsed_habits,
    })
}

#[tauri::command]
pub async fn get_orientation(
    state: tauri::State<'_, AppState>,
) -> Result<OrientationView, CmdError> {
    let today = Local::now().date_naive();
    with_vault(&state.vault, move |vault| {
        get_orientation_impl(vault, today)
    })
    .await?
}

/// Today's date for display only (headers, date labels). Domain
/// calls never take a frontend-computed date.
#[tauri::command]
pub fn get_today() -> String {
    Local::now().date_naive().format("%Y-%m-%d").to_string()
}
