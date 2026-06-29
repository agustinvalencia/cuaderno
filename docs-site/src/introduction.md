# Cuaderno

**Cuaderno** (`cdno`) is a command-line vault manager for the **Research Logbook Method (RLM)** — a
way of running long-horizon knowledge work out of a folder of plain Markdown files. It is built for
researchers and other deep-work practitioners, and is deliberately friendly to ADHD working styles:
it leads with *what is there*, not what is missing — no guilt counters, no angry overdue badges,
explicit permission to park or drop work.

You drive it from a terminal, and an AI assistant (Claude and other MCP clients) can drive it too,
through the bundled MCP server.

## What it gives you

- A dated, append-only **log** as the single source of truth for what you did.
- **Projects** with a current state, next actions, milestones, and waiting-on items — capped at five
  active at a time so you can't overcommit.
- **Evidence portfolios**: per-question dossiers that accumulate papers, experiment results, and
  notes over months and years.
- **Important questions** kept front-and-centre and re-read often.
- **Stewardships**: small, bounded, long-haul responsibilities (health, finances) with optional
  habit tracking.
- A **commitments register** — promises with dates, aggregated from everywhere they live.
- Full-text **search**, frontmatter **linting**, and a **JSON** mode so every read and write verb is
  scriptable.

## Two ways to use it

| Surface | What it is | Start here |
|---------|-----------|------------|
| **`cdno` CLI** | The terminal tool for the daily loop | [Quickstart](getting-started/quickstart.md) |
| **`cdno-mcp` server** | An MCP server so Claude can read and write your vault | [Connect to Claude](getting-started/connect-to-claude.md) |

Both operate on the same Markdown vault — the files on disk are always the source of truth (the
SQLite index is just a rebuildable cache).

## How this guide is organised

- **[Getting started](getting-started/installation.md)** — install, create a vault, run the daily
  loop, and wire up Claude.
- **[Concepts](concepts/rlm.md)** — the mental model: the method, the ten note types, the vault
  layout, and the rules the tool enforces.
- **[Tutorials](tutorials/daily-loop.md)** — task-oriented walkthroughs of each workflow.
- **[CLI reference](reference/cli/overview.md)** — every command, flag, and example.
- **[MCP server reference](reference/mcp/overview.md)** — every tool exposed to AI clients.
- **[Appendix](reference/json-output.md)** — JSON shapes, frontmatter fields, the full config file,
  recurrence syntax, and troubleshooting.

> This guide documents the shipped behaviour of Cuaderno. New to it? Read
> [The Research Logbook Method](concepts/rlm.md) for the *why*, then jump to the
> [Quickstart](getting-started/quickstart.md) for the *how*.
