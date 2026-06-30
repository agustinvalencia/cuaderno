//! Structural-creation tool handlers. Part of the handler-group split: a separate
//! `#[tool_router(router = creation_router)]` impl merged into the dispatch
//! table in `CuadernoServer::new`.

use std::str::FromStr;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorData};
use rmcp::{tool, tool_router};

use cdno_domain::frontmatter::{Context, QuestionDomain};

use crate::dto::WriteResultDto;

use crate::input::*;

use crate::util::{into_mcp_error, invalid_argument, json_result};

use crate::server::CuadernoServer;

#[tool_router(router = creation_router, vis = "pub")]
impl CuadernoServer {
    #[tool(
        description = "Create a new project map. Below the active-project cap (default 5) the project is created active; at or above the cap it's created parked (`projects/_parked/<slug>`) so you can capture it without parking another first — the cap is enforced on activation, not creation. `context` is a kebab-case Context (`work`, `household`, `personal`, …). `core_question` is an optional bare wikilink target (e.g. `questions/research/foo`) linking the project to the question it answers."
    )]
    pub async fn create_project(
        &self,
        Parameters(input): Parameters<CreateProjectInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let today = chrono::Local::now().date_naive();
        let context = Context::from_str(&input.context)
            .map_err(|e| invalid_argument("context", &e.to_string()))?;
        let vars = input.vars.unwrap_or_default();
        let path = self
            .vault
            .create_project_with_vars(
                today,
                &input.title,
                context,
                input.core_question.as_deref(),
                &vars,
            )
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created project at {}", path),
        ))
    }

    #[tool(
        description = "Create a portfolio (evidence folder + `_index.md`) for a question or topic. `project` optionally links it to a project — pass the bare wikilink target (e.g. `projects/surrogate-model`); resolve a real one (e.g. via `get_orientation`) rather than inventing it. When that project note exists, its `## Links` is backfilled with the portfolio in the same commit (and the portfolio's `project:` frontmatter is set either way); an unknown target sets the frontmatter only, not rejected. If a research/life question note already exists for the same `question` text, the two are linked both ways in the same commit — the question's `## Related Portfolios` gains the new portfolio and the portfolio's `## Related Questions` gains the question (pass the question's text verbatim so the slugs match). Use `link_portfolio_to_question` / `link_portfolio_to_project` to wire them when the slugs differ or the portfolio already exists."
    )]
    pub async fn create_portfolio(
        &self,
        Parameters(input): Parameters<CreatePortfolioInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let vars = input.vars.unwrap_or_default();
        let path = self
            .vault
            .create_portfolio_with_vars(at, &input.question, input.project.as_deref(), &vars)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created portfolio at {}", path),
        ))
    }

    #[tool(
        description = "Link an existing portfolio to an existing question, writing both ends in one commit: the question note's `## Related Portfolios` gains `[[portfolios/<slug>/_index]]` and the portfolio's `## Related Questions` gains `[[questions/<domain>/<slug>]]`. Both arguments are slugs (not free text). Use this to retrofit a portfolio created before its question, or when their slugs differ; `create_portfolio` already links automatically when they match. Idempotent on each end — re-linking never duplicates a bullet."
    )]
    pub async fn link_portfolio_to_question(
        &self,
        Parameters(input): Parameters<LinkPortfolioToQuestionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = self
            .vault
            .link_portfolio_to_question(&input.portfolio, &input.question)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!(
                "Linked portfolio '{}' to question '{}' ({})",
                input.portfolio, input.question, path
            ),
        ))
    }

    #[tool(
        description = "Link an existing portfolio to an existing project, writing both directions in one commit: the portfolio's `project:` frontmatter is set to `[[<project>]]` and the project map's `## Links` gains `[[portfolios/<slug>/_index]]` (replacing the `(none yet)` placeholder on the first link). `portfolio` is a slug; `project` is the bare wikilink target (e.g. `projects/surrogate-model`, no `[[ ]]`). Both must already exist. Use this to retrofit a portfolio created before its project, created without one, or whose `## Links` predates the auto-backfill; `create_portfolio` already backfills when a project is given. Idempotent — re-linking never duplicates the bullet."
    )]
    pub async fn link_portfolio_to_project(
        &self,
        Parameters(input): Parameters<LinkPortfolioToProjectInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = self
            .vault
            .link_portfolio_to_project(&input.portfolio, &input.project)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!(
                "Linked portfolio '{}' to project '{}' ({})",
                input.portfolio, input.project, path
            ),
        ))
    }

    #[tool(
        description = "Create a research or life question note. `domain` is `research` or `life`."
    )]
    pub async fn create_question(
        &self,
        Parameters(input): Parameters<CreateQuestionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let domain = QuestionDomain::from_str(&input.domain)
            .map_err(|e| invalid_argument("domain", &e.to_string()))?;
        let vars = input.vars.unwrap_or_default();
        let path = self
            .vault
            .create_question_with_vars(at, domain, &input.text, &vars)
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created question at {}", path),
        ))
    }

    #[tool(
        description = "Create a stewardship. With `expanded: true` it's a folder stewardship (`stewardships/<slug>/_index.md` with a lazy `tracking/`); otherwise a flat file. `context` is a kebab-case Context."
    )]
    pub async fn create_stewardship(
        &self,
        Parameters(input): Parameters<CreateStewardshipInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let context = Context::from_str(&input.context)
            .map_err(|e| invalid_argument("context", &e.to_string()))?;
        let vars = input.vars.unwrap_or_default();
        let path = if input.expanded {
            self.vault
                .create_stewardship_expanded_with_vars(at, &input.name, context, &vars)
                .map_err(into_mcp_error)?
        } else {
            self.vault
                .create_stewardship_flat_with_vars(at, &input.name, context, &vars)
                .map_err(into_mcp_error)?
        };
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created stewardship at {}", path),
        ))
    }
}
