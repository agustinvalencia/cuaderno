//! Operation (write) tool handlers. Part of the handler-group split: a separate
//! `#[tool_router(router = operations_router)]` impl merged into the dispatch
//! table in `CuadernoServer::new`.

use std::str::FromStr;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorData};
use rmcp::{tool, tool_router};

use cdno_domain::frontmatter::{Context, EnergyLevel};
use cdno_domain::{DailySection, MonthlySection, TrackingEntryDraft, WeeklySection};

use crate::input::*;

use crate::util::{into_mcp_error, invalid_argument};

use crate::server::CuadernoServer;
use crate::verify::WriteShape;

#[tool_router(router = operations_router, vis = "pub")]
impl CuadernoServer {
    #[tool(
        description = "Append a single line to today's daily log entry, creating the daily note if it doesn't yet exist. The entry is stamped with the vault clock (`- **HH:MM**: <text>`), so pass the text alone; a time you prefix yourself is stamped twice. Link as you write: wikilink every vault note the line names (`[[slug]]`), and render every forge reference (issue, MR/PR, epic, commit, repo file) as a markdown link rather than a bare `#N`. An unlinked line is invisible to the vault graph."
    )]
    pub async fn append_to_log(
        &self,
        Parameters(input): Parameters<AppendToLogInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .with_vault(move |vault| vault.log_to_daily_note(at, &input.text))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Logged to {}", path);
        // The section comes from the domain, never a literal here: the
        // MCP layer must not carry its own opinion about where a log
        // line lands (see `WriteShape::AppendedToSection`).
        self.verified_write(
            path,
            message,
            WriteShape::AppendedToSection(cdno_domain::DAILY_LOGS_SECTION),
        )
        .await
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
            .with_vault(move |vault| vault.capture_to_inbox(at, &input.text))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Captured to {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
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
            .with_vault(move |vault| vault.discard_inbox_item(at, &input.slug))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Discarded {}", path);
        self.verified_write(path, message, WriteShape::Removed)
            .await
    }

    #[tool(
        description = "Add a milestone to an active project's `## Milestones`. `target_date` is ISO `YYYY-MM-DD`; omit it for a milestone gated by a condition rather than a date (\"all Round-1 replies received\") -- the bullet records `target: TBD` and stays out of the commitments aggregation. Never invent an estimate to fill the field: a fabricated date reads later as a commitment somebody made. `hard: true` records a hard deadline that the commitments aggregation surfaces, and requires `target_date`; omit it (or `false`) for a soft target. The section is auto-created if missing."
    )]
    pub async fn add_milestone(
        &self,
        Parameters(input): Parameters<AddMilestoneInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .with_vault(move |vault| {
                vault.add_milestone(
                    at,
                    &input.project,
                    &input.title,
                    input.target_date,
                    input.hard,
                )
            })
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Added milestone to {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
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
            .with_vault(move |vault| vault.complete_milestone(at, &input.project, &input.query))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Completed milestone on {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
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
            .with_vault(move |vault| vault.add_waiting_on(at, &input.project, &input.description))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Added waiting-on to {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
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
            .with_vault(move |vault| vault.resolve_waiting_on(at, &input.project, &input.query))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Resolved waiting-on on {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "File evidence into the named portfolio. `origin` is a bare wikilink target (e.g. `projects/foo`); the server wraps it. Resolve a real slug in `origin` (e.g. a project via `get_orientation`) rather than guessing — `origin` is not validated, so a wrong slug silently writes a dangling link instead of erroring. By default writes a markdown evidence note with `content` as the body. To file a non-markdown artefact (PDF, image, video, …), set `attach` to its server-side path: the file is copied into the portfolio and a linked stub is scaffolded, and `content` becomes the stub's abstract — write a descriptive one, since it's the only thing search and other agents see of the artefact."
    )]
    pub async fn file_to_portfolio(
        &self,
        Parameters(input): Parameters<FileToPortfolioInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .with_vault(move |vault| {
                // With `attach`, file the artefact (copy + linked stub); otherwise
                // write a plain markdown evidence note. `content` is the body /
                // abstract respectively. Only the markdown path is templated, so
                // `vars` is gathered inside that arm (the attach stub ignores it).
                match input.attach.as_deref() {
                    Some(artefact) => vault.file_attachment(
                        at,
                        &input.portfolio,
                        std::path::Path::new(artefact),
                        &input.source,
                        &input.origin,
                        &input.content,
                    ),
                    None => {
                        let vars = input.vars.unwrap_or_default();
                        vault.file_evidence_with_vars(
                            at,
                            &input.portfolio,
                            &input.source,
                            &input.origin,
                            &input.content,
                            &vars,
                        )
                    }
                }
            })
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Filed evidence at {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Set or clear an active project's `core_question` after creation. `core_question` is the **bare** wikilink target (e.g. `questions/research/foo`), the same form `create_project` takes -- passing `[[...]]` is rejected, and so is a bare slug like `foo` without its `questions/<domain>/` prefix. To detach the question instead, pass `clear: true`; omitting both is an error rather than a silent detach, because dropping a project's question is a decision and must be asked for. The previous value is auto-logged to today's daily note, so a project's changing question is traceable; setting the value it already holds is a no-op."
    )]
    pub async fn set_core_question(
        &self,
        Parameters(input): Parameters<SetCoreQuestionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        // Mirrors the CLI's `--question` / `--clear` pair. An omitted
        // field is the likeliest agent slip, and it must not be the one
        // that quietly unlinks a project from its question.
        if input.clear && input.core_question.is_some() {
            return Err(invalid_argument(
                "clear",
                "pass either `core_question` or `clear: true`, not both",
            ));
        }
        if !input.clear && input.core_question.is_none() {
            return Err(invalid_argument(
                "core_question",
                "pass a bare target such as `questions/research/foo` to set one, or `clear: true` \
                 to detach the project's current question",
            ));
        }
        let outcome = self
            .with_vault(move |vault| {
                vault.set_core_question(at, &input.project, input.core_question.as_deref())
            })
            .await?
            .map_err(into_mcp_error)?;
        let path = outcome.primary;
        let message = format!("Set core question on {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Rewrite a project's `## Current State` section, auto-logging the previous state to today's daily entry in the same atomic transaction. No-op (returns the path) when `new_state` matches the existing state — silent so logging 'was X, now X' doesn't fire."
    )]
    pub async fn update_project_state(
        &self,
        Parameters(input): Parameters<UpdateProjectStateInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        // The MCP surface reports a single path; take the outcome's
        // primary (the project map). The richer touched-path set is for
        // the desktop echo journal (#315) and is ignored here.
        let outcome = self
            .with_vault(move |vault| {
                vault.update_project_state(at, &input.project, &input.new_state)
            })
            .await?
            .map_err(into_mcp_error)?;
        let path = outcome.primary;
        // Fold any soft length advisory (state_overflow = "warn") into
        // the message so the calling agent sees it and can self-correct
        // on the next write; `reject` mode never reaches here (it errors).
        let mut message = format!("Updated state on {}", path);
        for warning in &outcome.warnings {
            message.push('\n');
            message.push_str(warning);
        }
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Set a declared, settable typed frontmatter field on a note, writing through the index so it never desyncs. `note` is `today`, a `YYYY-MM-DD` date (daily note), or a vault-relative path. `key` must be declared `settable = true` under `[schemas.<type>.fields.<key>]`; `value` is coerced to the declared type. Engine-owned keys (`type`, `status`, the calendar period key) are rejected -- use the lifecycle tools for those. Toggles daily flags like `meds`/`workout`/`closed` without a hand-edit."
    )]
    pub async fn set_frontmatter(
        &self,
        Parameters(input): Parameters<SetFrontmatterInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        // The MCP surface reports a single path; take the outcome's primary.
        // The richer touched-path set is for the desktop echo journal (#315).
        let path = self
            .with_vault(move |vault| {
                vault.set_frontmatter(at, &input.note, &input.key, &input.value)
            })
            .await?
            .map_err(into_mcp_error)?
            .primary;
        let message = format!("Set frontmatter on {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Append a next-action bullet to a project. This is the default way to capture an action — a bullet on the project map, not a note per task. Three next actions per project is a ceiling rather than a backlog, so prefer replacing a stale bullet to stacking a fourth. With `with_note: true`, also creates an action note (design §5.11) and rewrites the bullet to wikilink it. `energy` is one of `deep`, `medium`, `light`."
    )]
    pub async fn add_action(
        &self,
        Parameters(input): Parameters<AddActionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let energy = EnergyLevel::from_str(&input.energy)
            .map_err(|e| invalid_argument("energy", &e.to_string()))?;
        let with_note = input.with_note;
        let path = self
            .with_vault(move |vault| {
                if input.with_note {
                    // Only the action-note form is templated; `vars` feeds its
                    // prompted variables. The inline-bullet form below ignores them.
                    let vars = input.vars.unwrap_or_default();
                    vault.add_action_with_note_and_vars(
                        at,
                        &input.project,
                        &input.title,
                        energy,
                        &vars,
                    )
                } else {
                    vault.add_action(at, &input.project, &input.title, energy)
                }
            })
            .await?
            .map_err(into_mcp_error)?;
        let label = if with_note {
            "Added action with note at"
        } else {
            "Added action bullet to"
        };
        let message = format!("{label} {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Promote an existing bullet to an action note. An action is a bullet on the project map by default (add_action); promote it only once it has grown into a multi-day investigation with its own evidence, since a note per task is friction the method is built to avoid. Matches the bullet on the project by substring `query`, creates the note from the template, and rewrites the bullet to wikilink it. Errors with `INTERNAL_ERROR` on ambiguous matches (multiple bullets contain `query`)."
    )]
    pub async fn promote_action(
        &self,
        Parameters(input): Parameters<PromoteActionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .with_vault(move |vault| {
                let vars = input.vars.unwrap_or_default();
                vault.promote_action_with_vars(at, &input.project, &input.query, &vars)
            })
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Promoted action note at {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Complete an action: matches the bullet on the project by substring `query`, removes the bullet, logs the completion to today's daily, and (if an action note is attached) archives it to `actions/_done/<year>/`."
    )]
    pub async fn complete_action(
        &self,
        Parameters(input): Parameters<ActionQueryInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        // Only the primary project path is surfaced here; the outcome's
        // full touched set (which includes any archival move) is for the
        // desktop echo journal (#315), not the MCP reply.
        let path = self
            .with_vault(move |vault| vault.complete_action(at, &input.project, &input.query))
            .await?
            .map_err(into_mcp_error)?
            .primary;
        let message = format!("Completed action on {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
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
        let path = self
            .with_vault(move |vault| {
                let vars = input.vars.unwrap_or_default();
                vault.create_commitment_with_vars(
                    at,
                    &input.title,
                    input.due,
                    context,
                    input.project.as_deref(),
                    input.stewardship.as_deref(),
                    &vars,
                )
            })
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Created commitment at {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
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
            .with_vault(move |vault| vault.complete_commitment(at, &input.commitment))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Completed commitment, archived to {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "File a tracking note under an expanded stewardship. `stewardship` must be the slug of an existing expanded stewardship — do not invent one (there is no generic `fitness`; gym sessions go under `gym`); on a miss the error lists the valid slugs. The `activity` selects the template: a vault's `.cuaderno/templates/tracking-<activity>.md` if present, else the built-in generic template (no activity-specific templates ship built-in). `routine` is the bare slug of a routine doc (e.g. `upper-body-a`); the server wraps it into the template's `routine:` wikilink, taking effect only when the resolved template has a `routine:` field. `metrics` writes the entry's numbers into frontmatter, where they are queryable — a scalar per reading (`{\"balance\": 1240.5}`), or an array of flat records when one entry holds several comparable items (`{\"detail\": [{\"subject\": \"harmony\", \"minutes\": 25}]}`); prefer it over prose in `content` for anything you would later want to total or chart. `date` files the entry for a past day, so a session recorded after the fact lands on the day it happened. A second call for the same activity and date MERGES into the first rather than erroring - its content is appended and its metrics folded in - so a day can be recorded in several passes. Give each record a stable `id` when the payload may be sent more than once (an import, a retry): a record whose `id` matches replaces it, whereas a record without one appends, so re-sending without ids double-counts every summed metric. Filing is journalled to today's daily log either way. Returns the path."
    )]
    pub async fn create_tracking_entry(
        &self,
        Parameters(input): Parameters<CreateTrackingEntryInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let now = chrono::Local::now().naive_local();
        // The payload must be a JSON *object* — each key becomes a frontmatter
        // key. Reject anything else here rather than letting a bare array or
        // scalar reach the merge with no key to write it under.
        let metrics = match input.metrics {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::Object(map)) => Some(map),
            Some(_) => {
                return Err(invalid_argument(
                    "metrics",
                    "must be a JSON object mapping each metric name to its value",
                ));
            }
        };
        let (outcome, _source) = self
            .with_vault(move |vault| {
                let mut draft = TrackingEntryDraft::new(&input.stewardship, &input.activity)
                    .with_content(&input.content)
                    .with_prompted(input.vars.clone().unwrap_or_default());
                if let Some(routine) = &input.routine {
                    draft = draft.with_routine(routine);
                }
                if let Some(date) = input.date {
                    draft = draft.on(date);
                }
                if let Some(metrics) = metrics {
                    draft = draft.with_metrics(metrics);
                }
                vault.add_tracking_entry(now, draft)
            })
            .await?
            .map_err(into_mcp_error)?;
        let path = outcome.primary;
        let message = format!("Tracked at {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Write a section of the daily note (defaults to today). `section` is one of `Standup`, `Intention`, `Agenda`, `Meeting` (case-insensitive); any other value is rejected as an invalid argument. The append-only history sections (`## Logs`, `## Notes`) are NOT writable here — they grow via `append_to_log`. With `append: false` (default) the section is replaced (the planning sections); with `append: true` the content is appended (live meeting notes that accrue). Creates the section (and the daily note) if absent. An empty `content` with `append: false` clears the section to just its heading. The prose written here follows the same linking convention as the log: wikilink the vault notes it names (`[[slug]]`) and give forge references markdown links, never a bare `#N`."
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
        let append = input.append;
        let path = self
            .with_vault(move |vault| {
                vault.upsert_daily_section(date, section, &input.content, input.append)
            })
            .await?
            .map_err(into_mcp_error)?;
        let verb = if append { "Appended to" } else { "Updated" };
        let message = format!("{verb} {} on {}", section.heading(), path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
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
        let append = input.append;
        let path = self
            .with_vault(move |vault| {
                vault.upsert_weekly_section(date, section, &input.content, input.append)
            })
            .await?
            .map_err(into_mcp_error)?;
        let verb = if append { "Appended to" } else { "Updated" };
        let message = format!("{verb} {} on {}", section.heading(), path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Write a section of the monthly note for the calendar month containing `date` (any day in the month; defaults to this month). `section` is one of `Wins`, `Themes`, `Next Month's Focus` (case-insensitive); any other value is rejected. Creates the monthly note (frontmatter + the three section headings + a `## Weeks` block linking the month's weekly notes) if absent. With `append: false` (default) the section is replaced — compose the review; with `append: true` the content is appended — accrue within a section across a session. The monthly note links (never copies) its weeks, so the weekly notes stay the source of truth; there is no Metrics section (quantitative metrics live behind the desktop 'show metrics' toggle)."
    )]
    pub async fn upsert_monthly_section(
        &self,
        Parameters(input): Parameters<UpsertMonthlySectionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let date = input
            .date
            .unwrap_or_else(|| chrono::Local::now().date_naive());
        let section = MonthlySection::from_str(&input.section)
            .map_err(|reason| invalid_argument("section", &reason))?;
        let append = input.append;
        let path = self
            .with_vault(move |vault| {
                vault.upsert_monthly_section(date, section, &input.content, input.append)
            })
            .await?
            .map_err(into_mcp_error)?;
        let verb = if append { "Appended to" } else { "Updated" };
        let message = format!("{verb} {} on {}", section.heading(), path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }
}
