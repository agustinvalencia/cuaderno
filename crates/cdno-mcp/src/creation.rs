//! Structural-creation tool handlers. Part of the handler-group split: a separate
//! `#[tool_router(router = creation_router)]` impl merged into the dispatch
//! table in `CuadernoServer::new`.

use std::str::FromStr;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorData};
use rmcp::{tool, tool_router};

use cdno_domain::error::DomainError;
use cdno_domain::frontmatter::{Context, QuestionDomain};

use crate::input::*;

use crate::util::{into_mcp_error, invalid_argument};

use crate::server::CuadernoServer;
use crate::verify::WriteShape;

#[tool_router(router = creation_router, vis = "pub")]
impl CuadernoServer {
    #[tool(
        description = "Create a new project map for a piece of *finite* work — something with a deliverable that can be finished. If the thing never completes (health, finances, household, a recurring service), it is a stewardship: use create_stewardship, because active projects are capped and a perpetual responsibility would consume a slot permanently. Below the active-project cap (default 5) the project is created active; at or above the cap it's created parked (`projects/_parked/<slug>`) so you can capture it without parking another first — the cap is applied on activation, not creation. `context` is a kebab-case Context (`work`, `household`, `personal`, …). `core_question` is an optional bare wikilink target (e.g. `questions/research/foo`) linking the project to the question it answers."
    )]
    pub async fn create_project(
        &self,
        Parameters(input): Parameters<CreateProjectInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let context = Context::from_str(&input.context)
            .map_err(|e| invalid_argument("context", &e.to_string()))?;
        let path = self
            .with_vault(move |vault| {
                let vars = input.vars.unwrap_or_default();
                vault.create_project_with_vars(
                    at,
                    &input.title,
                    context,
                    input.core_question.as_deref(),
                    &vars,
                )
            })
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Created project at {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Create a portfolio — a dossier that accumulates evidence for one question over months. Reach for this when you are gathering material rather than delivering an outcome; delivering is a project. Do not organise inside a portfolio, and file evidence as it arises. Creates an evidence folder + `_index.md` for a question or topic. `project` optionally links it to a project — pass the bare wikilink target (e.g. `projects/surrogate-model`); resolve a real one (e.g. via `get_orientation`) rather than inventing it. When that project note exists, its `## Links` is backfilled with the portfolio in the same commit (and the portfolio's `project:` frontmatter is set either way); an unknown target sets the frontmatter only, not rejected. If a research/life question note already exists for the same `question` text, the two are linked both ways in the same commit — the question's `## Related Portfolios` gains the new portfolio and the portfolio's `## Related Questions` gains the question (pass the question's text verbatim so the slugs match). Use `link_portfolio_to_question` / `link_portfolio_to_project` to wire them when the slugs differ or the portfolio already exists."
    )]
    pub async fn create_portfolio(
        &self,
        Parameters(input): Parameters<CreatePortfolioInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .with_vault(move |vault| {
                let vars = input.vars.unwrap_or_default();
                vault.create_portfolio_with_vars(
                    at,
                    &input.question,
                    input.project.as_deref(),
                    &vars,
                )
            })
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Created portfolio at {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Link an existing portfolio to an existing question, writing both ends in one commit: the question note's `## Related Portfolios` gains `[[portfolios/<slug>/_index]]` and the portfolio's `## Related Questions` gains `[[questions/<domain>/<slug>]]`. Both arguments are slugs (not free text). Use this to retrofit a portfolio created before its question, or when their slugs differ; `create_portfolio` already links automatically when they match. Idempotent on each end — re-linking never duplicates a bullet."
    )]
    pub async fn link_portfolio_to_question(
        &self,
        Parameters(input): Parameters<LinkPortfolioToQuestionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let portfolio = input.portfolio.clone();
        let question = input.question.clone();
        let path = self
            .with_vault(move |vault| vault.link_portfolio_to_question(&portfolio, &question))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!(
            "Linked portfolio '{}' to question '{}' ({})",
            input.portfolio, input.question, path
        );
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Link an existing portfolio to an existing project, writing both directions in one commit: the portfolio's `project:` frontmatter is set to `[[<project>]]` and the project map's `## Links` gains `[[portfolios/<slug>/_index]]` (replacing the `(none yet)` placeholder on the first link). `portfolio` is a slug; `project` is the bare wikilink target (e.g. `projects/surrogate-model`, no `[[ ]]`). Both must already exist. Use this to retrofit a portfolio created before its project, created without one, or whose `## Links` predates the auto-backfill; `create_portfolio` already backfills when a project is given. Idempotent — re-linking never duplicates the bullet."
    )]
    pub async fn link_portfolio_to_project(
        &self,
        Parameters(input): Parameters<LinkPortfolioToProjectInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let portfolio = input.portfolio.clone();
        let project = input.project.clone();
        let path = self
            .with_vault(move |vault| vault.link_portfolio_to_project(&portfolio, &project))
            .await?
            .map_err(into_mcp_error)?;
        let message = format!(
            "Linked portfolio '{}' to project '{}' ({})",
            input.portfolio, input.project, path
        );
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Create a research or life question note. A question is open-ended and sits *above* projects — it may spawn several over time and outlives them, so phrase it as a question that would change the situation if answered, not as a topic or a task. `domain` is `research` or `life`."
    )]
    pub async fn create_question(
        &self,
        Parameters(input): Parameters<CreateQuestionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let domain = QuestionDomain::from_str(&input.domain)
            .map_err(|e| invalid_argument("domain", &e.to_string()))?;
        let path = self
            .with_vault(move |vault| {
                let vars = input.vars.unwrap_or_default();
                vault.create_question_with_vars(at, domain, &input.text, &vars)
            })
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Created question at {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Create a note of a config-defined custom type (declared under `[note_types.<name>]` in the vault config; built-in types have their own dedicated create tools). `type_name` is the custom type; `fields` is a name -> value map of its declared frontmatter fields — each key must be a declared `required`/`optional` field. A required `title`, `slug`, `created` or `date` is filled in by the engine when omitted from `fields`, and may be overridden with a non-blank value (for example, to backdate `created`); every other required field must be supplied. The valid types and their fields come from the vault's config, not this schema. `body` is the note's markdown prose WITHOUT the title heading (the engine writes the `# <title>` H1 itself, and a leading H1 equal to the title is dropped): it fills the template's `{{body}}` placeholder, or is inserted after the H1 when the template has none. `origin` is one string of wikilinks to where the note came from, stored as a frontmatter string, e.g. `[[journal/2026/daily/2026-09-02#Woodbury identity]]` (promotion, RFC 0002 §5.5 — promoting a daily `## Notes` entry is creating the note with `origin`); it must be a field the type declares (the `concept` type declares it optional), else the call is refused. Creation is logged to today's daily note as `<type> created [[…]] — <title>`; do not log it again by hand, and a promotion needs no separate log line. For a `concept`, search before you create: run `search_notes` with `note_type: concept` and one or two distinctive words for the subject (terms are ANDed, so a long query misses), and if a note on the subject exists, refine it with `revise_note` rather than creating a second; if you find two already, tell the person rather than merging them yourself. Nothing on the server checks this: a second create with the same title succeeds as `<slug>-2`. Keep one concept per note: if the body needs two headings that could each be cited on their own, it is usually two notes. The `concept` template `cdno init` writes appends its own `## Statement`, `## Why it matters` and `## See also` after `body`, so do not repeat those headings in `body`: a duplicated heading makes `revise_note` refuse that section as `ambiguous_section`. A concept is understanding you will reuse independent of any deliverable (a theorem, a definition, a technique, a procedure); a dated observation bearing on a question is evidence, not a concept. If the type is not declared in this vault and is one the `cdno` binary ships (`concept`), the refusal names the command that installs it, `cdno config note-type install --name <name>`; there is no install tool, so tell the owner to run it rather than retrying."
    )]
    pub async fn create_custom_note(
        &self,
        Parameters(input): Parameters<CreateCustomNoteInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let type_name = input.type_name.clone();
        let path = self
            .with_vault(move |vault| {
                let vars = input.vars.unwrap_or_default();
                vault.create_custom_note_with_vars(
                    at,
                    &input.type_name,
                    &input.title,
                    &input.fields,
                    &vars,
                    input.body.as_deref(),
                    input.origin.as_deref(),
                )
            })
            .await?
            .map_err(custom_create_error)?;
        let message = format!("Created {} note at {}", type_name, path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }

    #[tool(
        description = "Create a stewardship — a dashboard for a perpetual responsibility such as health, finances, household or a recurring service. The distinction that matters: projects end, stewardships do not, and stewardships deliberately do not compete for the capped project slots. If the thing has a completion state, use create_project instead. With `expanded: true` it's a folder stewardship (`stewardships/<slug>/_index.md` with a lazy `tracking/`); otherwise a flat file. `context` is a kebab-case Context."
    )]
    pub async fn create_stewardship(
        &self,
        Parameters(input): Parameters<CreateStewardshipInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let context = Context::from_str(&input.context)
            .map_err(|e| invalid_argument("context", &e.to_string()))?;
        let path = self
            .with_vault(move |vault| {
                let vars = input.vars.unwrap_or_default();
                if input.expanded {
                    vault.create_stewardship_expanded_with_vars(at, &input.name, context, &vars)
                } else {
                    vault.create_stewardship_flat_with_vars(at, &input.name, context, &vars)
                }
            })
            .await?
            .map_err(into_mcp_error)?;
        let message = format!("Created stewardship at {}", path);
        self.verified_write(path, message, WriteShape::Rewritten)
            .await
    }
}

/// Translate a `create_custom_note` failure. A type that is not declared but
/// is bundled with `cdno` gets the command that installs it (RFC 0003 §4.5),
/// so an agent can tell the owner what to run; everything else is the
/// ordinary translation.
fn custom_create_error(e: DomainError) -> ErrorData {
    match &e {
        DomainError::UnknownNoteType { note_type }
            if cdno_core::paths::BUNDLED_NOTE_TYPE_NAMES.contains(&note_type.as_str()) =>
        {
            let command = format!("cdno config note-type install --name {note_type}");
            crate::rejection::reject(
                crate::rejection::RejectionCode::UnknownNoteType,
                format!(
                    "{e}: `{note_type}` ships with cdno but is not installed in this vault; \
                     run `{command}` to install it"
                ),
                serde_json::json!({ "note_type": note_type, "install_command": command }),
            )
        }
        _ => into_mcp_error(e),
    }
}
