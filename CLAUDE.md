# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Cuaderno is a vault-management tool implementing the **Research Logbook Method** — markdown
notes on disk are the product. A five-crate Rust workspace plus a React frontend ships three
binaries and one app:

| Binary / target | Source | Purpose |
|---|---|---|
| `cdno` | `crates/cdno-cli` | terminal CLI |
| `cdno-mcp` | `crates/cdno-mcp/src/bin/stdio.rs` | MCP server over stdio (Claude Desktop / Claude Code) |
| `cdno-mcp-server` | `crates/cdno-mcp/src/bin/server.rs` | MCP over Streamable HTTP, behind an OAuth-terminating proxy |
| desktop app | `crates/cdno-tauri` + `ui/` | Tauri 2 + React 19 |

Vault resolution differs per binary, and the ordering is deliberate. The **CLI**: `--vault` flag first,
then upward discovery from the cwd to the nearest `.cuaderno/`, and `CUADERNO_VAULT_PATH` only as the
last resort — discovery outranks the env var so a stray exported value cannot misroute writes
(`crates/cdno-cli/src/bootstrap.rs`). The **MCP binaries** do env-or-bare-cwd only — no upward walk.
The **desktop app** adds its own layer: a persisted `vault.json` + native folder picker
(`crates/cdno-tauri/src/vault_locator.rs`), with the env var as explicit override. **The repo root is
itself a dev vault** (`.cuaderno/config.toml` is tracked; `index.db` and `.lock` are gitignored), so
`cdno` run from here works against the repo — and, per the ordering above, does so even when
`CUADERNO_VAULT_PATH` points elsewhere.

## Commands

```bash
just ci               # fmt + clippy + test — run before proposing a change.
                      # NB: runs file-REWRITING `cargo fmt` (not a check) and no ui checks,
                      # so it is narrower than the actual CI gate
just build            # cargo build --workspace
just check            # cargo check --workspace --all-targets
just test             # cargo test --workspace
just clippy           # cargo clippy --workspace --all-targets -- -D warnings
just coverage         # cargo tarpaulin --workspace --out html
just install          # cargo install --path crates/cdno-cli --locked
```

Single test — every Rust test lives in a `tests/` integration target, so pass `--test <file-stem>`:

```bash
cargo test -p cdno-domain --test unit -- unit::orient_tests::orientation_context_composes_active_projects_and_commitments   # one test
cargo test -p cdno-domain --test unit -- unit::orient_tests                       # one module
cargo test -p cdno-cli --test project                                             # one CLI target
cargo test -p cdno-mcp --test e2e_http                                            # spawns the real binary
```

Frontend and desktop app:

```bash
cd ui && bun install
bun run test                       # vitest run (whole suite)
bunx vitest run src/lib/dates.test.ts   # single file
bun run build                      # tsc -b && vite build

just app-dev                       # tauri dev — MUST run from the repo root, not ui/
just gen-bindings                  # regenerate ui/src/api/bindings/ from the Rust wire types
```

Docs site (mdBook, separate from `docs/`):

```bash
mdbook serve docs-site             # live preview
mdbook build docs-site             # warns on broken intra-book links — keep it clean
```

CI (`.github/workflows/ci.yml`) runs the jobs check / fmt / clippy / test / ui / coverage
with `RUSTFLAGS: -Dwarnings` (coverage is tarpaulin with xml output). Release is tag-driven (`vX.Y.Z`) and **fails if the tag does not
match the workspace `version` in the root `Cargo.toml`** — bump both together.

## Architecture

```
cdno-core → cdno-domain → cdno-cli
                        → cdno-mcp   → cdno-mcp (stdio) / cdno-mcp-server (HTTP)
                        → cdno-tauri → ui/ (React)
```

The layering is load-bearing; each crate has a rule that the code enforces:

- **`cdno-core`** — mechanics, not policy. Frontmatter/markdown parsing, the `VaultStore` and
  `VaultIndex` traits with filesystem/SQLite and in-memory implementations, `VaultTransaction`,
  templates, reconciliation, file watching. It DOES know the RLM vault taxonomy — the folder
  constants live in `cdno-core/src/paths.rs`, portfolio/evidence path semantics in `artefacts.rs`,
  and the index schema carries RLM tables — so a new note type's folder/path logic belongs in core
  alongside those, or reconcile/init will never see it. What core has none of is business *rules*
  (caps, lifecycle, aggregation).
- **`cdno-domain`** — all RLM business logic, pure: no file I/O, no networking, dependencies by
  constructor injection. The one named exception is `bootstrap` (`open_vault`), the composition
  root that wires concrete store + index for long-lived consumers.
- **`cdno-cli` / `cdno-mcp` / `cdno-tauri`** — thin translation layers. They parse arguments, stamp
  "today", call a `Vault` method, and format the result. Business rules never live here.

`Vault` (`crates/cdno-domain/src/vault/mod.rs`) is the single domain entry point, holding
`Arc<dyn VaultStore>` + `Arc<dyn VaultIndex>` + `VaultConfig`. **Operations are split one-per-file
under `src/vault/` and each attaches its own `impl Vault { ... }` block** — add a new operation as a
new file there rather than growing `mod.rs`.

### Invariants that shape every write

- **Markdown is the source of truth; the SQLite index is a cache.** Deleting everything but the
  vault folder must lose nothing — reconciliation rebuilds. Never make the index authoritative.
- **Domain writes go through `VaultTransaction`** (`cdno-core/src/transaction.rs`): buffer file ops and
  index ops, then `commit()`. File writes are atomic-ish with reverse-order rollback; index updates
  run after all file writes succeed, and an index failure surfaces as `IndexStale` (files are
  correct, next startup reconciles). Not crash-safe by design. **Known exceptions that bypass the
  transaction AND the write lock**: `write_note_raw` (the desktop free-edit save), template
  eject/save, and `save_config_raw` — these call `store.write_file` directly, so the cross-process
  serialisation guarantee does not cover them. Do not add new raw-write paths on their precedent.
- **Startup reconciliation runs inside `Vault::new`**, so any domain method may assume the index
  matches the filesystem on entry.
- **Cross-process writes serialise** on an OS advisory lock (`.cuaderno/.lock`) held for the life of
  a transaction, with a 5s timeout rather than an unbounded hang.
- **Index schema changes are append-only**: add an entry to `MIGRATIONS` in `cdno-core/src/index.rs`
  plus a file under `crates/cdno-core/migrations/`. Never edit a migration that has shipped.
- **Errors are values**: library crates use `thiserror` with layer-specific enums
  (`StoreError`/`IndexError` → `DomainError` → interface error), binaries use `anyhow`. Translate at
  the boundary; don't leak `anyhow` into a library.

### Layer-specific conventions

**CLI — flags-and-prompts** (`docs/cli-ergonomics.md`, non-negotiable for mutating verbs). Every
promptable argument is a clap `Option<T>` flag, never a positional or a required flag (read verbs
like `cdno open` and `project show` are the documented exception and may take a positional).
Handlers fold each one through the shared `gather_or_error` helper: present → use it; absent +
interactive → prompt and set `prompted`; absent + non-interactive → `missing_flag("…")`. Confirm
only when something was prompted. `is_interactive` = `!--no-interactive && stdin AND stdout are
both TTYs` (`crates/cdno-cli/src/prompt.rs`) — stdin matters: with stdout-only, `cdno <verb> <
/dev/null` in a terminal would die in the prompt library instead of failing fast, and no test can
catch that regression, so do not "simplify" the formula.

**MCP.** Tools are `#[tool]` methods on `CuadernoServer` (`crates/cdno-mcp/src/server.rs`), grouped
across `context.rs` / `creation.rs` / `lifecycle.rs` / `operations.rs`. JSON-Schema DTOs live in
`crates/cdno-mcp/src/dto.rs` with `From<DomainType>` impls — they are here, not in `cdno-domain`, to
keep `schemars` out of the domain crate. Every handler routes its synchronous domain call through
`CuadernoServer::with_vault`, which uses `spawn_blocking`. **Stdout is the JSON-RPC channel** — never
print to it; log with `tracing` to stderr (`RUST_LOG=cdno_mcp=debug`). Tool descriptions are the only
instruction surface an agent sees, so they must state the vault's conventions themselves.

**Tauri + UI.** Commands are thin wrappers over `cdno-domain`. Domain types serialise over IPC
directly where they can, but there IS an established wire-struct pattern: `crates/cdno-tauri/src/commands/`
defines crate-local `Serialize + ts_rs::TS` view structs (e.g. `MilestoneView`, `BacklinksView`)
converting core/domain types that cannot carry ts-rs derives — add new command payloads as wire
structs there, never by adding ts-rs to `cdno-core`. The TypeScript types are generated: after
changing any type a Tauri command returns, run `just gen-bindings` (three passes — `cdno-tauri`,
`cdno-domain`, `cdno-core` — because ts-rs cannot follow every container transitively). On the
frontend, `ui/src/api/commands.ts` is the single `invoke()` seam that tests mock; components do not
call `invoke` directly (one legacy exception: `ui/src/shell/useDeepLinkNavigation.ts`).
`ui/src/api/events.ts` maps backend `vault:changed` / `clock:day-changed` / `watcher:status` events
onto react-query invalidations, fed by the watcher thread in `crates/cdno-tauri/src/watcher.rs`.

## Testing

Almost nothing is an inline `#[cfg(test)]` module. Each crate has integration test targets under
`tests/`, and the unit-style suites use an aggregator: `tests/unit.rs` declares `mod unit { mod
foo_tests; … }` and the actual tests live in `tests/unit/foo_tests.rs`. **A new test file must be
registered in `tests/unit.rs` or it silently never runs.**

Profiles per crate (`docs/implementation-plan.md` §7 sketches these, but is stale for the
`cdno-tauri` and `ui/` rows — this list is the accurate one):

- `cdno-core` — real files in `tempfile` temp dirs; OS behaviour is the point.
- `cdno-domain` — `MemoryVaultStore` + `MemoryIndex`; the bulk of the suite lives here.
- `cdno-cli` — `assert_cmd` subprocess runs against a temp vault; wiring only, don't re-test domain logic.
- `cdno-mcp` — handler tests plus `e2e_*.rs` targets that spawn the real binaries and speak JSON-RPC.
- `cdno-tauri` — `tauri::test::{mock_builder, get_ipc_response}` for real IPC round-trips.
- `ui/` — vitest + Testing Library, with `vitest-axe` for accessibility.

## Domain model quick reference

Note types and where they live: `daily`/`weekly` (`journal/`, append-only — `monthly` lives there
too but is mutable: its review sections are upserted), `project` (`projects/`, the primary mutable
note, max 5 active, parked under `projects/_parked/`), `action` (`actions/`, completed notes
archived to `actions/_done/<year>/` — append-only past the frozen prefix, so late retrospectives
may be appended, never edited in), `portfolio` + `evidence` (`portfolios/<slug>/_index.md` + dated
notes), `stewardship` + `tracking` (flat `.md` or expanded folder with `_index.md` and `tracking/`),
`question` (`questions/research|life/`), `commitment` (`commitments/`, `_done/` when fulfilled),
`inbox`. The append-only set is exactly `daily`/`weekly`/`evidence`/`tracking`
(`NoteType::is_append_only`); action, question, portfolio, stewardship dashboards and monthly are
all legitimately mutable in place.

**History preservation**: because the project map is mutable, every `## Current State` update appends
an entry to today's daily log in the form `state on [[<slug>]]` followed by indented `was:` / `now:`
lines (`crates/cdno-domain/src/vault/projects/state.rs` — the context reader parses exactly the
`state on [[` prefix), and completed actions log likewise. Tracing a project's evolution is a search
over the daily log — so never replace a mutable section without emitting its log entry, and never
hand-write that entry in any other shape.

Notes link with wikilinks and the linking rules are enforced; `cdno lint` reports frontmatter and
link problems with an error/warning split (`Error` = downstream code can trip over it, e.g. an edited
frozen note; `Warning` = e.g. a dangling wikilink). Vault-level config is `.cuaderno/config.toml`
(project cap, `ignore` globs, per-type `extra_required` fields, template variables); custom templates
live in `.cuaderno/templates/`.

## Where the documentation lives

- `docs/design.md` — the specification: note types, folder structure, RLM rationale, CLI/MCP/UI surfaces.
- `docs/implementation-plan.md` — trait landscape, layering rules, error strategy, testing strategy, build phases.
- `docs/cli-ergonomics.md` — the flags-and-prompts convention, with the implementation template.
- `docs/rfcs/` — accepted design RFCs (e.g. declarative tracking metrics).
- `STATUS.md` — phase-by-phase status with issue/PR links; `CHANGELOG.md` — per-PR history. Both are
  maintained alongside user-visible changes; a behavioural PR normally updates `CHANGELOG.md`.
- `docs-site/` — the mdBook **user** guide (published to GitHub Pages); every page must be listed in
  `docs-site/src/SUMMARY.md` or mdBook drops it. Contributor-facing notes stay in `docs/`.
- `examples/` — reference custom note types, tracking templates, and Claude skill definitions.
