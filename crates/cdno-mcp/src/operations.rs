//! Operation (write) tool handlers. Part of the handler-group split: a separate
//! `#[tool_router(router = operations_router)]` impl merged into the dispatch
//! table in `CuadernoServer::new`.

use std::str::FromStr;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorData};
use rmcp::{tool, tool_router};

use cdno_domain::frontmatter::{Context, EnergyLevel};
use cdno_domain::{DailySection, WeeklySection};

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
        description = "Capture a raw line into the inbox for later triage -- zero-friction quick capture, the counterpart to `append_to_log` for thoughts that aren't a dated log entry. The text is stored verbatim under `inbox/`; routing it into a task/note happens later."
    )]
    pub async fn capture(
        &self,
        Parameters(input): Parameters<CaptureInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .capture_to_inbox(at, &input.text)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Captured to {}", path),
        ))
    }

    #[tool(
        description = "Discard a triaged inbox capture by `slug` (from `triage_inbox`): deletes the note and logs the discard. Use once its content has been routed elsewhere (e.g. via `add_action`), or to drop it outright."
    )]
    pub async fn discard_inbox_item(
        &self,
        Parameters(input): Parameters<DiscardInboxItemInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .discard_inbox_item(at, &input.slug)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Discarded {}", path),
        ))
    }

    #[tool(
        description = "Add a milestone to an active project's `## Milestones`. `target_date` is ISO `YYYY-MM-DD`. `hard: true` records a hard deadline that the commitments aggregation surfaces; omit it (or `false`) for a soft target. The section is auto-created if missing."
    )]
    pub async fn add_milestone(
        &self,
        Parameters(input): Parameters<AddMilestoneInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .add_milestone(
                at,
                &input.project,
                &input.title,
                input.target_date,
                input.hard,
            )
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Added milestone to {}", path),
        ))
    }

    #[tool(
        description = "Complete an open milestone on an active project: ticks the bullet in `## Milestones`. `query` is a case-insensitive substring of the milestone title (the `-- <keyword>: <date>` suffix is ignored); already-completed bullets are skipped."
    )]
    pub async fn complete_milestone(
        &self,
        Parameters(input): Parameters<CompleteMilestoneInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .complete_milestone(at, &input.project, &input.query)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Completed milestone on {}", path),
        ))
    }

    #[tool(
        description = "Record a blocker in an active project's `## Waiting On`. `description` is informational (no checkbox) -- e.g. `Vendor quote -- awaiting reply`. The section is auto-created and its `(nothing yet)` placeholder replaced."
    )]
    pub async fn add_waiting_on(
        &self,
        Parameters(input): Parameters<AddWaitingOnInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .add_waiting_on(at, &input.project, &input.description)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Added waiting-on to {}", path),
        ))
    }

    #[tool(
        description = "Remove a resolved blocker from an active project's `## Waiting On`. `query` is a case-insensitive substring of the waiting-on line; if it was the last one, the `(nothing yet)` placeholder is restored."
    )]
    pub async fn resolve_waiting_on(
        &self,
        Parameters(input): Parameters<ResolveWaitingOnInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .resolve_waiting_on(at, &input.project, &input.query)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Resolved waiting-on on {}", path),
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
        // abstract respectively. `vars` feeds the evidence template's
        // prompted variables and is ignored on the (non-templated) attach path.
        let vars = input.vars.unwrap_or_default();
        let path = match input.attach.as_deref() {
            Some(artefact) => self.vault.file_attachment(
                at,
                &input.portfolio,
                std::path::Path::new(artefact),
                &input.source,
                &input.origin,
                &input.content,
            ),
            None => self.vault.file_evidence_with_vars(
                at,
                &input.portfolio,
                &input.source,
                &input.origin,
                &input.content,
                &vars,
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
            // Only the action-note form is templated; `vars` feeds its
            // prompted variables. The inline-bullet form below ignores them.
            let vars = input.vars.unwrap_or_default();
            self.vault
                .add_action_with_note_and_vars(at, &input.project, &input.title, energy, &vars)
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
        Parameters(input): Parameters<PromoteActionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let vars = input.vars.unwrap_or_default();
        let path = self
            .vault
            .promote_action_with_vars(at, &input.project, &input.query, &vars)
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
        description = "Create a standalone commitment note with a due date and life context. Optional `project` and `stewardship` are bare origin-link slugs recording which project or stewardship the commitment relates to; that source can then list its related dated items. Omit both for a purely standalone commitment (the common case per design \u{00a7}5.9). The links are loose pointers \u{2014} the target's existence isn't validated."
    )]
    pub async fn create_commitment(
        &self,
        Parameters(input): Parameters<CreateCommitmentInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let context = Context::from_str(&input.context)
            .map_err(|e| invalid_argument("context", &e.to_string()))?;
        let vars = input.vars.unwrap_or_default();
        let path = self
            .vault
            .create_commitment_with_vars(
                at,
                &input.title,
                input.due,
                context,
                input.project.as_deref(),
                input.stewardship.as_deref(),
                &vars,
            )
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
        let vars = input.vars.unwrap_or_default();
        let path = self
            .vault
            .add_tracking_entry_with_vars(
                at,
                &input.stewardship,
                &input.activity,
                input.routine.as_deref(),
                &input.content,
                &vars,
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

    #[tool(
        description = "Write a section of the weekly note for the ISO week containing `date` (any day in the week; defaults to this week). `section` is one of `Wins`, `Challenges`, `One Improvement`, `This Week's Goal` (case-insensitive; the former `Next Week's Focus` is still accepted as a deprecated alias and maps to `This Week's Goal`); any other value is rejected. Creates the weekly note (frontmatter + all four section headings) if absent. With `append: false` (default) the section is replaced — compose the review; with `append: true` the content is appended — accrue within a section across a session. `This Week's Goal` is the week's anchoring goal: set ahead of the week by weekly-planning (pass a `date` in the week being planned to create that week's note and set its goal in one call), or written into next week's note by weekly-review as the carry-forward hand-off. cdno keeps no separate weekly-plan note: the review and the plan share this one artefact per week."
    )]
    pub async fn upsert_weekly_section(
        &self,
        Parameters(input): Parameters<UpsertWeeklySectionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let date = input
            .date
            .unwrap_or_else(|| chrono::Local::now().date_naive());
        let section = WeeklySection::from_str(&input.section)
            .map_err(|reason| invalid_argument("section", &reason))?;
        let path = self
            .vault
            .upsert_weekly_section(date, section, &input.content, input.append)
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
