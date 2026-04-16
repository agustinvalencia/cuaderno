# Cuaderno

[![CI](https://github.com/agustinvalencia/cuaderno/actions/workflows/ci.yml/badge.svg)](https://github.com/agustinvalencia/cuaderno/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/agustinvalencia/cuaderno/graph/badge.svg?token=8J5LGNS1TN)](https://codecov.io/gh/agustinvalencia/cuaderno)
[![License: MPL 2.0](https://img.shields.io/badge/License-MPL_2.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)

A vault management tool implementing the **Research Logbook Method** (RLM) — a system for knowledge, tasks, and life organisation designed for experimental researchers, with specific accommodations for ADHD.

**Command**: `cdno` (alias: `cdrn`)
**Full name**: cuaderno ("notebook" / "logbook" in Spanish)

## The Research Logbook Method

The RLM distils practices from Faraday, Darwin, Hamming, Knuth, and Tao into six pillars:

1. **Chronological log** — dated, append-only, single source of truth
2. **Evidence portfolios** — per-question dossiers accumulating over years
3. **Important questions** — Hamming weaponised, re-read often
4. **Project maps** — lightweight overviews (not Gantt)
5. **Stewardships** — small, bounded, long-haul responsibilities
6. **Commitments register** — promises to others with dates (not a todo list)

## Architecture

Five-crate Rust workspace:

```
cuaderno/
├── Cargo.toml              ← workspace root
├── crates/
│   ├── cdno-core/          ← file I/O, markdown parsing, SQLite indexing, file watching
│   ├── cdno-domain/        ← note types, business rules, queries, state transitions
│   ├── cdno-cli/           ← terminal commands (`cdno`)
│   ├── cdno-mcp/           ← MCP server (stdio + HTTP transports)
│   └── cdno-tauri/         ← Tauri backend for the desktop app
├── ui/                     ← React + Tremor frontend (Tauri)
└── skills/                 ← Claude skill definitions (markdown)
```

```
cdno-core → cdno-domain → cdno-cli
                        → cdno-mcp → stdio transport
                                   → HTTP transport (Axum)
                        → cdno-tauri → React UI
```

**cdno-core** has no domain knowledge — it handles markdown files with YAML frontmatter, section manipulation, SQLite indexing, and filesystem watching. Reusable in any markdown vault tool.

**cdno-domain** contains all RLM business logic. Defines note types, enforces rules (5-project cap, required frontmatter, enforced linking), implements queries (commitments aggregation, portfolio health), and handles state transitions. Pure logic — no file I/O, no networking.

**cdno-cli**, **cdno-mcp**, and **cdno-tauri** are thin translation layers that call domain methods through their respective protocols.

## Design Principles

- **Markdown files are the source of truth.** The SQLite index is a cache. If everything except the vault folder were deleted, the system could be rebuilt by reindexing.
- **Everything in one place, in open formats.** No proprietary formats, no cloud dependency.
- **Opinionated enforcement over flexibility.** Required frontmatter by note type, automatic scaffolding, validation, enforced linking patterns.
- **ADHD-friendly emotional design.** Lead with what is there, not what is missing. No guilt engines. No angry red overdue counts. Permission to park or drop things.
- **Minimal maintenance overhead.** If maintaining the system takes more than five minutes a day (outside the weekly review), something is wrong.

## Note Types

| Type | Description | Location |
|------|-------------|----------|
| `daily` | Chronological log entry (append-only) | `journal/daily/` |
| `weekly` | Weekly review artefact (append-only) | `journal/weekly/` |
| `project` | Mutable project dashboard (max 5 active) | `projects/` |
| `portfolio` | Index note for an evidence folder | `portfolios/*/` |
| `evidence` | Individual capture inside a portfolio | `portfolios/*/` |
| `stewardship` | Dashboard for a perpetual responsibility | `stewardships/` |
| `tracking` | Structured log entry for a stewardship | `stewardships/*/tracking/` |
| `question` | An important research or life question | `questions/` |
| `commitment` | A standalone promise with a hard deadline | `commitments/` |
| `inbox` | Uncategorised capture awaiting triage | `inbox/` |

## Consumers

The tool has four consumers:

- **The researcher** via the CLI in a terminal
- **The researcher** via the Cuaderno desktop UI (Tauri 2.0)
- **Claude** via the MCP server (stdio for local, HTTP for self-hosted)
- **Claude skills** as choreographed workflows combining MCP calls with ADHD-friendly interaction patterns

## Status

In early development — design and implementation planning are complete, initial crate scaffolding is next.

## Acknoledgements

- The planning a task management has carried out mostly by Claude with `gh cli`
- The software has been mostly architected by me, with second opinions of Claude
- Certainly some implementation tasks will be delegated to Claude, though it will never auto-push. All code will be reviewed and assessed by me.

## Licence

[MPL-2.0](LICENSE)
