# Using with Claude skills

The MCP tools are building blocks. A **skill** composes them into a repeatable ritual the assistant
can run on request — "do my daily orientation," "walk me through the weekly review." Cuaderno ships
worked examples you can adapt.

## Where the examples live

The repo's [`examples/skills/`](https://github.com/agustinvalencia/cuaderno/tree/main/examples/skills)
directory contains three reference skills:

- **`quick-capture`** — capture a thought into the inbox with minimal friction.
- **`daily-orientation`** — read your orientation and help set the day's intention.
- **`weekly-review`** — celebrate the week's wins first, scan stewardship habits, walk the projects,
  look two weeks ahead, and pick one goal for next week, written to the weekly notes.

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

A *weekly review* skill calls `get_weekly_context`, whose `projects` carry the
[review fields](reads.md#review-fields), and asks each project the question its relationship
implies rather than one question of all of them:

| The project | The question |
|---|---|
| Yours (`counts: true`) | Did it move? |
| Handed off, and `changed_this_week` | What came back, and what is waiting on your decision? |
| Handed off over an empty Waiting On (`waiting_on_empty`) | A name over an empty Waiting On is a parked map wearing a name: park it, or take it back? |
| Handed off, with `focus_days_while_held` of two or more | Offered as evidence: take it back, or change the name to whoever really has it? |

The skill asks and never concludes: the fields are evidence, and the person decides. It never
offers a hand-off as a way to make room at the cap. See
[Weekly review](../../tutorials/weekly-review.md#a-question-per-relationship).

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
