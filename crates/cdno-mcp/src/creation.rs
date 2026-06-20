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
        let path = self
            .vault
            .create_project(today, &input.title, context, input.core_question.as_deref())
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created project at {}", path),
        ))
    }

    #[tool(
        description = "Create a portfolio (evidence folder + `_index.md`) for a question or topic. `project` optionally links it to a project slug — resolve a real slug (e.g. via `get_orientation`) rather than inventing one; an unknown slug is written as a dangling link, not rejected. If a research/life question note already exists for the same `question` text, its `## Related Portfolios` section gets a backlink to the new portfolio in the same commit (pass the question's text verbatim so the slugs match)."
    )]
    pub async fn create_portfolio(
        &self,
        Parameters(input): Parameters<CreatePortfolioInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let path = self
            .vault
            .create_portfolio(at, &input.question, input.project.as_deref())
            .map_err(into_mcp_error)?;
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created portfolio at {}", path),
        ))
    }

    #[tool(
        description = "Link an existing portfolio to an existing question, adding a `[[portfolios/<slug>]]` backlink to the question note's `## Related Portfolios` section. Both arguments are slugs (not free text). Use this to retrofit a portfolio created before its question, or when their slugs differ; `create_portfolio` already backlinks automatically when they match. Idempotent — re-linking never duplicates the bullet."
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
        description = "Create a research or life question note. `domain` is `research` or `life`."
    )]
    pub async fn create_question(
        &self,
        Parameters(input): Parameters<CreateQuestionInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let at = chrono::Local::now().naive_local();
        let domain = QuestionDomain::from_str(&input.domain)
            .map_err(|e| invalid_argument("domain", &e.to_string()))?;
        let path = self
            .vault
            .create_question(at, domain, &input.text)
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
        let path = if input.expanded {
            self.vault
                .create_stewardship_expanded(at, &input.name, context)
                .map_err(into_mcp_error)?
        } else {
            self.vault
                .create_stewardship_flat(at, &input.name, context)
                .map_err(into_mcp_error)?
        };
        json_result(WriteResultDto::new(
            path.to_string(),
            format!("Created stewardship at {}", path),
        ))
    }
}
