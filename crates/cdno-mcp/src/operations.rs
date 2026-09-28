//! Operation (write) tool handlers. Part of the handler-group split: a separate
//! `#[tool_router(router = operations_router)]` impl merged into the dispatch
//! table in `CuadernoServer::new`.

use std::str::FromStr;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorData};
use rmcp::{tool, tool_router};

use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{Context, EnergyLevel};
use cdno_domain::vault::Revision;
use cdno_domain::{DailySection, MonthlySection, TrackingEntryDraft, WeeklySection};

use crate::context::{refuse_reference_outside_the_vault, resolve_note_reference};
use crate::dto::{NoteToDailyResponse, ReviseNoteResponse};
use crate::input::*;

use crate::util::{into_mcp_error, invalid_argument, json_result};

use crate::server::CuadernoServer;
use crate::verify::WriteShape;

#[tool_router(router = operations_router, vis = "pub")]
impl CuadernoServer {
    #[tool(
        description = "Append a single line to today's daily log entry, creating the daily note if it doesn't yet exist. `## Logs` is the sequence of one-line events; for worked-out substance (a derivation, a procedure, a page of reasoning) use `note_to_daily`, which writes the entry under `## Notes` and this pointer line for you. The entry is stamped with the vault clock (`- **HH:MM**: <text>`), so pass the text alone; a time you prefix yourself is stamped twice. Link as you write: wikilink every vault note the line names (`[[slug]]`), and render every forge reference (issue, MR/PR, epic, commit, repo file) as a markdown link rather than a bare `#N`. An unlinked line is invisible to the vault graph."
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
        description = "Complete an open milestone on an active project: ticks the bullet in `## Milestones`. `query` is a case-insensitive substring of the milestone title (the `-- <keyword>: <date>` suffix is ignored); already-completed bullets are skipped. Use this ONLY when the milestone was actually reached. If it was superseded, mis-typed or is not going to happen, use `drop_milestone` instead -- ticking it writes `milestone done on ...` into the daily log, so the vault would assert a milestone nobody met."
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
        description = "Drop an open milestone from an active project: removes the bullet from `## Milestones` and logs `milestone dropped on [[slug]]` to today's daily note, with an optional `reason` on a continuation line. `query` matches exactly as `complete_milestone`'s does. Use this for a milestone that was superseded, mis-typed or is not going to happen -- it is the only way to be rid of one without either claiming it was met or hand-editing the file, which desyncs the index. Completed bullets are never matched: those are a record of what happened."
    )]
    pub async fn drop_milestone(
        &self,
        Parameters(input): Parameters<DropMilestoneInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .with_vault(move |vault| {
                vault.drop_milestone(at, &input.project, &input.query, input.reason.as_deref())
            })
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Dropped milestone from {}", path);
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
        description = "Rewrite a project's `## Current State` section, auto-logging the previous state to today's daily entry in the same atomic transaction. No-op (returns the path) when `new_state` matches the existing state — silent so logging 'was X, now X' doesn't fire. `new_state` is capped at the vault's `max_state_chars` (default 500), and nothing is ever truncated: under the default `reject` policy new over-length text is refused outright, so write a summary and leave the detail in the daily log — which is where the previous state goes anyway. A vault may set `warn` (over-length accepted, with an advisory) or `off` instead, and trimming a state that is *already* over the cap is always accepted even under `reject`, so an inherited sprawl can be cut down across several edits."
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

    /// Revise a mutable custom note in place (RFC 0002 §6.4, T9).
    ///
    /// The MCP surface is stricter than [`Vault::revise_note`]: a
    /// whole-body revision must carry `expected_hash`, because an agent's
    /// read and its write are separated by a model turn and the lost-update
    /// window is widest here. Argument-shape mistakes are `INVALID_PARAMS`
    /// with the field named; domain refusals (`stale_revision`,
    /// `note_not_revisable`, `revision_invalid`, `ambiguous_section`) and
    /// resolver misses (`not_found`, `ambiguous_slug`) reach the client as
    /// rejections unchanged.
    ///
    /// When the revision changed the note, the result goes through
    /// [`verified_write_with`](CuadernoServer::verified_write_with) with
    /// [`WriteShape::Rewritten`]. When it did not (identical text), nothing
    /// was written or logged, so there is nothing on disk to verify: the
    /// handler returns a plain success saying so, with `verification` null.
    ///
    /// [`Vault::revise_note`]: cdno_domain::Vault::revise_note
    #[tool(
        description = "Refine a mutable custom note (such as a concept) in place, logging the revision to today's daily note in the same transaction; built-in note types (project, action, daily, ...) and custom types declared append-only are refused with code `note_not_revisable`. `note` takes the references `read_note` takes. Give exactly one of two forms. Whole body: pass `body` (everything after the frontmatter, which is kept as it is) together with `expected_hash`, the `content_hash` `read_note` returned -- required, and the revision is refused with code `stale_revision` when the note changed since that read, in which case read it again and redo the edit on the fresh text. One section: pass `section` (heading text as `read_note`'s `headings` lists it, without the `#` markers) and `content` (the section's new text, without its heading) to upsert it -- an existing section is replaced together with its sub-sections, a missing one is appended as `## <section>` -- and no hash is needed. The heading may not contain `[`, `]`, `|` or `#`, nor start with `^`, and `content` may not add a heading at the section's own level or above (such revisions are refused with code `revision_invalid`). `reason` is required: draft it yourself from what you changed, in a short clause, since it becomes the daily-log line `revised [[path#Section]] \u{2014} reason` (`revised [[path]] \u{2014} reason` for a whole body). Text identical to the note's current text writes and logs nothing and returns `changed: false`. The result's `new_hash` is the note's hash after the call: pass it as `expected_hash` on a follow-up revision."
    )]
    pub async fn revise_note(
        &self,
        Parameters(input): Parameters<ReviseNoteInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        // The domain refuses a blank reason too, but in its own words; the
        // agent should hear it from the tool, naming the tool's field.
        if input.reason.trim().is_empty() {
            return Err(invalid_argument(
                "reason",
                "required: say briefly why the note was revised; it becomes the daily-log line",
            ));
        }
        let revision = match (input.body, input.section, input.content) {
            (Some(_), Some(_), _) => {
                return Err(invalid_argument(
                    "body",
                    "pass either `body` (whole-body rewrite) or `section` with `content`, not both",
                ));
            }
            (None, None, None) => {
                return Err(invalid_argument(
                    "body",
                    "pass either `body` (whole-body rewrite) or `section` with `content`",
                ));
            }
            (Some(_), None, Some(_)) | (None, None, Some(_)) => {
                return Err(invalid_argument(
                    "content",
                    "`content` goes with `section`; for a whole-body rewrite pass `body` alone",
                ));
            }
            (None, Some(_), None) => {
                return Err(invalid_argument(
                    "content",
                    "`section` requires `content`, the section's new text without its heading",
                ));
            }
            (Some(body), None, None) => {
                // A blank hash is a missing hash: sent to the domain it would
                // come back as `stale_revision`, whose "read it again" advice
                // an agent that sent "" by mistake would follow in a loop.
                if input
                    .expected_hash
                    .as_deref()
                    .is_none_or(|hash| hash.trim().is_empty())
                {
                    return Err(invalid_argument(
                        "expected_hash",
                        "required with `body`: pass the `content_hash` from `read_note`, so a \
                         change made since that read is refused rather than overwritten",
                    ));
                }
                Revision::Body(body)
            }
            // Trimmed like `note`: headings are matched as trimmed text, so
            // a padded heading would miss the real section and be appended
            // as a duplicate of it.
            (None, Some(heading), Some(content)) => Revision::Section {
                heading: heading.trim().to_owned(),
                content,
            },
        };
        // A section upsert is not hash-guarded (the rest of the note is
        // left alone), so a hash passed with it is ignored, not refused.
        let expected_hash = match revision {
            Revision::Body(_) => input.expected_hash,
            Revision::Section { .. } => None,
        };
        let reference = input.note.trim().to_owned();
        refuse_reference_outside_the_vault(&reference)?;
        let today = at.date();
        let reason = input.reason;
        let outcome = self
            .with_vault(move |vault| {
                Ok::<_, DomainError>(match resolve_note_reference(vault, &reference, today)? {
                    Ok(path) => Ok(vault.revise_note(
                        &path,
                        expected_hash.as_deref(),
                        revision,
                        &reason,
                        at,
                    )?),
                    Err(rejection) => Err(rejection),
                })
            })
            .await?
            .map_err(into_mcp_error)??;
        if !outcome.changed {
            return json_result(ReviseNoteResponse::from(outcome));
        }
        let path = outcome.path.clone();
        let response = ReviseNoteResponse::from(outcome);
        self.verified_write_with(path, WriteShape::Rewritten, move |verification| {
            ReviseNoteResponse {
                verification: Some(verification),
                ..response
            }
        })
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
        description = "Start work on an action that is ALREADY on the project map: matches the open bullet by substring `query` and logs `- **HH:MM**: started [[project]] — <bullet>` to today's daily note. `current_focus` requires that whole shape -- the `- **HH:MM**: ` stamp AND the em dash (U+2014) -- so a line composed by hand without both is invisible to it. What gets logged is the RESOLVED bullet text, not your query, so the later `complete_action` or `drop_action` logs matching text and `current_focus` clears. One exception: `promote_action` REWRITES the bullet it matches, so promoting between the start and the close strands the focus until the day rolls over, and the close verbs then match nothing. Errors with `INTERNAL_ERROR` when `query` matches no open bullet, and on an ambiguous match (several bullets contain it) -- the message lists the candidates; re-call with enough text to pick one. For work that is NOT on the map yet, use `start_unplanned_action` instead: this tool will not create a bullet, deliberately, because a fallback would turn a typo into a new action silently."
    )]
    pub async fn start_action(
        &self,
        Parameters(input): Parameters<StartActionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .with_vault(move |vault| vault.start_action(at, &input.project, &input.query))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Started action, logged to {}", path);
        self.verified_write(
            path,
            message,
            WriteShape::AppendedToSection(cdno_domain::DAILY_LOGS_SECTION),
        )
        .await
    }

    #[tool(
        description = "Start work that is on NO project map yet: appends the action to the project's `## Next Actions` AND logs it as started, in one commit, so it becomes ordinary planned work the moment it begins and can be closed by `complete_action` or `drop_action` like any other -- with the same two limits every bullet has: a `promote_action` in between rewrites the bullet and strands the focus, and a `title` that duplicates an open bullet's text makes both unresolvable by substring (check the map first if the work may already be listed). Use this when the person is already doing something that was never planned -- the fix they noticed, the errand in front of them. It is a separate tool from `start_action` on purpose: routing a non-matching query into creation would turn a typo into a new action silently. Two lines land in the daily log, `action added to ...` then `started ...`, so the bullet never appears on the map without a trace of where it came from. `energy` is one of `\"deep\"`, `\"medium\"`, `\"light\"`."
    )]
    pub async fn start_unplanned_action(
        &self,
        Parameters(input): Parameters<StartUnplannedActionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let energy = EnergyLevel::from_str(&input.energy)
            .map_err(|e| invalid_argument("energy", &e.to_string()))?;
        let at = chrono::Local::now().naive_local();
        let path = self
            .with_vault(move |vault| {
                vault.start_unplanned_action(at, &input.project, &input.title, energy)
            })
            .await?
            .map_err(into_mcp_error)?
            .primary;
        let message = format!("Added to {} and started", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Complete an action: matches the bullet on the project by substring `query`, removes the bullet, logs the completion to today's daily, and (if an action note is attached) archives it to `actions/_done/<year>/`. Use this ONLY when the work was actually performed. If it was superseded, abandoned or reprioritised, use `drop_action` instead -- completing it writes `action done on ...` into the daily log, which every weekly and monthly review reads back from, so the vault would assert work nobody did."
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
        description = "Drop an action: closes it WITHOUT recording it as done. Use this whenever an action is superseded, abandoned or reprioritised -- `complete_action` writes `action done on ...` into the daily log, which is the record every weekly and monthly review reads back from, so completing something that was never performed makes the vault assert work that did not happen. Matches the bullet by substring `query` exactly as `complete_action` does, removes it, logs `action dropped on ...`, and (if an action note is attached) archives it to `actions/_done/<year>/` stamped `status: dropped` with no completion date. Pass `reason` whenever you know it -- \"superseded by X\" and \"no longer wanted\" are different facts, and only one of them tells a later reader to look for a replacement."
    )]
    pub async fn drop_action(
        &self,
        Parameters(input): Parameters<DropActionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        // Only the primary project path is surfaced here; the outcome's
        // full touched set (which includes any archival move) is for the
        // desktop echo journal (#315), not the MCP reply.
        let path = self
            .with_vault(move |vault| {
                vault.drop_action(at, &input.project, &input.query, input.reason.as_deref())
            })
            .await?
            .map_err(into_mcp_error)?
            .primary;
        let message = format!("Dropped action on {}", path);
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
        description = "End an active commitment that was NOT kept -- cancelled, superseded, or overtaken. Stamps `status: dropped`, clears `completed`, moves the file to `commitments/_done/<year>/`, and logs `commitment dropped on [[slug]]` with an optional `reason` on a continuation line. The counterpart to `complete_commitment`: use that only when the promise was actually kept. A dropped commitment can afterwards be neither completed nor rescheduled, and never appears as completed work."
    )]
    pub async fn drop_commitment(
        &self,
        Parameters(input): Parameters<DropCommitmentInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .with_vault(move |vault| {
                vault.drop_commitment(at, &input.commitment, input.reason.as_deref())
            })
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Dropped commitment, archived to {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Move an active commitment's `due` date and log the move to today's daily entry, recording BOTH the old and new dates. Use this rather than deleting and recreating the note: recreating destroys the body (the notes on who was chased and why it moved) and resets `created`, the field that shows how long something has been slipping. The new date must differ from the current one. Moving a date earlier is allowed."
    )]
    pub async fn reschedule_commitment(
        &self,
        Parameters(input): Parameters<RescheduleCommitmentInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .with_vault(move |vault| vault.reschedule_commitment(at, &input.commitment, input.due))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Rescheduled commitment at {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Complete one occurrence of a stewardship's periodic commitment, rolling its `next:` date forward by that line's own recurrence and logging the completion with both dates. A periodic commitment is a bullet in a stewardship's `## Periodic Commitments`, not a note, so `complete_commitment` does not apply to it. `title` is a case-insensitive substring. Pass `at` for work finished on another day: the roll-forward is anchored to the DUE date either way, so completing early never drags the schedule earlier, and a late completion advances until the next date is in the future."
    )]
    pub async fn complete_periodic(
        &self,
        Parameters(input): Parameters<CompletePeriodicInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let now = chrono::Local::now().naive_local();
        let at = match input.at {
            Some(date) => date.and_time(now.time()),
            None => now,
        };
        let path = self
            .with_vault(move |vault| vault.complete_periodic(at, &input.stewardship, &input.title))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Completed periodic commitment, updated {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Mark an active commitment as completed: stamps the `status` and `completed` frontmatter fields, moves the file to `commitments/_done/<year>/`, and logs to today's daily entry. All in one atomic transaction. Use this ONLY when the promise was actually kept. If it was cancelled, superseded or overtaken, use `drop_commitment` instead -- completing it writes `commitment completed ...` into the daily log, which every weekly and monthly review reads back from, so the vault would assert a promise nobody kept."
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

    /// Append one `### <heading>` entry to a daily note's `## Notes` and
    /// its `noted [[…#<heading>]]` pointer to `## Logs`, in one domain
    /// transaction (RFC 0002 §5.4, §6.4; T10, #623).
    ///
    /// Blank `heading` or `body` (after trimming) is `INVALID_PARAMS`
    /// naming the field; every other refusal is the domain's
    /// (`history_entry_heading_invalid` for a bad, reused or duplicate
    /// heading) and reaches the client as a rejection unchanged. `date`
    /// omitted is now; a given date is stamped at the current local
    /// time, as `complete_periodic` does, so the pointer carries a time.
    ///
    /// Verified with [`WriteShape::AppendedToSection`] on `## Notes`
    /// rather than [`WriteShape::Rewritten`]: the verification re-reads
    /// the whole file either way, and the appended shape additionally
    /// carries back the tail of `## Notes`, which ends with the entry
    /// just written — the substance, the half a caller cannot see from
    /// the response's `log_line`. The pointer half is already in the
    /// response verbatim (`log_line`), so the tail need not show it.
    /// The heading comes from the domain (`DailySection::Notes`), not a
    /// literal here.
    ///
    /// The blank-body refusal is a check on the argument's shape, like
    /// the blank-heading one, not a rule of the operation: the domain
    /// accepts an empty body and writes a bare `### heading`, and a
    /// later CLI verb decides for itself whether to demand substance.
    #[tool(
        description = "Write worked-out substance (a derivation, a procedure, a page of reasoning) to a daily note (defaults to today) as one entry per call: `### <heading>` followed by `body`, appended under the day's `## Notes`. The pointer line `noted [[journal/<year>/daily/<date>#<heading>]] (<links>)`, listing the body's wikilinks, is written to `## Logs` for you in the same write, so do not log it again with `append_to_log`. Keep `## Logs` for one-line events and put the substance here. The heading must be unique within the day, must not reuse a daily section name (`Standup`, `Intention`, `Agenda`, `Meeting`, `Notes`, `Logs`), and must not contain `[`, `]`, `|`, `#` or inline markup (bold, italics, code) nor start with `^`; such headings are refused with code `history_entry_heading_invalid`. Headings inside the body must be level 3 or deeper (a `#` or `##` line is refused) and are held to the same uniqueness rule as the entry heading, so a pasted derivation with its own `## Proof` must be demoted first. Wikilink the vault notes the body names (`[[slug]]`). End an entry that could be reused beyond today with the tag `#concept` on the body's last line, so the review can find it as a candidate for promotion to a concept note. The returned `target` is the entry's anchored link: cite it as `[[<target>]]`, for example from a concept's `origin`."
    )]
    pub async fn note_to_daily(
        &self,
        Parameters(input): Parameters<NoteToDailyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let now = chrono::Local::now().naive_local();
        let at = match input.date {
            Some(date) => date.and_time(now.time()),
            None => now,
        };
        let heading = input.heading.trim().to_owned();
        if heading.is_empty() {
            return Err(invalid_argument(
                "heading",
                "required: the entry's heading, written as `### <heading>` under `## Notes`",
            ));
        }
        // Only leading blank lines and trailing whitespace are stripped:
        // the first line's indentation is part of the substance (an
        // entry may open with an indented code block), and the domain
        // writes the body verbatim.
        let body = input
            .body
            .trim_start_matches(['\n', '\r'])
            .trim_end()
            .to_owned();
        if body.trim().is_empty() {
            return Err(invalid_argument(
                "body",
                "required: the entry's substance; a one-line event belongs in `append_to_log`",
            ));
        }
        let outcome = self
            .with_vault(move |vault| vault.note_to_daily(at, &heading, &body))
            .await?
            .map_err(into_mcp_error)?;
        let path = outcome.path.clone();
        let response = NoteToDailyResponse::from(outcome);
        self.verified_write_with(
            path,
            WriteShape::AppendedToSection(DailySection::Notes.heading()),
            move |verification| NoteToDailyResponse {
                verification: Some(verification),
                ..response
            },
        )
        .await
    }

    #[tool(
        description = "Write a section of the daily note (defaults to today). `section` is one of `Standup`, `Intention`, `Agenda`, `Meeting`, `Notes` (case-insensitive); any other value is rejected as an invalid argument. With `append: false` (default) the section is replaced (the planning sections); with `append: true` the content is appended (live meeting notes that accrue). `Notes` is append-only: it takes `append: true` only, and `append: false` is refused; to add one entry to it, use `note_to_daily`, which also writes the pointer line to `## Logs`. `## Logs` is not writable here at all -- it grows via `append_to_log`. Creates the section (and the daily note) if absent. An empty `content` with `append: false` clears the section to just its heading. The prose written here follows the same linking convention as the log: wikilink the vault notes it names (`[[slug]]`) and give forge references markdown links, never a bare `#N`."
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
        // The domain refuses this too (`history_section_not_replaceable`),
        // but it is an argument-shape mistake the tool can name, with the
        // tool that does what the caller most likely meant.
        if section.is_history() && !input.append {
            return Err(invalid_argument(
                "append",
                &format!(
                    "`{}` is append-only: pass `append: true` to append to it; to add one \
                     entry, use `note_to_daily`, which also writes its pointer line to `## Logs`",
                    section.heading()
                ),
            ));
        }
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
        description = "Write a section of the monthly note for the calendar month containing `date` (any day in the month; defaults to this month). `section` is one of `Wins`, `Themes`, `Next Month's Focus` (case-insensitive); any other value is rejected. Creates the monthly note (frontmatter + the three section headings + a `## Weeks` block linking the month's weekly notes) if absent. With `append: false` (default) the section is replaced — compose the review; with `append: true` the content is appended — accrue within a section across a session. The monthly note links (never copies) its weeks, so the weekly notes stay the source of truth; there is no Metrics section — quantitative metrics are not note content, so never add one."
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
