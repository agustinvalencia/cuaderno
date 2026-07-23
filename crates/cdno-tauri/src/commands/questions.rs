//! The Questions view (#443) — RLM's Important Questions as a surface of
//! their own.
//!
//! Questions sit *above* the project level: three-to-five research and
//! three-to-five life questions, reviewed monthly, there to stop drift
//! across months. They existed in the domain, the CLI and the MCP, but the
//! desktop app surfaced them only as chips inside the Strategic dashboard —
//! the view visited least. The thing meant to sit above projects was
//! reachable only through the one place you go monthly.
//!
//! The rows are composed by the same `question_rows` the Strategic bundle
//! uses, so the two cannot disagree about what a question is linked to.

use chrono::Local;

use cdno_domain::Vault;
use cdno_domain::frontmatter::question::QuestionStatus;

use crate::error::CmdError;
use crate::events::VaultArea;
use crate::state::AppState;
use crate::with_vault::with_vault;

use super::actions::record_and_emit;

use super::strategic::{QuestionStrategicRow, question_rows};

/// Every question with its backlinks, whatever its status.
///
/// Unlike the Strategic grid this does not filter to `active`: the view
/// groups by domain and lets the reader see the parked and answered ones
/// too, which is the difference between a dashboard and a place the
/// questions live.
pub fn list_questions_impl(vault: &Vault) -> Result<Vec<QuestionStrategicRow>, CmdError> {
    Ok(question_rows(vault, vault.list_questions()?))
}

#[tauri::command]
pub async fn list_questions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<QuestionStrategicRow>, CmdError> {
    with_vault(&state.vault(), list_questions_impl).await?
}

/// Move a question between active / parked / answered / retired.
///
/// The one write this view has. Without it the page would repeat the
/// mistake the Strategic dashboard makes — showing the state of things
/// with no way to act on it — and a question you have answered would go on
/// asking itself.
#[tauri::command]
pub async fn set_question_status<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, AppState>,
    slug: String,
    status: QuestionStatus,
) -> Result<(), CmdError> {
    let now = Local::now().naive_local();
    let path = with_vault(&state.vault(), move |vault| {
        vault.set_question_status(now, &slug, status)
    })
    .await??;
    record_and_emit(&app, &state, vec![path], vec![VaultArea::Questions]);
    Ok(())
}
