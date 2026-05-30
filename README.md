# Cuaderno

[![CI](https://github.com/agustinvalencia/cuaderno/actions/workflows/ci.yml/badge.svg)](https://github.com/agustinvalencia/cuaderno/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/agustinvalencia/cuaderno/graph/badge.svg?token=8J5LGNS1TN)](https://codecov.io/gh/agustinvalencia/cuaderno)
[![License: MPL 2.0](https://img.shields.io/badge/License-MPL_2.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)

A vault management tool implementing the **Research Logbook Method** (RLM) — a system for knowledge, tasks, and life organisation designed for experimental researchers, with specific accommodations for ADHD.

**Command**: `cdno` (alias: `cdrn`)
**Full name**: cuaderno ("notebook" / "logbook" in Spanish)

## Getting Started

### Install (from source)

Cuaderno is in active pre-release development; the only install path today is building from source. A Homebrew tap and pre-built binaries are planned but not yet shipped.

```bash
git clone https://github.com/agustinvalencia/cuaderno
cd cuaderno
cargo build --release
# The binary lands at target/release/cdno — symlink or copy it
# somewhere on your PATH, for example:
ln -s "$PWD/target/release/cdno" /usr/local/bin/cdno
```

Verify:

```bash
cdno --version
```

### Initialise a vault

```bash
cdno init ~/notebook   # creates the folder tree + .cuaderno/ config
cd ~/notebook
```

`init` lays down the full vault structure (journal, projects, commitments, actions, portfolios, …) and writes a local config at `.cuaderno/config.toml`. From inside any subdirectory `cdno` discovers the vault root automatically.

### The daily loop in five commands

Capture a project, queue an action, promise a deadline, run the morning view, mark something done:

```bash
# 1. Start a new project. With no flags, in a TTY, cdno prompts for
#    title and context; piping or `--no-interactive` requires every
#    flag explicitly. The same flag-or-prompt pattern applies to
#    every mutating verb — see docs/cli-ergonomics.md.
cdno project create --title "Surrogate model" --context work

# 2. Add a next action. `--note` creates a manifest action note
#    alongside the bullet for heavier multi-day work.
cdno action add --project surrogate-model \
                --title "Run feature set B on full geometry mesh" \
                --energy deep

# 3. Promise something with a deadline.
cdno commit create --title "Pay rent" --due 2026-06-01 --context personal

# 4. Morning orientation: commitments due in the next 48h plus
#    anything overdue, your active projects with their top next
#    action, and a suggested starting point. `--energy` biases the
#    suggestion toward a project whose top action matches.
cdno orient --energy deep

# 5. Mark an action done. Wikilinked bullets archive their attached
#    note to actions/_done/<year>/ automatically and the file is
#    locked against prefix edits by the append-only lint.
cdno action complete --project surrogate-model --query "feature set B"
```

For the full verb list: `cdno --help`, then `cdno <verb> --help`.

### Where to go next

- **[`docs/design.md`](docs/design.md)** — full specification of the note types, folder structure, the RLM rationale, and the CLI / MCP / UI surfaces.
- **[`docs/cli-ergonomics.md`](docs/cli-ergonomics.md)** — the flags-and-prompts convention every mutating verb follows. Useful when scripting, in CI, or when an interactive prompt isn't where you'd expect.
- **[`docs/implementation-plan.md`](docs/implementation-plan.md)** — architecture, trait landscape, and the phased build sequence.

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

Phase 2 of [the build sequence](docs/implementation-plan.md) is complete: the CLI is daily-usable end-to-end. Projects (create, state, milestones, waiting, park / activate), the action layer (bullets and the heavier manifest notes, with `add` / `promote` / `complete` / `list`), commitments (create, complete, aggregated timeline), the morning `cdno orient` and `cdno status` views, and the append-only-after-completion lint that protects archived action notes — all reachable from the terminal with the flags-and-prompts ergonomics from [`docs/cli-ergonomics.md`](docs/cli-ergonomics.md).

The MCP server (Phase 4) and the Tauri desktop UI (Phase 5) are scaffolded but not yet implemented.

## Acknowledgements

- The planning a task management has carried out mostly by Claude with `gh cli`
- The software has been mostly architected by me, with second opinions of Claude
- Certainly some implementation tasks will be delegated to Claude, though it will never auto-push. All code will be reviewed and assessed by me.

## Licence

[MPL-2.0](LICENSE)
