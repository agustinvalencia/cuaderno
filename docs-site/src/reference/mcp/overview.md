# MCP server reference

`cdno-mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server that exposes your
vault to AI clients (Claude Desktop, Claude Code, Kiro, Gemini CLI, …). It runs the same domain
engine as the CLI, so anything the assistant does goes through the same rules and lands in the same
Markdown files. To wire it up, see [Connect to Claude](../../getting-started/connect-to-claude.md).

## Transport and vault selection

Two binaries serve the same tool catalogue:

- **`cdno-mcp`** speaks JSON-RPC over **stdio** — the client launches it as a subprocess. It opens
  the vault named by `CUADERNO_VAULT_PATH`, or, if that's unset, discovers one from its working
  directory (the same rule as the CLI).
- **`cdno-mcp-server`** speaks MCP **Streamable HTTP** for remote clients — see
  [The HTTP server](http-server.md) for its flags and security model.

## The tool surface

The server advertises **42 tools**. This reference groups them by purpose:

| Group | Page | What's in it |
|-------|------|--------------|
| Context-gathering reads | [Context-gathering tools](reads.md) | Orientation, project/portfolio/weekly context, search, reads, lint, triage list |
| Writes | [Write tools](writes.md) | Log, capture, file evidence, project/action/milestone/waiting edits, commitments, tracking, daily/weekly sections |
| Creation & lifecycle | [Creation and lifecycle tools](creation-and-lifecycle.md) | Create projects/portfolios/questions/stewardships, link portfolios, park/activate, status transitions |

Every tool returns typed JSON; the shapes mirror the CLI's [`--json`](../json-output.md) output, so a
client gets the same structures whichever surface it uses.

## Conventions

- **Slugs, not paths.** Tools take slugs (`surrogate-model`), matching the CLI.
- **Substring matching** for completing actions/milestones and resolving waiting-on items, exactly as
  on the CLI.
- **The same rules apply.** The five-project cap, append-only notes, auto-logged project-state
  history, and commitments aggregation all hold — the MCP server is not a back door around them.

## Building skills on top

Multi-step rituals (a morning orientation, a guided weekly review) are best wrapped as **Claude
skills** that call these tools in sequence. See [Using with Claude skills](with-claude-skills.md).
