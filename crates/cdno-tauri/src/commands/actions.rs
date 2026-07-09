//! Action write commands: `start_action` and `complete_action`.
//!
//! The write-command pattern every later module copies: run the
//! domain call on the blocking pool, record the touched paths in the
//! `WriteJournal` (so the watcher suppresses the echo), then emit a
//! precise `vault:changed { origin: self }` for the frontend's
//! invalidation map. Recording happens *after* the commit inside the
//! domain call returned — a failed write must not poison the journal.

use std::str::FromStr;

use chrono::{Local, NaiveDate};
use tauri::Emitter;

use cdno_core::path::VaultPath;
use cdno_domain::Context;
use cdno_domain::Vault;
use cdno_domain::frontmatter::EnergyLevel;
use cdno_domain::vault::ActionListEntry;

use crate::error::CmdError;
use crate::events::{Origin, VAULT_CHANGED, VaultArea, VaultChanged};
use crate::state::AppState;
use crate::with_vault::with_vault;

/// Record `paths` as self-writes and emit the matching
/// `origin: self` change event. Shared by every write command that
/// always writes on success.
pub(crate) fn record_and_emit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    paths: Vec<VaultPath>,
    areas: Vec<VaultArea>,
) {
    state.journal.record(paths.iter().cloned());
    emit_self_change(app, &paths, areas);
}

/// Journal a write [`WriteOutcome`] and emit the matching `origin: self`
/// event — but only when the write actually touched something.
///
/// A silent domain no-op (e.g. `update_project_state` with unchanged
/// text) records nothing and emits nothing, so it can't plant a false
/// echo-suppression entry over paths that never changed (#315). The
/// emitted paths are exactly the domain's touched set, not a client-side
/// reconstruction, so archival moves and daily-log notes are covered.
pub(crate) fn record_outcome_and_emit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    outcome: &cdno_domain::WriteOutcome,
    areas: Vec<VaultArea>,
) {
    if state.journal.record_write(outcome) {
        emit_self_change(app, &outcome.paths, areas);
    }
}

/// Emit a precise `origin: self` `vault:changed` for `paths`. Journalling
/// is the caller's responsibility; this is only the frontend notify.
fn emit_self_change<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    paths: &[VaultPath],
    areas: Vec<VaultArea>,
) {
    let path_strings = paths.iter().map(|p| p.to_string()).collect();
    // A dropped invalidation event leaves the frontend showing stale
    // data until the next reload — rare (the channel is in-process) but
    // silent, so surface it in the log rather than swallowing the Err.
    if let Err(err) = app.emit(
        VAULT_CHANGED,
        VaultChanged {
            origin: Origin::SelfWrite,
            areas,
            paths: path_strings,
        },
    ) {
        tracing::warn!(error = %err, "failed to emit vault:changed after a write");
    }
}

/// Log that work on `action` is starting (`started [[slug]] — …` in
/// today's daily note). The Home view's Start button.
#[tauri::command]
pub async fn start_action<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    project: String,
    action: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let daily = with_vault(&state.vault(), move |vault| {
        vault.start_action(now, &project, &action)
    })
    .await??;
    record_and_emit(&app, &state, vec![daily], vec![VaultArea::Daily]);
    Ok(())
}

/// Complete the action bullet matching `action` on `project`
/// (case-insensitive substring; ambiguity comes back as
/// `CmdError::Ambiguous` and the UI shows a picker). Removes the
/// bullet and logs the completion to today's daily note.
#[tauri::command]
pub async fn complete_action<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    project: String,
    action: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let slug = project.clone();
    let outcome = with_vault(&state.vault(), move |vault| {
        vault.complete_action(now, &slug, &action)
    })
    .await??;
    // Journal exactly what the domain wrote: the project map, the daily
    // log line, and — when the completed bullet wikilinked an action note
    // — the archival move's source and destination. Taking the domain's
    // reported set (rather than reconstructing it here) is what stops the
    // watcher echoing those archive writes back as external edits (#315).
    record_outcome_and_emit(
        &app,
        &state,
        &outcome,
        vec![VaultArea::Projects, VaultArea::Daily, VaultArea::Actions],
    );
    Ok(())
}

/// The daily-note path for `date`, for journalling writes that log to
/// the daily as a side effect. Mirrors the domain's
/// `journal/<year>/daily/<date>.md` rule via the shared relpath helper.
/// Callers pass the same `date` the domain call used, so the journalled
/// path can't drift across a midnight boundary between the write and the
/// path reconstruction. `#[doc(hidden)] pub` so the IPC integration
/// tests can locate the note a write was expected to touch.
#[doc(hidden)]
pub fn daily_path_for(date: NaiveDate) -> VaultPath {
    VaultPath::new(cdno_core::paths::daily_note_relpath(date))
        .expect("the daily relpath rule always yields a valid vault path")
}

/// Add a next action to an active project's `## Next Actions` (Home
/// card / Actions-view / Project-Detail quick-row). `energy` is the
/// wire string (`"deep" | "medium" | "light"`); an unrecognised value
/// is a `CmdError::Invalid` — the frontend's select can only send the
/// three, but a malformed IPC call must fail loudly rather than default
/// to a bucket. The domain appends the bullet and logs the addition to
/// today's daily in one transaction.
#[tauri::command]
pub async fn add_action<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    project: String,
    action: String,
    energy: String,
) -> Result<(), CmdError> {
    let energy = EnergyLevel::from_str(&energy).map_err(|e| CmdError::Invalid(e.to_string()))?;
    let now = Local::now().naive_local();
    let date = now.date();
    let project_path = with_vault(&state.vault(), move |vault| {
        vault.add_action(now, &project, &action, energy)
    })
    .await??;
    // add_action stages the daily-log line in the same transaction, so
    // both the project map (returned) and the daily are ours to
    // journal. Actions invalidates too — the new bullet shows up in the
    // cross-project Actions view.
    let daily = daily_path_for(date);
    record_and_emit(
        &app,
        &state,
        vec![project_path, daily],
        vec![VaultArea::Projects, VaultArea::Actions, VaultArea::Daily],
    );
    Ok(())
}

/// Promote an open action bullet to a manifest action note (Actions
/// view / Project Detail). Matching is the same case-insensitive
/// substring as `complete_action` (ambiguity → `CmdError::Ambiguous`,
/// missing → `NotFound`). The domain spins the note, rewrites the
/// bullet to wikilink it, and logs the promotion to today's daily —
/// all atomic.
#[tauri::command]
pub async fn promote_action<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    project: String,
    action: String,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let date = now.date();
    let slug = project.clone();
    let note_path = with_vault(&state.vault(), move |vault| {
        vault.promote_action(now, &slug, &action)
    })
    .await??;
    // The transaction touched three files, of which the domain returns
    // only the new action note. The other two are deterministic from
    // the inputs and so ours to journal: the project map — which must be
    // active for a promote to succeed, hence `projects/<project>.md` —
    // whose bullet was rewritten, and today's daily where the promotion
    // was logged.
    let project_path = VaultPath::new(format!("{}/{project}.md", cdno_core::paths::PROJECTS))
        .map_err(|e| CmdError::Invalid(e.to_string()))?;
    let daily = daily_path_for(date);
    record_and_emit(
        &app,
        &state,
        vec![note_path, project_path, daily],
        vec![VaultArea::Projects, VaultArea::Actions, VaultArea::Daily],
    );
    Ok(())
}

/// One active project's open actions, for the cross-project Actions
/// view (plan §1.2). Carries the slug and life `context` (for the
/// colour dot) alongside the bullets so the view groups and tints
/// without a second read.
#[cfg_attr(feature = "ts-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-bindings", ts(export))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectActions {
    pub slug: String,
    pub context: Context,
    pub actions: Vec<ActionListEntry>,
}

/// Compose the Actions view's data: every active project with its open
/// action bullets. Public and synchronous — the test seam.
pub fn list_all_actions_impl(vault: &Vault) -> Result<Vec<ProjectActions>, CmdError> {
    let mut out = Vec::new();
    for (path, fm) in vault.active_projects()? {
        // The active-project slug is the file stem of `projects/<slug>.md`.
        let slug = path
            .as_path()
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        let actions = vault.list_actions(&slug)?;
        out.push(ProjectActions {
            slug,
            context: fm.context,
            actions,
        });
    }
    Ok(out)
}

/// Every active project's open actions — the cross-project Actions
/// list. Pure read: no journal, no events.
#[tauri::command]
pub async fn list_all_actions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProjectActions>, CmdError> {
    with_vault(&state.vault(), list_all_actions_impl).await?
}
