# Example skills

Three example [Claude Agent Skills](https://agentskills.io/specification) showing how to drive cuaderno's CLI and MCP surface from inside a skill. They're **illustrative**, not a shipped product surface — copy the patterns and adapt them to your own workflow.

## What's here

| Example | Shows |
|---------|-------|
| [`quick-capture`](quick-capture/SKILL.md) | The minimal shape — one job done well, zero-friction: `append_to_log` by default, `capture` for the inbox, and a return cue when a focus is open. |
| [`daily-orientation`](daily-orientation/SKILL.md) | The full pattern — several context reads (`get_orientation`, `get_weekly_context`, `read_daily_note`), writes (`upsert_daily_section`, and the focus tools `start_action`, `resume_action`, `switch_action`, `pause_action`), a second MCP server (calendar) used with graceful degradation, and a multi-step interaction flow. |
| [`weekly-review`](weekly-review/SKILL.md) | A reflective review over one week — `get_weekly_context` and `get_orientation` read together, writes split across two weekly notes (`upsert_weekly_section` into this week's note and next week's `This Week's Goal`), and a one-line recap with `append_to_log`. |

`references/` holds the shared design notes the skills link to (interaction principles, calendar conventions, linking rules).

`check-tool-names.py` asks the built `cdno-mcp` for its advertised catalogue and checks every backticked tool name in a skill against it (`python3 examples/skills/check-tool-names.py [SKILL.md ...]`, run from the repository root; with no argument it checks `weekly-review`).

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
ln -s "$(pwd)/weekly-review" ~/.claude/skills/weekly-review
```

All three require the `cdno-mcp` server configured against a cuaderno vault. `daily-orientation` additionally uses an `apple-calendar` MCP server, and skips the calendar step cleanly when it isn't present.

## Building your own

These examples are intentionally generic. A real personal workflow — your projects, calendars, and habits — is better kept in your own (likely private) skills repo, with these as a starting point for the structure.
