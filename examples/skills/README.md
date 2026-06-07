# Example skills

Two example [Claude Agent Skills](https://agentskills.io/specification) showing how to drive cuaderno's CLI and MCP surface from inside a skill. They're **illustrative**, not a shipped product surface — copy the patterns and adapt them to your own workflow.

## What's here

| Example | Shows |
|---------|-------|
| [`quick-capture`](quick-capture/SKILL.md) | The minimal shape — a single MCP tool (`append_to_log`), zero-friction, one job done well. |
| [`daily-orientation`](daily-orientation/SKILL.md) | The full pattern — several context reads (`get_orientation`, `get_weekly_context`, `read_daily_note`), writes (`upsert_daily_section`, `append_to_log`), a second MCP server (calendar) used with graceful degradation, and a multi-step interaction flow. |

`references/` holds the shared design notes both skills link to (interaction principles, calendar conventions, linking rules).

## Patterns worth copying

- **A "Surface notes" block.** State up front what the MCP can and can't do, so the steps never reference tools that don't exist. The cuaderno surface is deliberately lean — write the skill against what's actually there.
- **An MCP-tools table split by server.** Make the dependencies explicit, and mark which server each tool comes from.
- **Bind steps to real response shapes.** Reference actual DTO fields (`projects[].top_action.energy`, `commitments[].is_overdue`), not a sketch of them.
- **Degrade gracefully.** If an optional server (e.g. calendar) is unavailable, the skill continues instead of failing.
- **Shared `references/`.** Keep cross-cutting design (principles, conventions) in one place and link to it from each skill, rather than repeating it.

## Try them

```bash
ln -s "$(pwd)/quick-capture" ~/.claude/skills/quick-capture
ln -s "$(pwd)/daily-orientation" ~/.claude/skills/daily-orientation
```

Both require the `cdno-mcp` server configured against a cuaderno vault. `daily-orientation` additionally uses an `apple-calendar` MCP server, and skips the calendar step cleanly when it isn't present.

## Building your own

These examples are intentionally generic. A real personal workflow — your projects, calendars, and habits — is better kept in your own (likely private) skills repo, with these as a starting point for the structure.
