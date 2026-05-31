# Cuaderno Status

Snapshot of development progress as of the most recent merge. For per-PR detail see [`CHANGELOG.md`](CHANGELOG.md); for the underlying plan see [`docs/implementation-plan.md`](docs/implementation-plan.md).

## Phase summary

| Phase | Scope | Status |
|-------|-------|--------|
| **1 — Foundation** | Workspace layout, `cdno-core` traits + impls (`VaultStore`, `VaultIndex`, transactions, reconciliation, markdown parsing, hashing), `cdno-domain` skeleton, basic CLI bootstrap | Complete |
| **2 — Daily loop** | Projects (5-cap, state, milestones, waiting-on, park/activate), actions (bullets + manifest notes, add/promote/complete/list), commitments (create/complete + aggregation timeline), orient/status/lint, flags-and-prompts ergonomics retrofit, append-only-after-completion lint | Complete |
| **3 — Knowledge & stewardship** | Portfolios + evidence (create, file, list, show), questions (CRUD + status transitions + grouped list), stewardships (flat + expanded, list, show, periodic commitments, tracking notes with built-in templates), `cdno track` | Complete |
| **4 — MCP server** | `cdno-mcp` crate on `rmcp`, full 16-tool schema catalogue, stdio binary | In progress — see table below |
| **5 — Tauri UI** | `cdno-tauri` backend, React frontend with Tremor, Home / Weekly / Commitments views | Not started |
| **6 — Extended UI + HTTP** | Monthly / Portfolio / Stewardship views, HTTP transport, periodic reconciliation | Not started |
| **7 — Migration** | `cdno migrate --from-mdv` interactive importer | Not started |

## Phase 4 detail

| Issue | Scope | Status | PR |
|-------|-------|--------|----|
| #45 | `cdno-mcp` crate scaffold on `rmcp`, all 16 tool schemas advertised, stdio binary | Complete | #140 |
| — | Doc tidy: implementation plan §5.2 updated for rmcp choice | Complete | #141 |
| #46 | `HandlerRegistry` + 7 context-gathering handlers | Partial — 3 of 7 handlers shipped (`get_orientation`, `get_active_questions`, `get_portfolio_contents`); registry covered by `#[tool_router]` macro | (this PR) |
| #142 | Remaining 4 context handlers + supporting domain queries (weekly/monthly context, project context, stewardship tracking) | Not started | — |
| #47 | 9 operation handlers (append_to_log, file_to_portfolio, update_project_state, add/promote/complete_action, create/complete_commitment, create_tracking_entry) | Complete | (this PR) |
| #48 | Stdio transport polish + Claude Desktop end-to-end test | Not started | — |
| #49 | File watcher integration for live external edits | Not started | — |
| #50 | Update existing Claude skills for the new domain model | Not started | — |
| #51 | Create new cuaderno-native skills (daily-orientation, weekly-review, monthly-review, file-to-portfolio, create-project, triage) | Not started | — |
| #52 | End-to-end skill testing through Claude Desktop | Not started | — |

## MCP tool surface — per-tool status

| Tool | Status |
|------|--------|
| `get_orientation` | Wired |
| `get_active_questions` | Wired |
| `get_portfolio_contents` | Wired |
| `get_weekly_context` | Stub (#142) |
| `get_monthly_context` | Stub (#142) |
| `get_project_context` | Stub (#142) |
| `get_stewardship_tracking` | Stub (#142) |
| `append_to_log` | Wired |
| `file_to_portfolio` | Wired |
| `update_project_state` | Wired |
| `add_action` | Wired |
| `promote_action` | Wired |
| `complete_action` | Wired |
| `create_commitment` | Wired (`project` / `stewardship` reserved fields ignored; see tool description) |
| `complete_commitment` | Wired |
| `create_tracking_entry` | Wired |

12 of 16 are wired through to the domain. The 4 remaining stubs (the deferred context tools — `get_weekly_context`, `get_monthly_context`, `get_project_context`, `get_stewardship_tracking`) need new domain queries and land in GH #142. All 16 are advertised in `tools/list` with full schemas — Claude can discover them at startup. Stubs return JSON-RPC `INTERNAL_ERROR` with a `"not yet implemented"` message when called.

## What works today

Reachable from the terminal via `cdno`:

- `init` — scaffold a vault
- `log` / `lint` / `capture` — daily-log writes, validation, inbox capture
- `project create / state / park / activate / list / show / milestone {add,done} / waiting {add,resolve}`
- `action add / promote / complete / list` (bullet form + manifest note form)
- `commit create / complete` and `commitments` aggregated view
- `orient` / `status` — morning views
- `portfolio create / list / show` and `file` (file evidence into a portfolio)
- `question create / park / answer / retire / activate` and `questions` (active grouped by domain)
- `stewardship create / list / show / add-periodic` and `track <activity>`

Reachable from Claude via MCP (`cdno-mcp` binary):

- **Context reads** — `get_orientation`, `get_active_questions` (optional domain filter), `get_portfolio_contents`
- **Operations** — `append_to_log`, `file_to_portfolio`, `update_project_state`, `add_action` (with optional `with_note`), `promote_action`, `complete_action`, `create_commitment`, `complete_commitment`, `create_tracking_entry` (with optional `routine`)

Each operation returns a `WriteResultDto { path, message }` so clients can chain on the touched file path.

## How this file is maintained

Updated as part of any PR that changes shipped functionality. The CHANGELOG records the per-PR delta; this file is the rolling snapshot.
