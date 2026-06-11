# Changelog

All notable changes to Cuaderno are recorded here. The project is pre-release; entries are grouped by phase milestone rather than version.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Each entry links to the merged PR.

## [Unreleased]

## [0.1.6] - 2026-06-11

Minor release: full-text content search across the vault — the first way to answer "where did we say X?" rather than only retrieving by note type, slug, or date. Tool count 26 → 27.

### Added — content search (#172)

- **FTS5 content search** (#172) — a SQLite FTS5 index over note title + body, surfaced through a new `search_notes` MCP tool (26 → 27) and a `cdno search` CLI command. Results are ranked best-first (bm25, with a note's H1 title weighted above its body), porter-stemmed for forgiving recall (a singular query matches the plural), and carry a bracketed snippet of the match. Optional filters narrow by note type, an inclusive date window (a daily note's filename date, else the note's `created`), and portfolio. Delivered in three layers: the core index plus its write/reconcile maintenance lifecycle — the index stays live on every write and is self-healed by reconciliation (#175); the domain `Vault::search` with query sanitisation, so arbitrary user text (stray quotes, bare operators, punctuation) becomes a safe `MATCH` rather than an error, plus the `search_notes` tool (#176); and the `cdno search` command with `--type`/`--from`/`--to`/`--portfolio`/`--limit` (#177).

## [0.1.5] - 2026-06-10

Minor release: native meeting-note capture. `upsert_daily_section` gains a `Meeting` section and an append mode so a skill can take live meeting notes that accrue into the daily note — without adding a `meeting` note type (the RLM decomposes a meeting into the chronological log + evidence + actions/commitments). Tool count unchanged (26).

### Added — Phase 4 (MCP server)

- **Daily `Meeting` section + append mode** (#170) — `upsert_daily_section` gains a `Meeting` allowlist value (`{Standup, Intention, Agenda, Meeting}`) and an `append` flag: `append: false` (default) replaces the section as before; `append: true` appends to it, so a meeting skill can take live notes that accrue into the daily `## Meeting` section. The append-only history sections (`## Logs`, `## Notes`) remain off-limits — they grow only through `append_to_log`. cuaderno keeps no `meeting` note type (the RLM decomposes a meeting into the chronological log + evidence + actions/commitments); this is the minimal surface for capturing meeting notes natively.

## [0.1.4] - 2026-06-08

Minor release: the MCP server gains the project/question/stewardship lifecycle tools (26 tools total), so an AI client can move notes through their lifecycle, not just create them.

### Added — Phase 4 (MCP server)

- **Lifecycle MCP tools** (#166) — four tools (22 → 26) so a client can move notes through their lifecycle, not just create them: `park_project`, `activate_project` (enforces the active cap — errors if full, the inverse of `create_project`), `set_question_status` (`active`/`parked`/`answered`/`retired`; unknown → `INVALID_PARAMS`), and `add_periodic_commitment` (recurrence `daily`/`weekly`/`monthly`/`yearly`/`every N months` + next-occurrence date). Each wraps the domain method the CLI already uses and returns `WriteResultDto`. The lifecycle handlers live in a separate `lifecycle.rs` as their own `#[tool_router]`, merged into the dispatch table in `CuadernoServer::new` (with `#[tool_handler(router = self.tool_router)]` so the wire `tools/list` serves the merged set) — the first slice of the handler-group split; the remaining context/operations/creation groups can peel off the same way.

## [0.1.3] - 2026-06-07

Minor release: the MCP server grows from 16 to 22 tools — daily-note persistence and structural creation — so an AI client can manage a vault end-to-end, not just operate the daily loop. No changes to existing CLI or tool behaviour.

### Added — Phase 4 (MCP server)

- **Structural-creation MCP tools** (#162) — four tools (18 → 22) so Claude can create note types, not just operate on existing ones: `create_project` (active below the configurable cap, seeded parked at/above it — the cap is enforced on activation, not creation), `create_portfolio`, `create_question` (`research`/`life`), and `create_stewardship` (`expanded` flag dispatches flat file vs folder with a lazy `tracking/`). Each wraps the domain create method the CLI already uses and returns `WriteResultDto`; unknown `context`/`domain` values are rejected as `INVALID_PARAMS`. This unblocks seeding/managing a vault entirely from Claude (e.g. standing up active projects during an mdv → cdno migration). Lifecycle ops (park/activate, question transitions, periodic commitments) remain a follow-up.
- **Richer daily-note MCP tools** (#158) — two new tools (16 → 18) so skills can persist structured planning content and read it back, which `append_to_log` (single log lines only) couldn't support. `read_daily_note(date?)` returns `{ path, exists, markdown }`, reporting `exists: false` for a day with no note yet rather than erroring, so a skill can check for pre-planned content before writing. `upsert_daily_section(section, content, date?)` creates-or-replaces a daily-note planning section; `section` is allowlisted to `{Standup, Intention, Agenda}` via a typed `DailySection` enum and any other value (including the append-only `Logs`/`Notes`) is rejected as `INVALID_PARAMS`. The append-only history sections are deliberately unreachable through the overwrite path — `## Logs` survives a section upsert untouched. Empty `content` clears a section to just its heading. Domain methods live in a new `crates/cdno-domain/src/vault/daily.rs`; the daily scaffold was refactored so a planning section can seed a fresh note with an empty `## Logs`.

## [0.1.2] - 2026-06-02

Patch release: `cdno` no longer has to be run from inside the vault.

### Added

- **Run `cdno` from outside the vault** (#155) — a new global `--vault <path>` flag plus support for the `CUADERNO_VAULT_PATH` environment variable (the same name the MCP server already honours) let quick verbs like `cdno log` / `cdno capture` run from any directory, instead of failing unless invoked from inside the vault tree. Resolution precedence is `--vault` > a vault discovered by walking up from the current directory > `CUADERNO_VAULT_PATH`; cwd-discovery deliberately beats the env var so a stray `CUADERNO_VAULT_PATH` can't misroute writes when working inside a different vault. Blank / whitespace-only env values are treated as unset. The precedence policy lives in a pure `bootstrap::resolve_vault_root(flag, cwd, env)` (unit-tested across the matrix in `tests/bootstrap.rs`); `main` supplies the real CWD/environment, and three subprocess tests in `tests/cli.rs` cover the flag, the env var, and the cwd-beats-env guarantee end-to-end. The outside-any-vault error now names all three mechanisms. Deferred to a follow-up: a user-level config `default_vault` (the fourth fallback layer from #155).

## [0.1.1] - 2026-06-01

Patch release adding shell completion support. No behavioural changes to existing CLI or MCP surfaces.

### Added

- **Shell completions, both static script + dynamic vault-aware values** (#152) — new `cdno completions <shell>` subcommand emits the registration shim for bash, zsh, fish, elvish, or powershell. The shim uses `clap_complete`'s dynamic engine (gated by the `unstable-dynamic` feature on `clap_complete = "4.5"`): pressing TAB re-invokes `cdno` with `COMPLETE=<shell>` set, which `CompleteEnv::with_factory(Cli::command).complete()` at the top of `main` intercepts before the normal parse runs. Per-flag `ArgValueCompleter` closures open the vault on the fly and surface real slugs as candidates: `--project` (active), `--slug` on project verbs (active for state/park/milestone/waiting, parked for activate, both for show), `--portfolio` (on `cdno file` and `portfolio show`), `--stewardship` (on `cdno track` and `stewardship add-periodic`), `--slug` on `stewardship show`, and `--slug` on every `question` lifecycle verb (park/answer/retire/activate). Completers fail silently when the vault can't be opened — TAB does nothing rather than smearing an error across the prompt. 12 subprocess integration tests in `crates/cdno-cli/tests/completions.rs` cover script emission per shell and runtime intercept behaviour against seeded temp vaults. Homebrew formula needs a `generate_completions_from_executable bin/"cdno", "completions"` line in a separate tap PR once the v0.1.1 bottles publish.

## [0.1.0] - 2026-05-31

First tagged release. Cuts the line under everything shipped across Phases 1, 2, 3, and the closing surface of Phase 4 (all 16 design §11 MCP tools wired, stdio binary polished + e2e-tested). The CLI is daily-usable end-to-end; the MCP server is production-ready against Claude Desktop / Claude Code / Kiro / Gemini CLI.

### Added — Phase 4 (MCP server)

- **Stdio transport polish + subprocess end-to-end tests** (closes #48). The protocol surface (JSON-RPC framing, init handshake, `tools/list`, `tools/call`, error formatting, binary main) was already done by rmcp + #45; this PR adds: structured stderr logging via `tracing` (filter via `RUST_LOG`, defaults to `info`, never writes to stdout because that's the JSON-RPC channel); better startup error messages with `cdno init`/`CUADERNO_VAULT_PATH` hints; a `tests/e2e_stdio.rs` integration test suite that spawns the actual `cdno-mcp` binary, speaks JSON-RPC at it through stdin/stdout, and verifies the init handshake, the full 16-tool `tools/list` catalogue, a successful read tool (`get_orientation`), a successful write tool (`append_to_log` with on-disk artefact verification), and the error path for an unknown tool name. (GH #48)
- **`get_stewardship_tracking` MCP handler** — composes `Vault::list_tracking(stewardship, activity, from, today)` with a small `period` parser supporting `Nd | Nw | Nm | Ny` (calendar-aware months and years via `chrono::Months`). Defaults to `90d` when `period` is omitted. Activity is required per design §11. Unknown period shapes / out-of-range arithmetic surface as `INVALID_PARAMS`. The `from` / `to` bounds are echoed back so clients render the window explicitly. **Closes GH #142**: all 16 design §11 tools are now wired through to the domain; the `not_yet_implemented` placeholder helper retired. (GH #142, final follow-up)
- **`get_project_context` MCP handler** — full context for a single project: typed frontmatter, the full body of the project map, recent daily-log mentions (past 30 days, bare or qualified wikilinks), body backlinks grouped by source note type, and the resolved `core_question` summary when the project sets one. Resolves the slug against both `projects/` and `projects/_parked/`. Core-question resolution silently degrades to `None` on unparseable wikilink or missing target — surfacing as an error would break read-only context queries on a hand-edited link; lint is the right place for that. (GH #142, third follow-up)
- **`get_monthly_context` MCP handler** — strategic monthly scan composing the past 30 days' completed actions, all active questions, the portfolio health table, active projects unchanged for >2 weeks (stuck-detection), every stewardship dashboard, a six-week commitments lookahead, and active-project slot allocation against the configured cap. Output's `since` field echoes the start of the 30-day window so clients render it explicitly. (GH #142, second follow-up)
- **`get_weekly_context` MCP handler** — composes the ISO week's daily logs, the week's completed actions, project state changes during the week, and the next two weeks of commitments into a single `WeeklyContextDto`. The resolved Monday is echoed back as `week_of` so clients render the window explicitly. (GH #142, first follow-up)
- **Context-gathering domain queries (8 methods)** in a new `crates/cdno-domain/src/vault/context.rs` module: `weekly_logs(week_of)` (ISO week, Mon-Sun), `completed_actions_between(from, to)`, `project_state_changes_between(from, to)` (parses the canonical `was → now` shape from daily-note `## Logs`), `stuck_projects(today, unchanged_for_days)` (active-only, mtime-based), `get_project_full(slug)`, `daily_log_mentions(project_slug, since)`, `project_backlinks(slug)` (groups by source note type; body-level wikilinks only — frontmatter wikilinks aren't indexed today, documented as a scope limitation), `list_tracking(stewardship, activity?, from, to)`. These are the foundations for the 4 deferred MCP context handlers (#142 partial). (GH #142)
- **MCP operation handlers** — all 9 design §11 operations wired through to the domain: `append_to_log`, `file_to_portfolio`, `update_project_state`, `add_action` (with optional `with_note`), `promote_action`, `complete_action`, `create_commitment`, `complete_commitment`, `create_tracking_entry`. Each returns a uniform `WriteResultDto { path, message }`. Unknown energy / context strings surface as JSON-RPC `INVALID_PARAMS`. `create_commitment.project` and `create_commitment.stewardship` are reserved on the input schema but ignored today (domain writes both as null per design §5.9). (#47)
- **MCP context handlers (partial)** — `get_orientation`, `get_active_questions` (with optional domain filter), and `get_portfolio_contents` wired through to the domain. Unknown question-domain inputs surface as JSON-RPC `INVALID_PARAMS`. Remaining four context handlers (#142) defer to follow-up because they need new domain queries first. (#46)
- **MCP crate scaffold on `rmcp`** — `cdno-mcp` crate with `CuadernoServer`, 16 typed-input tool stubs covering the full design §11 catalogue, DTO mirrors for every domain summary, and a `cdno-mcp` stdio binary. JSON-RPC over stdio against `cdno-mcp` returns the full tool list with schemas. (#140)
- **Doc tidy** — implementation plan §5.2 rewritten to reflect the `rmcp` adoption; dependency table updated. (#141)

### Added — Phase 3 (knowledge & stewardship)

- **`cdno stewardship` CLI + `cdno track`** — create (flat or expanded with `--tracking`), list, show, add-periodic; tracking notes filed under expanded stewardships with built-in templates for gym/body/swim plus a generic fallback. `cdno track` defaults to the only expanded stewardship when unambiguous. (#139)
- **Stewardship list query + tracking scaffolding** — `Vault::list_stewardships` with per-stewardship staleness/tracking count; `Vault::add_tracking_entry` with activity-aware templates; `StewardshipSummary` / `StewardshipVariant` / `TrackingFrontmatter`. (#138)
- **Periodic commitments + recurrence** — `Recurrence` enum (`Daily | Weekly | Monthly | EveryNMonths | Yearly`) with calendar-aware `next_after`; `Vault::add_periodic_commitment` writes a canonical line to a dashboard's `## Periodic Commitments`; the aggregation source-2 hookup surfaces them in `cdno commitments`. (#137)
- **Stewardship dashboards** — `StewardshipFrontmatter`; `create_stewardship_flat` and `create_stewardship_expanded` with cross-variant collision checks. (#136)
- **Question CLI** — `cdno question {create,park,answer,retire,activate}` plus the top-level `cdno questions` list grouped by domain. (#135)
- **Question CRUD (domain)** — `QuestionFrontmatter`, `QuestionDomain`, `QuestionStatus`; `create_question`, `set_question_status` (no-op on unchanged, logs `was → now` to the daily on a real transition), `active_questions`. Slugs are unique across both domains. (#134)
- **Portfolio CLI + `cdno file`** — `cdno portfolio {create,list,show}` plus the top-level `cdno file` verb for routing evidence into a portfolio. (#133)
- **Portfolio queries** — `Vault::list_portfolios` with staleness, `Vault::get_portfolio_contents`. (#132)
- **Portfolio create + file evidence** — `PortfolioFrontmatter`, `EvidenceFrontmatter` (with required `origin:` from day one); `create_portfolio` and `file_evidence`. (#131)

### Added — Phase 2 (daily loop)

- **README "Getting Started"** — install-from-source, init, daily-loop in five commands. (#130)
- **Pre-release migration collapse** — three SQLite migration files folded into one `001_initial.sql`. (#129)
- **Append-only-after-completion lint** — `cdno lint` protects archived action notes from prefix edits. (#128)
- **Ergonomics retrofit (rest)** — flags-and-prompts convention applied to every remaining mutating verb. (#126, #127)
- **Prompt ergonomics framework** — `prompt::gather_or_error` + the `is_interactive` / `confirm_preview` helpers; flags-and-prompts retrofit on action verbs. (#125)
- **`cdno action` CLI** — `add` / `promote` / `complete` / `list`. (#124)
- **`promote_action` domain op** — promotes a bullet to a manifest action note in-place. (#123)
- **`cdno commitments`** — aggregated date-sorted timeline across project milestones, standalone commitments, and action-note deadlines. (#122)
- **`cdno orient` + `cdno status`** — daily orientation context + project-snapshot views. (#121)
- **`orientation_context` query** — composes commitments + active projects + lapsed habits. (#120)
- **Commitments aggregation (domain)** — `Vault::commitments(today, lookahead_days)` over the four source types (only three present this phase; stewardship source slot wired empty). (#119)
- **`create_action_note`** — the heavier manifest action form. (#118)
- **Milestones index table** — hard deadlines pulled by the aggregation query. (#117)
- **Tags index table** — secondary index for tag-based daily-log queries. (#116)
- **Action frontmatter** — `ActionFrontmatter`, `ActionStatus`. (#115)
- **Standalone commitment notes** — `Vault::create_commitment` and `complete_commitment`. (#106)
- **Project CLI** — `cdno project create / state / park / activate / list / show / milestone / waiting`. (#105)
- **Project summary** — `ProjectSummary` composition with top action. (#104)
- **`vault/projects/` split** — single-file `projects.rs` refactored into a feature folder. (#103)
- **Park / activate project** — lifecycle moves between `projects/` and `projects/_parked/`. (#102)
- **Milestones + waiting-on** — add/done milestones, add/resolve waiting-on items in the project map body. (#101)
- **Action management (bullets)** — append next-action bullets to a project's `## Next Actions`. (#100)
- **`update_project_state`** — rewrites `## Current State` with auto-logging of the previous state to today's daily. (#99)
- **`create_project`** — 5-cap enforcement, template scaffold. (#98)
- **`ProjectFrontmatter`** — typed parse + validation. (#97)
- **Extractors: tags + wikilinks** — body-text helpers for the lint and index. (#95)
- **`cdno capture`** — quick inbox capture with slugged filenames. (#93)
- **vault feature split** — `vault.rs` broken up by feature module. (#92)
- **`cdno log` + `cdno lint`** — daily log writes from CLI; vault-wide validation. (#90)
- **`vault_lint_all`** — domain-level lint over every indexed note. (#89)
- **Reconciliation skips `.cuaderno/`** — internal dir not treated as vault content. (#88)
- **`cdno init`** — vault scaffolding from the terminal. (#86)

### Added — Phase 1 (foundation)

- **`Vault::log_to_daily_note` + `stage_daily_log`** — the canonical daily-log write surface used by every state-changing op. (#85)
- **Reconciliation** — mtime + content-hash sweep on every vault open. (#83)
- **`VaultTransaction`** — atomic file + index writes with rollback. (#82)
- **xxh3 hashing + hard deadline extractor** — content fingerprinting and milestone scraping. (#81)
- **`MemoryIndex`** — in-memory `VaultIndex` for tests. (#80)
- **`VaultIndex` trait + SQLite impl** — three-layer index (nodes, edges, derived). (#79)

(Earlier setup PRs are visible in the git history; this changelog starts at the first feature merge.)

## Conventions

- **Pre-release**: no version numbers yet. When the first release tag lands, the `[Unreleased]` section becomes `[X.Y.Z]` and `[Unreleased]` starts empty above it.
- **Grouping**: entries inside a section appear most-recent-first.
- **PR links**: every entry ends with `(#NNN)`. Hover on any entry to inspect the actual diff.
