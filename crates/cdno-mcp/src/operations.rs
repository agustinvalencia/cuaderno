//! Operation (write) tool handlers. Part of the handler-group split: a separate
//! `#[tool_router(router = operations_router)]` impl merged into the dispatch
//! table in `CuadernoServer::new`.

use std::str::FromStr;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorData};
use rmcp::{tool, tool_router};

use cdno_domain::DailySection;
use cdno_domain::frontmatter::{Context, EnergyLevel};

use crate::dto::WriteResultDto;

use crate::input::*;

use crate::util::{into_mcp_error, invalid_argument, json_result};

use crate::server::CuadernoServer;

#[tool_router(router = operations_router, vis = "pub")]
impl CuadernoServer {
    #[tool(
        description = "Append a single line to today's daily log entry, creating the daily note if it doesn't yet exist."
    )]
    pub async fn append_to_log(
        &self,
        Parameters(input): Parameters<AppendToLogInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .log_to_daily_note(at, &input.text)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Logged to {}", path),
        ))
    }

    #[tool(
        description = "File evidence into the named portfolio. `origin` is a bare wikilink target (e.g. `projects/foo`); the server wraps it. Resolve a real slug in `origin` (e.g. a project via `get_orientation`) rather than guessing — `origin` is not validated, so a wrong slug silently writes a dangling link instead of erroring. By default writes a markdown evidence note with `content` as the body. To file a non-markdown artefact (PDF, image, video, …), set `attach` to its server-side path: the file is copied into the portfolio and a linked stub is scaffolded, and `content` becomes the stub's abstract — write a descriptive one, since it's the only thing search and other agents see of the artefact."
    )]
    pub async fn file_to_portfolio(
        &self,
        Parameters(input): Parameters<FileToPortfolioInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        // With `attach`, file the artefact (copy + linked stub); otherwise
        // write a plain markdown evidence note. `content` is the body /
        // abstract respectively.
        let path = match input.attach.as_deref() {
            Some(artefact) => self.vault.file_attachment(
                at,
                &input.portfolio,
                std::path::Path::new(artefact),
                &input.source,
                &input.origin,
                &input.content,
            ),
            None => self.vault.file_evidence(
                at,
                &input.portfolio,
                &input.source,
                &input.origin,
                &input.content,
            ),
        }
        .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Filed evidence at {}", path),
        ))
    }

    #[tool(
        description = "Rewrite a project's `## Current State` section, auto-logging the previous state to today's daily entry in the same atomic transaction. No-op (returns the path) when `new_state` matches the existing state — silent so logging 'was X, now X' doesn't fire."
    )]
    pub async fn update_project_state(
        &self,
        Parameters(input): Parameters<UpdateProjectStateInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .update_project_state(at, &input.project, &input.new_state)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Updated state on {}", path),
        ))
    }

    #[tool(
        description = "Append a next-action bullet to a project. With `with_note: true`, also creates an action note (design §5.11) and rewrites the bullet to wikilink it. `energy` is one of `deep`, `medium`, `light`."
    )]
    pub async fn add_action(
        &self,
        Parameters(input): Parameters<AddActionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let energy = EnergyLevel::from_str(&input.energy)
            .map_err(|e| invalid_argument("energy", &e.to_string()))?;
        let path = if input.with_note {
            self.vault
                .add_action_with_note(at, &input.project, &input.title, energy)
                .map_err(into_mcp_error)?
        } else {
            self.vault
                .add_action(at, &input.project, &input.title, energy)
                .map_err(into_mcp_error)?
        };
        let label = if input.with_note {
            "Added action with note at"
        } else {
            "Added action bullet to"
        };
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("{label} {}", path),
        ))
    }

    #[tool(
        description = "Promote an existing bullet to an action note: matches the bullet on the project by substring `query`, creates the note from the template, and rewrites the bullet to wikilink it. Errors with `INTERNAL_ERROR` on ambiguous matches (multiple bullets contain `query`)."
    )]
    pub async fn promote_action(
        &self,
        Parameters(input): Parameters<ActionQueryInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .promote_action(at, &input.project, &input.query)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Promoted action note at {}", path),
        ))
    }

    #[tool(
        description = "Complete an action: matches the bullet on the project by substring `query`, removes the bullet, logs the completion to today's daily, and (if an action note is attached) archives it to `actions/_done/<year>/`."
    )]
    pub async fn complete_action(
        &self,
        Parameters(input): Parameters<ActionQueryInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .complete_action(at, &input.project, &input.query)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Completed action on {}", path),
        ))
    }

    #[tool(
        description = "Create a standalone commitment note with a due date and life context. `project` and `stewardship` are reserved on the input schema for the eventual originating-source link but ignored today \u{2014} the domain currently writes both as null per design \u{00a7}5.9 (originating commitments are tracked inline at their source: project milestones, stewardship periodic commitments)."
    )]
    pub async fn create_commitment(
        &self,
        Parameters(input): Parameters<CreateCommitmentInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let context = Context::from_str(&input.context)
            .map_err(|e| invalid_argument("context", &e.to_string()))?;
        // input.project and input.stewardship are deliberately unused
        // today; see tool description.
        let path = self
            .vault
            .create_commitment(at, &input.title, input.due, context)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created commitment at {}", path),
        ))
    }

    #[tool(
        description = "Mark an active commitment as completed: stamps the `status` and `completed` frontmatter fields, moves the file to `commitments/_done/<year>/`, and logs to today's daily entry. All in one atomic transaction."
    )]
    pub async fn complete_commitment(
        &self,
        Parameters(input): Parameters<CompleteCommitmentInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .complete_commitment(at, &input.commitment)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Completed commitment, archived to {}", path),
        ))
    }

    #[tool(
        description = "Scaffold a tracking note under an expanded stewardship. `stewardship` must be the slug of an existing expanded stewardship — do not invent one (there is no generic `fitness`; gym sessions go under `gym`); on a miss the error lists the valid slugs. Built-in templates for `gym`, `body`, and `swim`; generic fallback for anything else. `routine` is the bare slug of a routine doc (e.g. `upper-body-a`); the server wraps it into the gym template's `routine:` wikilink. Returns the new path for the user to flesh out."
    )]
    pub async fn create_tracking_entry(
        &self,
        Parameters(input): Parameters<CreateTrackingEntryInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .add_tracking_entry(
                at,
                &input.stewardship,
                &input.activity,
                input.routine.as_deref(),
                &input.content,
            )
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Tracked at {}", path),
        ))
    }

    #[tool(
        description = "Write a section of the daily note (defaults to today). `section` is one of `Standup`, `Intention`, `Agenda`, `Meeting` (case-insensitive); any other value is rejected as an invalid argument. The append-only history sections (`## Logs`, `## Notes`) are NOT writable here — they grow via `append_to_log`. With `append: false` (default) the section is replaced (the planning sections); with `append: true` the content is appended (live meeting notes that accrue). Creates the section (and the daily note) if absent. An empty `content` with `append: false` clears the section to just its heading."
    )]
    pub async fn upsert_daily_section(
        &self,
        Parameters(input): Parameters<UpsertDailySectionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let date = input
            .date
            .unwrap_or_else(|| chrono::Local::now().date_naive());
        let section = DailySection::from_str(&input.section)
            .map_err(|reason| invalid_argument("section", &reason))?;
        let path = self
            .vault
            .upsert_daily_section(date, section, &input.content, input.append)
            .map_err(into_mcp_error)?;
        let verb = if input.append {
            "Appended to"
        } else {
            "Updated"
        };
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("{verb} {} on {}", section.heading(), path),
        ))
    }
}
