//! Project commands.
//!
//! M2 shipped `update_project_state` (the Home card's inline Current
//! State editor). M5 adds the Project Detail view (plan §1.8): the
//! composed `get_project` read plus the lifecycle and Waiting-On
//! writes the detail page drives.

use chrono::{Duration, Local, NaiveDate, NaiveTime};

use cdno_core::path::VaultPath;
use cdno_domain::error::DomainError;
use cdno_domain::vault::ActionListEntry;
use cdno_domain::{Context, ProjectStatus, Vault};

use crate::error::CmdError;
use crate::events::VaultArea;
use crate::state::AppState;
use crate::with_vault::with_vault;

use super::actions::{daily_path_for, record_and_emit, record_outcome_and_emit};

/// How far back Project Detail looks for daily-log mentions of the
/// project. Matches the MCP `get_project_context` window (30 days) so
/// the two surfaces show the same "recently in your logs" slice.
const LOG_MENTION_WINDOW_DAYS: i64 = 30;

/// The Project Detail view-model (plan §1.8). Composes the project map
/// (typed frontmatter fields + raw body) with its actions, open
/// milestones, backlinks, and recent log mentions — everything the
/// `/projects/:slug` page renders, in one invoke.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectDetail {
    pub slug: String,
    /// The project's lifecycle status — `parked` renders the page
    /// read-only (no action ticks / adds), so the frontend needs it
    /// even though a parked project still shows its map and history.
    pub status: ProjectStatus,
    pub context: Context,
    pub created: NaiveDate,
    /// The raw `core_question` wikilink string, when the project sets
    /// one (`"[[questions/research/foo]]"`). Resolution is the
    /// frontend's job via `resolve_wikilink`.
    pub core_question: Option<String>,
    /// The project map's markdown body (below the frontmatter), for
    /// the Links section and any prose the structured fields don't
    /// capture.
    pub body_markdown: String,
    /// Open action bullets. Empty for a parked project — `list_actions`
    /// refuses parked projects, and the detail page renders them
    /// read-only anyway (see `get_project_impl`).
    pub actions: Vec<ActionListEntry>,
    pub open_milestones: Vec<MilestoneView>,
    pub backlinks: BacklinksView,
    pub log_mentions: Vec<LogMentionView>,
}

/// One open milestone, flattened from the core `MilestoneEntry` (which
/// lives in `cdno-core` and can't carry ts-rs derives) into a
/// wire-ready shape.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct MilestoneView {
    pub name: String,
    /// ISO `YYYY-MM-DD`, or `None` for a non-date marker (`target: Q3`).
    pub date: Option<String>,
    /// `true` for a `hard:` deadline, `false` for a soft target.
    pub is_hard: bool,
}

/// Backlinks to the project, grouped by source note type, as path
/// strings ready for the frontend to render and route on. Mirrors the
/// domain `ProjectBacklinks` grouping (which holds `VaultPath`s and
/// carries no ts-rs derives).
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BacklinksView {
    pub portfolios: Vec<String>,
    pub questions: Vec<String>,
    pub evidence: Vec<String>,
    pub actions: Vec<String>,
    pub other: Vec<String>,
}

/// One daily-log line mentioning the project, for the "recently in
/// your logs" strip. Flattened from the domain `DailyLogLine`.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogMentionView {
    pub date: NaiveDate,
    /// The log line's timestamp (`HH:MM:SS`).
    pub time: NaiveTime,
    pub text: String,
}

/// Compose the Project Detail bundle as of `today`. Public and
/// synchronous — the test seam, exercised directly over the Memory
/// doubles.
///
/// Parked handling: `get_project_full` resolves the slug against both
/// `projects/` and `projects/_parked/`, so a parked project loads. Its
/// `## Next Actions` are *not* listed (`list_actions` refuses parked
/// projects — queued work on a parked project isn't actionable), so
/// `actions` comes back empty and `status` tells the frontend to render
/// the page read-only. Milestones, backlinks, and log mentions read the
/// same for parked and active projects.
pub fn get_project_impl(
    vault: &Vault,
    slug: &str,
    today: NaiveDate,
) -> Result<ProjectDetail, CmdError> {
    let (fm, body) = vault.get_project_full(slug)?;

    // Only active projects expose their action list; parked ones fold
    // to an empty list rather than erroring, so the page stays a
    // read-only history view. The frontmatter `status` is the primary
    // gate.
    //
    // Drift case: the map's frontmatter can say `active` while the file
    // actually sits under `_parked/` (a torn park/activate that only got
    // half-committed, or a hand-edited status). `list_actions` reads the
    // file's real location and refuses a parked map with
    // `ProjectNotActive` — so we catch that here and also fold to empty.
    // An inconsistent vault must degrade to a read-only page, never fail
    // the whole detail load.
    let actions = if fm.status == ProjectStatus::Active {
        match vault.list_actions(slug) {
            Ok(actions) => actions,
            Err(DomainError::ProjectNotActive(_)) => Vec::new(),
            Err(e) => return Err(e.into()),
        }
    } else {
        Vec::new()
    };

    let open_milestones = vault
        .open_milestones(slug)?
        .into_iter()
        .map(|m| MilestoneView {
            name: m.name,
            date: m.date,
            is_hard: m.is_hard,
        })
        .collect();

    let backlinks = vault.project_backlinks(slug)?;
    let backlinks = BacklinksView {
        portfolios: to_strings(&backlinks.portfolios),
        questions: to_strings(&backlinks.questions),
        evidence: to_strings(&backlinks.evidence),
        actions: to_strings(&backlinks.actions),
        other: to_strings(&backlinks.other),
    };

    let since = today - Duration::days(LOG_MENTION_WINDOW_DAYS);
    let log_mentions = vault
        .daily_log_mentions(slug, since)?
        .into_iter()
        .map(|l| LogMentionView {
            date: l.date,
            time: l.time,
            text: l.text,
        })
        .collect();

    Ok(ProjectDetail {
        slug: slug.to_owned(),
        status: fm.status,
        context: fm.context,
        created: fm.created,
        core_question: fm.core_question,
        body_markdown: body,
        actions,
        open_milestones,
        backlinks,
        log_mentions,
    })
}

fn to_strings(paths: &[VaultPath]) -> Vec<String> {
    paths.iter().map(|p| p.to_string()).collect()
}

/// The active-project note path for `slug` — `projects/<slug>.md`.
/// Deterministic from the slug, so lifecycle writes can journal the
/// path the domain doesn't hand back.
fn active_project_path(slug: &str) -> Result<VaultPath, CmdError> {
    VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS))
        .map_err(|e| CmdError::Invalid(e.to_string()))
}

/// The parked-project note path for `slug` — `projects/_parked/<slug>.md`.
fn parked_project_path(slug: &str) -> Result<VaultPath, CmdError> {
    VaultPath::new(format!("{}/{slug}.md", cdno_core::paths::PROJECTS_PARKED))
        .map_err(|e| CmdError::Invalid(e.to_string()))
}

/// The composed Project Detail read behind `/projects/:slug`.
#[tauri::command]
pub async fn get_project(
    state: tauri::State<'_, AppState>,
    slug: String,
) -> Result<ProjectDetail, CmdError> {
    let today = Local::now().date_naive();
    with_vault(&state.vault, move |vault| {
        get_project_impl(vault, &slug, today)
    })
    .await?
}

/// Rewrite a project's `## Current State`. The domain auto-logs the
/// previous state to today's daily entry in the same transaction —
/// free history — and no-ops silently when the text is unchanged.
#[tauri::command]
pub async fn update_project_state<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    project: String,
    new_state: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let outcome = with_vault(&state.vault, move |vault| {
        vault.update_project_state(now, &project, &new_state)
    })
    .await??;
    // `record_outcome_and_emit` journals and emits only when the domain
    // actually wrote (a no-op on unchanged text touches nothing), so an
    // unchanged-state call plants no false echo-suppression entry, and
    // the daily path we journal is the one the domain wrote — never a
    // client-side reconstruction that could drift across midnight (#315).
    record_outcome_and_emit(
        &app,
        &state,
        &outcome,
        vec![VaultArea::Projects, VaultArea::Daily],
    );
    Ok(())
}

/// Add a Waiting-On blocker via the Project Detail quick-row (plan
/// §1.8): "I'm now blocked on X". The domain appends the item and logs
/// the addition to today's daily in one transaction.
#[tauri::command]
pub async fn add_waiting_on<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    project: String,
    item: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let date = now.date();
    let project_path = with_vault(&state.vault, move |vault| {
        vault.add_waiting_on(now, &project, &item)
    })
    .await??;
    // add_waiting_on stages the daily-log line in the same transaction,
    // so both the project map (returned) and the daily are ours to
    // journal.
    let daily = daily_path_for(date);
    record_and_emit(
        &app,
        &state,
        vec![project_path, daily],
        vec![VaultArea::Projects, VaultArea::Daily],
    );
    Ok(())
}

/// Resolve (remove) a Waiting-On blocker matching `query`
/// (case-insensitive substring; ambiguity → `CmdError::Ambiguous`, the
/// UI shows a picker). The domain rewrites the section and logs the
/// resolution to today's daily.
#[tauri::command]
pub async fn resolve_waiting<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    project: String,
    query: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let date = now.date();
    let project_path = with_vault(&state.vault, move |vault| {
        vault.resolve_waiting_on(now, &project, &query)
    })
    .await??;
    let daily = daily_path_for(date);
    record_and_emit(
        &app,
        &state,
        vec![project_path, daily],
        vec![VaultArea::Projects, VaultArea::Daily],
    );
    Ok(())
}

/// Park an active project (Project Detail header action): the domain
/// moves the map to `projects/_parked/<slug>.md`, flips its `status`,
/// and logs the parking to today's daily in one transaction.
#[tauri::command]
pub async fn park_project<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    slug: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let date = now.date();
    let slug_for_call = slug.clone();
    let parked_path = with_vault(&state.vault, move |vault| {
        vault.park_project(now, &slug_for_call)
    })
    .await??;
    // The transaction touched three files: the new parked destination
    // (returned above), the deletion of the active `projects/<slug>.md`
    // (deterministic from the slug, reconstructed here), and the
    // daily-log line the domain staged. All three are ours to journal.
    let active_path = active_project_path(&slug)?;
    let daily = daily_path_for(date);
    record_and_emit(
        &app,
        &state,
        vec![parked_path, active_path, daily],
        vec![VaultArea::Projects, VaultArea::Daily],
    );
    Ok(())
}

/// Activate a parked project (Strategic allocator / Project Detail):
/// the domain moves it back to `projects/<slug>.md` and flips its
/// `status`. At the active-project cap this fails with
/// `ProjectCapReached` — the structured `CmdError` the allocator's
/// "park one to make space" modal keys on.
#[tauri::command]
pub async fn activate_project<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    slug: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let date = now.date();
    let slug_for_call = slug.clone();
    let active_path = with_vault(&state.vault, move |vault| {
        vault.activate_project(now, &slug_for_call)
    })
    .await??;
    // Mirror of park_project: the new active destination (returned),
    // the deletion of the parked `projects/_parked/<slug>.md`
    // (deterministic, reconstructed), and the staged daily-log line.
    let parked_path = parked_project_path(&slug)?;
    let daily = daily_path_for(date);
    record_and_emit(
        &app,
        &state,
        vec![active_path, parked_path, daily],
        vec![VaultArea::Projects, VaultArea::Daily],
    );
    Ok(())
}
