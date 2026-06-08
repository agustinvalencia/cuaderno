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

use crate::dto::WriteResultDto;
use crate::input::{AddPeriodicCommitmentInput, ProjectSlugInput, SetQuestionStatusInput};
use crate::server::CuadernoServer;
use crate::util::{into_mcp_error, invalid_argument, json_result};

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
            .vault
            .park_project(at, &input.project)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Parked project at {}", path),
        ))
    }

    #[tool(
        description = "Activate a parked project: move it back to `projects/` and flip its status to active. Enforces the active-project cap — errors if the vault is already at the cap (park another first)."
    )]
    pub async fn activate_project(
        &self,
        Parameters(input): Parameters<ProjectSlugInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .activate_project(at, &input.project)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Activated project at {}", path),
        ))
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
        let path = self
            .vault
            .set_question_status(at, &input.question, status)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Set question status on {}", path),
        ))
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
            .vault
            .add_periodic_commitment(
                at,
                &input.stewardship,
                &input.title,
                recurrence,
                input.next_date,
            )
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Added periodic commitment to {}", path),
        ))
    }
}
