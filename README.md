# Cuaderno

[![CI](https://github.com/agustinvalencia/cuaderno/actions/workflows/ci.yml/badge.svg)](https://github.com/agustinvalencia/cuaderno/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/agustinvalencia/cuaderno/graph/badge.svg?token=8J5LGNS1TN)](https://codecov.io/gh/agustinvalencia/cuaderno)
[![License: MPL 2.0](https://img.shields.io/badge/License-MPL_2.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)

A vault management tool implementing the **Research Logbook Method** (RLM) — a system for knowledge, tasks, and life organisation designed for experimental researchers, with specific accommodations for ADHD.

**Command**: `cdno` (alias: `cdrn`)
**Full name**: cuaderno ("notebook" / "logbook" in Spanish)

## Getting Started

### Install

**Homebrew** (macOS + Linux, recommended):

```bash
brew install agustinvalencia/tap/cuaderno
```

That installs both binaries — `cdno` (the CLI for the daily loop) and `cdno-mcp` (the MCP server for Claude / Kiro / Gemini CLI). Pre-built bottles for macOS arm64 + intel and Linux x86_64 + aarch64.

**From source** (everywhere else, or if you want to track `main`):

```bash
git clone https://github.com/agustinvalencia/cuaderno
cd cuaderno
cargo build --release --bins
# Binaries land at target/release/cdno and target/release/cdno-mcp.
# Symlink both somewhere on your PATH, for example:
ln -s "$PWD/target/release/cdno" /usr/local/bin/cdno
ln -s "$PWD/target/release/cdno-mcp" /usr/local/bin/cdno-mcp
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

### Connect to Claude (MCP server)

Cuaderno ships an MCP server so Claude Desktop / Claude Code / any MCP-aware agent can read and write the vault. The server is a separate binary, `cdno-mcp`, that talks JSON-RPC over stdio. It's already on your PATH if you installed via Homebrew or symlinked the `from source` build above.

Wire it into Claude Desktop / Claude Code with:

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

The vault path can also be omitted; the server then opens whichever vault the working directory belongs to.

**Tool surface today.** Three context-gathering tools are wired through to the domain — `get_orientation`, `get_active_questions`, `get_portfolio_contents`. Twelve more (the rest of the design §11 catalogue) are advertised with schemas but return a "not yet implemented" error when called; see [`STATUS.md`](STATUS.md) for the per-tool status.

### Where to go next

- **[`STATUS.md`](STATUS.md)** — current development status, phase by phase, with PR links.
- **[`docs/design.md`](docs/design.md)** — full specification of the note types, folder structure, the RLM rationale, and the CLI / MCP / UI surfaces.
- **[`docs/cli-ergonomics.md`](docs/cli-ergonomics.md)** — the flags-and-prompts convention every mutating verb follows. Useful when scripting, in CI, or when an interactive prompt isn't where you'd expect.
- **[`docs/implementation-plan.md`](docs/implementation-plan.md)** — architecture, trait landscape, and the phased build sequence.
- **[`CHANGELOG.md`](CHANGELOG.md)** — what's landed per PR since the start.

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
│   ├── cdno-mcp/           ← MCP server — stdio shipped, HTTP planned (Phase 6)
│   └── cdno-tauri/         ← Tauri backend for the desktop app (Phase 5, not yet created)
├── ui/                     ← React + Tremor frontend (Phase 5, not yet created)
└── skills/                 ← Claude skill definitions (Phase 4 skill adaptation, not yet created)
```

```
cdno-core → cdno-domain → cdno-cli
                        → cdno-mcp → stdio transport (shipped)
                                   → HTTP transport (Axum, Phase 6)
                        → cdno-tauri (Phase 5) → React UI
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

Phases 1, 2, and 3 of [the build sequence](docs/implementation-plan.md) are complete; Phase 4 is in progress. **The CLI is daily-usable end-to-end** — every note type (projects, actions, commitments, portfolios + evidence, questions, stewardships + tracking + periodic commitments) is reachable from the terminal with the flags-and-prompts ergonomics from [`docs/cli-ergonomics.md`](docs/cli-ergonomics.md). The aggregated `cdno orient` / `cdno status` / `cdno commitments` views compose across every source.

The MCP server (Phase 4) is scaffolded with the full 16-tool catalogue advertised; three context tools are wired through to the domain (`get_orientation`, `get_active_questions`, `get_portfolio_contents`), the remaining 13 return "not yet implemented" until #46-follow-ups and #47 land. The Tauri desktop UI (Phase 5) is planned but not yet started.

See **[`STATUS.md`](STATUS.md)** for the per-phase and per-issue breakdown, and **[`CHANGELOG.md`](CHANGELOG.md)** for what's shipped per PR.

## Acknowledgements

- The planning a task management has carried out mostly by Claude with `gh cli`
- The software has been mostly architected by me, with second opinions of Claude
- Certainly some implementation tasks will be delegated to Claude, though it will never auto-push. All code will be reviewed and assessed by me.

## Licence

[MPL-2.0](LICENSE)
