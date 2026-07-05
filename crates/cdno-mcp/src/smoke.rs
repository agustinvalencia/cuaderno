//! `SmokeServer` — a one-tool MCP handler holding **no vault handle**.
//!
//! Served by `cdno-mcp-server --smoke`, it lets the whole remote
//! pipeline (tunnel, OAuth proxy, origin JWT check, Claude connector
//! registration) be proven end-to-end with the *real binary* while the
//! vault stays entirely out of reach: this type has no field through
//! which vault data could ever flow, so a misconfiguration during
//! infra bring-up cannot expose a note. It stays wired in permanently
//! as the deployment's smoke-test fixture (GH #61).

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, ErrorData, Implementation, ProtocolVersion, ServerCapabilities,
    ServerInfo,
};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};

/// Argument for [`SmokeServer::echo`].
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EchoInput {
    /// Text to echo back verbatim.
    pub message: String,
}

/// The no-vault smoke-test handler. Mirrors [`crate::CuadernoServer`]'s
/// shape (cloneable struct + merged router) so the binary can serve
/// either through the same transport code path.
#[derive(Clone)]
pub struct SmokeServer {
    #[allow(dead_code)] // read by the #[tool_handler] proc-macro expansion
    tool_router: ToolRouter<Self>,
}

impl Default for SmokeServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router(router = smoke_router, vis = "pub")]
impl SmokeServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::smoke_router(),
        }
    }

    #[tool(
        description = "Echo the message back. Connectivity/auth smoke test — this server holds no vault handle and cannot read or write any note."
    )]
    pub async fn echo(
        &self,
        Parameters(input): Parameters<EchoInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(input.message)]))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SmokeServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default()
            .with_protocol_version(ProtocolVersion::default())
            .with_server_info(Implementation::new(
                "cdno-mcp-smoke",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Cuaderno smoke-test MCP server: a single `echo` tool and no vault \
                access. If you can call `echo`, transport and authentication work.",
            );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}
