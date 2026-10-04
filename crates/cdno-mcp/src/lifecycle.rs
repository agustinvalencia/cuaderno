//! Lifecycle operation handlers (GH #166): move projects, questions,
//! and stewardship commitments through their lifecycle.
//!
//! Split into its own `#[tool_router]` group (`lifecycle_router`),
//! merged into the dispatch table in [`CuadernoServer::new`]. This is
//! the first slice of the handler-group split; the remaining context /
//! operations / creation handlers stay in `server.rs` for now and can
//! peel off into their own routers the same way.

use std::str::FromStr;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorData};
use rmcp::{tool, tool_router};

use cdno_domain::frontmatter::QuestionStatus;
use cdno_domain::recurrence::Recurrence;

use crate::dto::ProjectClosureDto;
use crate::input::{
    AddPeriodicCommitmentInput, DropProjectInput, OpenItemsChoice, ProjectSlugInput,
    SetQuestionStatusInput,
};
use crate::server::CuadernoServer;
use crate::util::{into_mcp_error, invalid_argument};
use crate::verify::WriteShape;

#[tool_router(router = lifecycle_router, vis = "pub")]
impl CuadernoServer {
    #[tool(
        description = "Park an active project: move it to `projects/_parked/` and flip its status to parked, freeing an active slot."
    )]
    pub async fn park_project(
        &self,
        Parameters(input): Parameters<ProjectSlugInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .with_vault(move |vault| vault.park_project(at, &input.project))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Parked project at {}", path);
        self.verified_write_focused(path, message, WriteShape::Rewritten, at.date())
            .await
    }

    #[tool(
        description = "Complete a project: the work is done. Moves the map to `projects/_done/<year>/`, sets `status: completed` and `closed:` to today, and logs `project completed [[slug]] — <title>`. Works on an active or a parked project and never needs a slot. A completion is a claim that the work was done, so it is REFUSED while any action or milestone is still open: the `project_has_open_items` rejection lists them (with `untouched_commitments`, standalone commitments that stay open either way). Each must be completed (`complete_action`, `complete_milestone`) or dropped (`drop_action`, `drop_milestone`, with a reason) first; this tool never lets open work go. A dropped project may be completed later as a new decision; completing one already completed is refused with its date."
    )]
    pub async fn complete_project(
        &self,
        Parameters(input): Parameters<ProjectSlugInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let outcome = self
            .with_vault(move |vault| vault.complete_project(at, &input.project))
            .await?
            .map_err(into_mcp_error)?;
        self.verified_closure(outcome, "Completed", at.date()).await
    }

    #[tool(
        description = "Drop a project: it is not going to happen. Moves the map to `projects/_done/<year>/`, sets `status: dropped` and `closed:` to today, and logs `project dropped on [[slug]] — <title>` with `reason` when given (give one). Works on an active or a parked project and never needs a slot. With open actions or milestones it is REFUSED by default (`open_items: \"refuse\"`): the `project_has_open_items` rejection lists them and carries `open_items_hash`. Show that list to the user; items that were in fact done should be completed first, because a drop is not a claim that work was done. Only if the user agrees to let the rest go, call again with `open_items: \"drop\"` and `expected_open_items` set to that `open_items_hash`: each open item is then dropped too, logged with `reason: project dropped (<reason>)` (plain `reason: project dropped` without a `reason`), and attached action notes are archived as dropped. If the list changed in between, the call is refused again with the new list. Linked standalone commitments are never touched. A completed project may be dropped later as a new decision; dropping one already dropped is refused with its date."
    )]
    pub async fn drop_project(
        &self,
        Parameters(input): Parameters<DropProjectInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let open_items = match input.open_items {
            OpenItemsChoice::Refuse => cdno_domain::OpenItems::Refuse,
            // An agent must present the hash of the list it showed, so an
            // item added in between is refused rather than dropped unseen
            // (RFC 0004 D10).
            OpenItemsChoice::Drop => cdno_domain::OpenItems::Drop {
                expected: input
                    .expected_open_items
                    .map(cdno_domain::OpenItemsHash::from),
                hash_required: true,
            },
        };
        let outcome = self
            .with_vault(move |vault| {
                vault.drop_project(at, &input.project, input.reason.as_deref(), open_items)
            })
            .await?
            .map_err(into_mcp_error)?;
        self.verified_closure(outcome, "Dropped", at.date()).await
    }

    #[tool(
        description = "Activate a parked or closed project: move it back to `projects/` from `projects/_parked/` or `projects/_done/<year>/` and flip its status to active; a closed project also has its `closed:` date cleared, so it can be closed again later. Logs `project [[slug]] activated`. Enforces the active-project cap — errors if the vault is already at the cap (park another first)."
    )]
    pub async fn activate_project(
        &self,
        Parameters(input): Parameters<ProjectSlugInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .with_vault(move |vault| vault.activate_project(at, &input.project))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Activated project at {}", path);
        self.verified_write_focused(path, message, WriteShape::Rewritten, at.date())
            .await
    }

    #[tool(
        description = "Set a question's status. `status` is one of `active`, `parked`, `answered`, `retired`; any other value is rejected as an invalid argument. No-op if the question is already in that status."
    )]
    pub async fn set_question_status(
        &self,
        Parameters(input): Parameters<SetQuestionStatusInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let status = QuestionStatus::from_str(&input.status)
            .map_err(|e| invalid_argument("status", &e.to_string()))?;
        let outcome = self
            .with_vault(move |vault| vault.set_question_status(at, &input.question, status))
            .await?
            .map_err(into_mcp_error)?;
        // Tell an agent when nothing changed, so it does not report a
        // status move that did not happen.
        let touched = outcome.touched();
        let path = outcome.primary;
        let message = if touched {
            format!("Set question status on {path}")
        } else {
            format!("{path} is already {}", status.as_str())
        };
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Append a periodic commitment to a stewardship's `## Periodic Commitments` section. `recurrence` is one of `daily`, `weekly`, `monthly`, `yearly`, or `every N months`; `next_date` is the ISO `YYYY-MM-DD` of the next occurrence."
    )]
    pub async fn add_periodic_commitment(
        &self,
        Parameters(input): Parameters<AddPeriodicCommitmentInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let recurrence = Recurrence::from_str(&input.recurrence)
            .map_err(|e| invalid_argument("recurrence", &e.to_string()))?;
        let path = self
            .with_vault(move |vault| {
                vault.add_periodic_commitment(
                    at,
                    &input.stewardship,
                    &input.title,
                    recurrence,
                    input.next_date,
                )
            })
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Added periodic commitment to {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }
}

impl CuadernoServer {
    /// Verify a closed map landed and report what the close let go.
    async fn verified_closure(
        &self,
        outcome: cdno_domain::ProjectClosureOutcome,
        verb: &str,
        today: chrono::NaiveDate,
    ) -> Result<CallToolResult, ErrorData> {
        let path = outcome.outcome.primary.clone();
        let message = format!("{verb} project at {path}");
        let reported = path.to_string();
        self.verified_write_with_focus(
            path,
            WriteShape::Rewritten,
            today,
            move |verification, focus| ProjectClosureDto {
                path: reported,
                message,
                dropped_actions: outcome.dropped_actions,
                dropped_milestones: outcome.dropped_milestones,
                untouched_commitments: outcome
                    .untouched_commitments
                    .into_iter()
                    .map(Into::into)
                    .collect(),
                verification,
                focus,
            },
        )
        .await
    }
}
