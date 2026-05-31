# Changelog

All notable changes to Cuaderno are recorded here. The project is pre-release; entries are grouped by phase milestone rather than version.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Each entry links to the merged PR.

## [Unreleased]

### Added — Phase 4 (MCP server, in progress)

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
