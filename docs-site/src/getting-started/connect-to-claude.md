# Connect to Claude (MCP)

Cuaderno ships an [MCP](https://modelcontextprotocol.io) server, `cdno-mcp`, that exposes your vault
to AI clients — Claude Desktop, Claude Code, Kiro, Gemini CLI, and anything else that speaks MCP.
The assistant can then read context (your orientation, a project, a portfolio) and make writes
(append to the log, file evidence, complete an action) on your behalf.

It's the same engine as the CLI — the files on disk stay the source of truth.

## Register the server

Add `cdno-mcp` to your client's MCP configuration. For Claude Desktop, edit
`~/.claude/claude_desktop_config.json` (Claude Code: `~/.claude.json` or per-project `.mcp.json`):

```json
{
  "mcpServers": {
    "cuaderno": {
      "command": "cdno-mcp",
      "env": { "CUADERNO_VAULT_PATH": "/Users/you/notebook" }
    }
  }
}
```

- `command` must resolve on the client's `PATH`. If it doesn't, use the absolute path (e.g.
  `/opt/homebrew/bin/cdno-mcp`, or `target/release/cdno-mcp` for a source build).
- `CUADERNO_VAULT_PATH` tells the server which vault to open. You can omit it; the server then opens
  whichever vault its working directory belongs to (the same discovery rule as the CLI).

Restart the client. Cuaderno's tools then appear in the client's tool list.

## What the assistant can do

The server advertises **42 tools**, grouped by purpose:

- **Context-gathering reads** — `get_orientation`, `get_project_context`, `get_portfolio_contents`,
  `get_weekly_context`, `search_notes`, `read_daily_note`, and more.
- **Writes** — `append_to_log`, `file_to_portfolio`, `update_project_state`, `add_action`,
  `complete_action`, `create_commitment`, `create_tracking_entry`, the daily/weekly section
  writers, and others.
- **Creation and lifecycle** — `create_project`, `create_portfolio`, `create_question`,
  `create_stewardship`, `park_project`, `activate_project`, `set_question_status`, and so on.

The full catalogue, with each tool's inputs and output shape, is in the
[MCP server reference](../reference/mcp/overview.md).

## Skills

You can wrap common multi-step flows as **Claude skills** that call these tools — e.g. a daily
orientation that reads `get_orientation` and writes your intention with `upsert_daily_section`. The
repo ships worked examples under
[`examples/skills/`](https://github.com/agustinvalencia/cuaderno/tree/main/examples/skills); see
[Using with Claude skills](../reference/mcp/with-claude-skills.md).

## Next step

Learn the model behind it all: [The Research Logbook Method](../concepts/rlm.md).
