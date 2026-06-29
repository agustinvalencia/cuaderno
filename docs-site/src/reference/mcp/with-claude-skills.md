# Using with Claude skills

The MCP tools are building blocks. A **skill** composes them into a repeatable ritual the assistant
can run on request — "do my daily orientation," "walk me through the weekly review." Cuaderno ships
worked examples you can adapt.

## Where the examples live

The repo's [`examples/skills/`](https://github.com/agustinvalencia/cuaderno/tree/main/examples/skills)
directory contains two reference skills:

- **`quick-capture`** — capture a thought into the inbox with minimal friction.
- **`daily-orientation`** — read your orientation and help set the day's intention.

Each is a directory with a `SKILL.md` (YAML frontmatter + a Markdown body of steps), plus shared
`references/`. The README there documents the authoring pattern.

## The pattern

A skill typically:

1. **Surfaces context** with a read tool — e.g. `get_orientation` or `get_weekly_context`.
2. **Talks it through** with you, deciding what to do.
3. **Writes** with the matching tool — e.g. `append_to_log`, `upsert_daily_section`,
   `update_project_state`.

For example, a *daily orientation* skill calls `get_orientation`, presents the commitments and the
suggested start, asks for your intention, and writes it with
`upsert_daily_section(section="Intention", ...)`.

## Graceful degradation

Because each step maps to a discrete tool, a skill can degrade gracefully — if a write tool isn't
available or you decline it, the read half still gives you the briefing. The example skills show how
to bind steps to tools and reference shared material.

## Build your own

Start from an example, swap in the tools your ritual needs (see
[reads](reads.md), [writes](writes.md), [creation & lifecycle](creation-and-lifecycle.md)), and keep
each step mapped to one tool so the flow stays inspectable. Then install it like any other Claude
skill.

## See also

- [Connect to Claude](../../getting-started/connect-to-claude.md) — register the server.
- [MCP server reference](overview.md) — the full tool surface.
