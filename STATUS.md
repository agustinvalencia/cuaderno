# Cuaderno Status

Snapshot of development progress as of the most recent merge. For per-PR detail see [`CHANGELOG.md`](CHANGELOG.md); for the underlying plan see [`docs/implementation-plan.md`](docs/implementation-plan.md).

**Current release**: 0.1.22 (2026-06-29). The CLI is daily-usable end-to-end and the MCP server is production-ready (41 tools) against Claude Desktop / Claude Code / Kiro / Gemini CLI. v0.1.22 makes `--json` emit a `{path, message}` write result on the `project`/`action`/`portfolio`/`stewardship` write verbs (and forces those verbs non-interactive under `--json`), continuing #227. The remaining #227 slices are the standalone write commands (`log`/`capture`/`question`/`file`/`commit`) and the `show` verbs. Earlier: v0.1.21 (`--json` list verbs), v0.1.20 (`search --json`), v0.1.19 (portfolio↔project links #253), v0.1.18 (`$EDITOR` review prose #230, memoisation #248), v0.1.17 (frontmatter-order lint #236), v0.1.16 (#242/#244/#245). See [`CHANGELOG.md`](CHANGELOG.md) for the full list.

## Phase summary

| Phase | Scope | Status |
|-------|-------|--------|
| **1 — Foundation** | Workspace layout, `cdno-core` traits + impls (`VaultStore`, `VaultIndex`, transactions, reconciliation, markdown parsing, hashing), `cdno-domain` skeleton, basic CLI bootstrap | Complete |
| **2 — Daily loop** | Projects (5-cap, state, milestones, waiting-on, park/activate), actions (bullets + manifest notes, add/promote/complete/list), commitments (create/complete + aggregation timeline), orient/status/lint, flags-and-prompts ergonomics retrofit, append-only-after-completion lint | Complete |
| **3 — Knowledge & stewardship** | Portfolios + evidence (create, file, list, show), questions (CRUD + status transitions + grouped list), stewardships (flat + expanded, list, show, periodic commitments, tracking notes with built-in templates), `cdno track` | Complete |
| **4 — MCP server** | `cdno-mcp` crate on `rmcp`, full 41-tool schema catalogue, stdio binary | Core complete (16 §11 + 2 daily-note (#158) + 2 weekly-note + 4 structural-creation (#162) + 4 lifecycle (#166) + `search_notes` (#172) tools wired, stdio binary polished); file watcher (#49) + skill adaptations (#50/#51/#52) outstanding |
| **5 — Tauri UI** | `cdno-tauri` backend, React frontend with Tremor, Home / Weekly / Commitments views | Not started |
| **6 — Extended UI + HTTP** | Monthly / Portfolio / Stewardship views, HTTP transport, periodic reconciliation | Not started |
| **7 — Migration** | `cdno migrate --from-mdv` interactive importer | Not started |

## Phase 4 detail

| Issue | Scope | Status | PR |
|-------|-------|--------|----|
| #45 | `cdno-mcp` crate scaffold on `rmcp`, all 16 tool schemas advertised, stdio binary | Complete | #140 |
| — | Doc tidy: implementation plan §5.2 updated for rmcp choice | Complete | #141 |
| #46 | `HandlerRegistry` + 7 context-gathering handlers | Partial — 3 of 7 handlers shipped (`get_orientation`, `get_active_questions`, `get_portfolio_contents`); registry covered by `#[tool_router]` macro | (this PR) |
| #142 | Remaining 4 context handlers + supporting domain queries (weekly/monthly context, project context, stewardship tracking) | Complete — #145 (8 domain queries), #146 (weekly), #147 (monthly), #148 (project), this PR (stewardship_tracking) | #145 + #146 + #147 + #148 + (this PR) |
| #47 | 9 operation handlers (append_to_log, file_to_portfolio, update_project_state, add/promote/complete_action, create/complete_commitment, create_tracking_entry) | Complete | (this PR) |
| #48 | Stdio transport polish + Claude Desktop end-to-end test | Complete — protocol surface verified via subprocess JSON-RPC e2e tests; stderr `tracing` logging on the binary (RUST_LOG-controlled); better startup error messages. The "actually try it in Claude Desktop" smoke is a separate manual checklist item. | (this PR) |
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
| `get_weekly_context` | Wired |
| `get_monthly_context` | Wired |
| `get_project_context` | Wired |
| `get_stewardship_tracking` | Wired |
| `append_to_log` | Wired |
| `file_to_portfolio` | Wired (#154; optional `attach` files a non-markdown artefact as a linked stub) |
| `update_project_state` | Wired |
| `add_action` | Wired |
| `promote_action` | Wired |
| `complete_action` | Wired |
| `create_commitment` | Wired (`project` / `stewardship` origin links persisted as slugs; #199) |
| `complete_commitment` | Wired |
| `create_tracking_entry` | Wired |
| `read_daily_note` | Wired (#158) |
| `upsert_daily_section` | Wired (#158/#170; sections `{Standup, Intention, Agenda, Meeting}`; `append` mode for live meeting notes) |
| `create_project` | Wired (#162; at/above the cap, seeded parked) |
| `create_portfolio` | Wired (#162) |
| `create_question` | Wired (#162) |
| `create_stewardship` | Wired (#162; `expanded` flag = flat vs folder) |
| `park_project` | Wired (#166) |
| `activate_project` | Wired (#166; errors at the active cap) |
| `set_question_status` | Wired (#166; `active`/`parked`/`answered`/`retired`) |
| `add_periodic_commitment` | Wired (#166; recurrence + next date) |
| `search_notes` | Wired (#172; FTS5 content search, optional note-type / date / portfolio filters) |

**All 41 tools are wired through to the domain** — the 16 design §11 tools, the two daily-note tools (#158), the two weekly-note tools (`read_weekly_note` + `upsert_weekly_section`), the four structural-creation tools (#162), the four lifecycle tools (#166), `search_notes` (#172), the `link_portfolio_to_question` backlink tool (#200), the four read-parity tools (`list_projects`, `get_commitments`, `lint`, `capture` — #204), the four milestone / waiting-on tools (#213), the two inbox-triage tools (`triage_inbox`, `discard_inbox_item` — #208), and the `link_portfolio_to_project` backlink tool (#253). No stubs remain. All 41 are advertised in `tools/list` with full schemas, so Claude can discover them at startup. The lifecycle group is split into its own `#[tool_router]` (in `lifecycle.rs`), merged in `CuadernoServer::new` — the first slice of the handler-group split.

## What works today

Reachable from the terminal via `cdno`:

- `init` — scaffold a vault
- `log` / `lint` / `capture` — daily-log writes, validation, inbox capture
- `project create / state / park / activate / list / show / milestone {add,done} / waiting {add,resolve}`
- `action add / promote / complete / list` (bullet form + manifest note form)
- `commit create / complete` and `commitments` aggregated view
- `orient` / `status` — morning views
- `portfolio create / list / show` and `file` (file evidence into a portfolio; `--attach <path>` files a non-markdown artefact — PDF/image/video — by copying it in and scaffolding a linked evidence stub, `--move` to relocate instead, #154)
- `question create / park / answer / retire / activate` and `questions` (active grouped by domain)
- `stewardship create / list / show / add-periodic` and `track <activity>`
- `search <query>` — full-text content search across the vault, ranked best-first, with optional `--type` / `--from` / `--to` / `--portfolio` / `--limit` filters
- `completions <shell>` — emit a shell-completion shim (bash, zsh, fish, elvish, powershell) with **dynamic vault-aware tab completion** for slug-valued flags (`--project`, `--portfolio`, `--stewardship`, `--slug` on project/question verbs)

Reachable from Claude via MCP (`cdno-mcp` binary):

- **Context reads (9)** — `get_orientation`, `get_active_questions` (optional domain filter), `get_portfolio_contents`, `get_weekly_context` (ISO-week logs + completed actions + state changes + 2-week commitments), `get_monthly_context` (30-day wins + active questions + portfolios + stuck projects + stewardships + 6-week commitments + project slot allocation), `get_project_context` (project map + 30-day daily-log mentions + body backlinks + resolved core_question), `get_stewardship_tracking` (per-stewardship per-activity tracking notes in a configurable window like `30d`/`6m`/`1y`), `read_daily_note` (a day's markdown, or `exists: false` when none yet), `search_notes` (#172 — FTS5 full-text search over title + body, ranked best-first, with optional note-type / date-window / portfolio filters)
- **Operations** — `append_to_log`, `file_to_portfolio`, `update_project_state`, `add_action` (with optional `with_note`), `promote_action`, `complete_action`, `create_commitment`, `complete_commitment`, `create_tracking_entry` (with optional `routine`), `upsert_daily_section` (write a `{Standup, Intention, Agenda, Meeting}` section — replace, or `append` for live meeting notes)
- **Structural creation (#162)** — `create_project` (active below the cap, parked at/above it), `create_portfolio`, `create_question` (research/life), `create_stewardship` (flat or expanded)
- **Lifecycle (#166)** — `park_project`, `activate_project` (cap-enforced), `set_question_status` (active/parked/answered/retired), `add_periodic_commitment` (recurrence + next date)

Each operation returns a `WriteResultDto { path, message }` so clients can chain on the touched file path.

## How this file is maintained

Updated as part of any PR that changes shipped functionality. The CHANGELOG records the per-PR delta; this file is the rolling snapshot.
