# Changelog

All notable changes to Cuaderno are recorded here. The project is pre-release; entries are grouped by phase milestone rather than version.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Each entry links to the merged PR.

## [Unreleased]

### Added

- **Config-defined custom note types** (#293, #294, #295, #296) — declare your own **schema-only**
  note type under `[note_types.<name>]` in `.cuaderno/config.toml`, for entities the eleven built-in
  types don't cover (people, books, clients). A custom type gets a folder, enforced
  `required`/`optional` fields (checked by `cdno lint`), an optional template, canonical
  frontmatter ordering (`cdno normalise`), and full participation in indexing, search, backlinks,
  and completions — but **no bespoke behaviour** (caps, lifecycle, aggregation stay exclusive to the
  built-in types, so a custom type is invisible to `orient`/the cap by design). Create and list with
  `cdno note create <type> --title … --field k=v` / `cdno note list <type>`, or the MCP
  `create_custom_note` tool. `cdno templates vars <type>`, `cdno search --type <type>`, and shell
  completion all recognise custom types. Reserved names (a custom type may not shadow a built-in,
  case-insensitive), folder collisions, and vault-escaping folders are rejected at vault-open. A
  worked `person` example ships in [`examples/note-types/person/`](examples/note-types/person/) with
  a [Tracking people](https://agustinvalencia.github.io/cuaderno/tutorials/tracking-people.html)
  recipe. `append_only` on a custom type is accepted but not yet lint-enforced.

## [0.2.1] - 2026-07-02

`cdno templates` polish: `vars` now reports the complete supplied set, `eject --all` ejects every built-in at once, and `cdno track` nudges newcomers toward the example templates.

### Added

- **`cdno templates eject --all`** (#288) — eject every built-in template into `.cuaderno/templates/`
  in one go, for customising the whole vault at once. Types that already have a template file are
  skipped (a summary reports which) unless `--force`. `<type>` and `--all` are mutually exclusive;
  `--json` emits an object with `written` and `skipped` note-type-name arrays.
- **`cdno track` hints at the example templates for newcomers** (#282) — until a vault has any
  tracking template, `cdno track` prints a one-line nudge (on stderr) pointing at
  `examples/templates/tracking/` for a structured layout. It goes quiet once you author any tracking
  template (no per-activity nagging on this high-frequency command) and is suppressed under `--json`.
  Closes the discovery gap raised in the #281 review.

### Changed

- **`cdno templates vars <type>` now reports the complete supplied set** (#279) — the `supplied`
  placeholders come from the type's full create-path key set (`NoteType::supplied_placeholders`)
  rather than being scanned from the built-in template, so it no longer under-reports keys a default
  template happens not to reference (`daily`'s `weekday`, `tracking`'s `routine`).
  A drift test pins every built-in template to only reference names from that set. Because the set is
  per-type (variants supply the same keys), the now-inert `--variant` flag is dropped from `templates
  vars`. Closes the subset caveat #271 shipped with.

## [0.2.0] - 2026-07-01

MCP parity for prompted template variables, a `cdno templates` command group to discover a type's placeholders and eject its built-in, and config-driven tracking variants (gym/body/swim no longer ship built-in).

### Changed

- **Tracking variants are config-driven** — the built-in `gym`, `body`, and `swim` tracking
  templates no longer ship in the binary; only the neutral `generic` `tracking` template is built in.
  An activity now uses a vault's `.cuaderno/templates/tracking-<activity>.md` when present, else the
  generic template (the resolver is unchanged — slugify the activity, look up `tracking-<slug>`,
  fall back to generic). Ready-made gym/body/swim variants moved to
  [`examples/templates/tracking/`](examples/templates/tracking); copy one into a vault to use it.
  **Behaviour change:** without a custom template, `cdno track gym` (and the `create_tracking_entry`
  MCP tool) now produce the generic shape rather than the old exercise/metrics/set tables — add the
  matching example template to keep the old shape. To preserve the frontmatter order too, install the
  example template *before* running `cdno normalise` (a gym note normalised without it reorders its
  `duration_min`/`routine` keys to the end — cosmetic, no data loss). Since no variant templates now
  ship built-in, `cdno templates eject` no longer takes `--variant` (base note-type templates only;
  `tracking` variants are authored in the vault, not ejected).

### Added

- **`cdno templates eject <type>`** (#270) — copy a built-in template into
  `.cuaderno/templates/<type>.md` as an editable starting point, so customising a type no longer
  means copying from the source tree or hand-reconstructing from a created note. Refuses to overwrite
  an existing custom template unless `--force`. The written file is byte-identical to the built-in, so
  behaviour is unchanged until you edit it. Backed by a new `Vault::eject_template`. (Only base
  note-type templates ship built-in, so there's no `--variant` — `tracking` activity variants are
  authored in the vault; see `examples/templates/tracking/`.)
- **`cdno templates vars <type>`** (#271) — list the `{{placeholders}}` a note type's template
  supports, so you know what a custom `.cuaderno/templates/` override may reference without reading
  the source. The supplied set is **derived** from the type's built-in template — every entry is a
  key the create path fills, so it never advertises a placeholder that would render literally.
  (A couple of types' create paths set an extra key their default template doesn't use — e.g.
  `daily` also provides `{{weekday}}` — so treat it as the built-in template's set plus config vars;
  the templates tutorial lists the complete fillable set per type.) Config `[variables]` /
  `[variables.prompt]` names are folded in and classified by source (`supplied` / `config` /
  `prompt`). A `--variant` flag selects a `<type>-<variant>` built-in when one exists (none ship
  today, so it falls back to the base type); `--json`
  emits a `{ name, source }` array. A new public `Vault::template_placeholders` and
  `cdno_core::template::placeholder_names` back it.
- **`vars` on the MCP create tools** (#238) — the templated create handlers now accept an optional
  `vars` object (a `name -> value` map, the MCP analogue of the CLI's repeatable `--var name=value`),
  threaded to the domain's `*_with_vars` methods. Covers `create_project`, `create_portfolio`,
  `create_question`, `create_stewardship`, `create_commitment`, `create_tracking_entry`,
  `file_to_portfolio` (markdown path), `add_action` (`with_note: true`), and `promote_action`. An MCP
  agent can now create a note whose template uses a prompted variable; omitting a required one still
  surfaces `UnresolvedPrompts` via normal MCP error mapping. The paths that don't gather prompted
  variables (`capture`, `complete_action`, `file_to_portfolio --attach`, inline `add_action`) are
  unchanged, matching the CLI. The `UnresolvedPrompts` message is reworded to name both the CLI
  `--var` flag and the MCP `vars` parameter, so the guidance fits whichever surface hit it.

## [0.1.26] - 2026-06-30

Interactive `[variables.prompt]` template variables, with a `--var name=value` flag (#238 tier 4) — completes #238.

### Added

- **Prompted template variables** (#238) — `[variables.prompt]` entries in `.cuaderno/config.toml` are
  now gathered at note creation. A creating command resolves each prompted variable its effective
  template uses from a static `[variables]` default if one exists (which suppresses the prompt), else a
  repeatable **`--var name=value`** flag, else an interactive prompt (shown in the confirm preview);
  if none supplies it the command errors instead of writing a literal `{{name}}`. `--var` is available
  on `project create`, `question create`, `stewardship create`, `commit create`, `portfolio create`,
  `file`, `track`, `action add --note`, and `action promote`. A new `DomainError::UnresolvedPrompts`
  and a `Vault::template_prompts` query back the domain enforcement and the CLI gathering.
- Docs: the [Customising templates and frontmatter](https://agustinvalencia.github.io/cuaderno/tutorials/templates-and-frontmatter.html)
  tutorial gains a **Prompted variables** section, and `--var` is documented on each create command's
  CLI reference page.

### Notes

- The implicit-write paths (daily `log`, weekly, inbox `capture`) and the non-templated paths
  (`file --attach`, plain `action add`) don't gather prompted values — a `[variables.prompt]`
  placeholder in one of those templates surfaces `UnresolvedPrompts`; give it a static default.
- MCP create handlers don't supply prompted values, so an MCP-created note whose template uses a
  prompted variable surfaces the same error via normal MCP error mapping. (Resolved in the next
  release — see the Unreleased `vars` entry above.)

## [0.1.25] - 2026-06-30

Static config template variables now resolve; plus the documentation site (shipped earlier this cycle).

### Added

- **Static config template variables** (#238) — custom templates can now reference vault-wide
  variables set under `[variables]` in `.cuaderno/config.toml` (e.g. `{{author}}`); they resolve on
  every note type at creation. Per-type (contextual) placeholders take precedence over a config
  variable of the same name. (Previously parsed but inert.) Interactive `[variables.prompt]` variables
  remain a follow-up.
- **Documentation site** — a full user guide built with [mdBook](https://rust-lang.github.io/mdBook/) under [`docs-site/`](docs-site/), deployed to GitHub Pages (<https://agustinvalencia.github.io/cuaderno>) by a new `docs.yml` workflow. Covers concepts (the RLM, note types, vault structure, business rules, configuration), task tutorials, the full CLI reference (every command + flags), and the MCP tool reference (all 41 tools). Docs-only; no code or behaviour change.

## [0.1.24] - 2026-06-29

`--json` on the `show` verbs — completes the `--json` surface (#227 closed).

### Added

- **`--json` on the `show` verbs** (#227) — `project show`, `portfolio show`, and `stewardship show` now emit a composite detail object under `--json`: `project show` serialises the `ProjectSummary` (same shape as `project list`); `portfolio show` mirrors the MCP `PortfolioDetailDto` (`slug`/`question`/`created`/`project`/`evidence[]`); `stewardship show` mirrors `StewardshipDetailDto` (`slug`/`name`/`context`/`variant`/`body_markdown`). This is the final slice of #227 — every read and write verb now honours `--json`.

## [0.1.23] - 2026-06-29

`--json` write results on the standalone write commands — every write verb now honours `--json`.

### Added

- **`--json` on the standalone write commands** (#227) — `log`, `capture`, `file`, `track`, and the `question`/`commit` create/transition verbs now emit a `{path, message}` JSON result under `--json` (and run non-interactively), completing the write-verb half of #227. With the earlier project/action/portfolio/stewardship slice, every write verb now honours `--json`. Only the `show` verbs remain on #227.

## [0.1.22] - 2026-06-29

`--json` write results on the project/action/portfolio/stewardship write verbs.

### Added

- **`--json` on the `project`/`action`/`portfolio`/`stewardship` write verbs** (#227) — these write verbs now emit a `{path, message}` JSON result under `--json` (the same shape as the MCP `WriteResultDto`) instead of treating the flag as a silent no-op. Covers `project create`/`state`/`park`/`activate`/`milestone`/`waiting`, `action add`/`promote`/`complete`, `portfolio create`/`link`, and `stewardship create`/`add-periodic`. The standalone write commands (`log`, `capture`, `question`, `file`, `commit`/`track`) follow in a subsequent slice; `show` verbs still tracked on #227.

## [0.1.21] - 2026-06-29

`--json` on the `list` read verbs.

### Added

- **`--json` on the `list` read verbs** (#227) — `cdno project list`, `portfolio list`, `stewardship list`, and `action list` now honour `--json`, emitting their summaries as a JSON array (`project list` serialises the per-project summaries the text view shows). Casing matches the MCP DTOs (e.g. `stewardship` variant `flat`/`expanded`, action energy `deep`/`medium`/`light`). Continues #227 after `search --json`; the `show` verbs and the write-verb JSON result are still tracked there.

## [0.1.20] - 2026-06-29

`cdno search` gains `--json`.

### Added

- **`cdno search --json`** (#227) — `search` now honours the global `--json` flag, emitting the ranked hits as a JSON array (`path`, `note_type`, `title`, `snippet`, `score`) in best-first order for scripted consumers, alongside the existing `commitments` / `questions` / `status` / `orient` JSON verbs. This is the first slice of #227; the remaining `list`/`show` read verbs and the write-verb JSON policy are tracked there.

## [0.1.19] - 2026-06-28

Portfolio ↔ project linking is now bidirectional. MCP catalogue 40 → 41.

### Added

- **Portfolio → project links are bidirectional** (#253) — `create_portfolio` with a `project` now backfills that project map's `## Links` with `[[portfolios/<slug>/_index]]` (replacing the `(none yet)` placeholder) in the same commit, so the project map visibly lists its portfolios. Previously the portfolio's `project:` frontmatter pointed up but nothing pointed down — the project's `## Links` stayed empty, forcing a hand-edit (frontmatter links aren't body-scanned). Being a body wikilink, it also becomes a backlink-graph edge on the next full reindex (the same deferred-resolution caveat as every domain-written body link). A new retrofit verb — `cdno portfolio link --project <target>` and MCP tool `link_portfolio_to_project` — sets the portfolio's `project:` frontmatter *and* backfills the project's `## Links`, for portfolios created before their project, without one, or before this change. Idempotent on each end; skips silently at create time when the named project note doesn't exist (the frontmatter link still stands). MCP catalogue 40 → 41.

## [0.1.18] - 2026-06-28

Weekly-review ergonomics + an internal speedup. No new MCP tools (40); no new CLI commands.

### Changed

- **`cdno review weekly` prose sections open in `$EDITOR`** (#230) — Wins and Challenges (the multi-line retrospective sections) now launch `$EDITOR` instead of a single-line text prompt, pre-seeded with the section's current content so you edit in place. Editing in place also dissolves the compose-vs-accrue question: whatever you save replaces the section. The single-line `One Improvement` and the forward goal stay plain text prompts. Non-interactive / piped runs are unchanged (they print the note).

### Internal

- **Memoise canonical frontmatter order per `(type, variant)`** (#248) — `cdno lint` and `cdno normalise` now resolve each note type's effective template once per pass instead of once per note, removing O(notes) redundant template stats/reads on large vaults. No behavioural change; output is identical.

## [0.1.17] - 2026-06-28

Small hardening release: one new lint rule. No new MCP tools (40); no CLI surface changes.

### Added

- **Lint rule: frontmatter-order drift** (#236) — `cdno lint` now warns when a note's frontmatter keys aren't in the canonical per-type order (the same order `cdno normalise` would apply, derived from the effective template). It's a `Warning` — the note is valid, just untidy — so `cdno lint --strict` / CI catches drift without a separate command, and `cdno normalise` fixes it. Surfaced over MCP for free through the existing `lint` tool; `normalise` itself stays CLI-only for now.

## [0.1.16] - 2026-06-28

Backlog-hardening plus a template fix: a config `ignore` glob list, the daily `{{weekday}}` variable, and the weekly note's anchor-section rename. No new MCP tools (40); `cdno reindex` gains an excluded-files count line.

### Added

- **Config `ignore` globs** (#242) — a top-level `ignore = ["glob", ...]` in `.cuaderno/config.toml` excludes matched files from the index, and therefore from reconciliation, search, and lint, in one place. Matched against each file's vault-relative path: `*` stays within a path segment, `**` spans segments, bare names are root-anchored (`**/name` to match at any depth); patterns are additive only (no `!` negation). Empty by default — markdown is the source of truth, so a note is never silently dropped; intended for repo scaffolding (`CLAUDE.md`, `README.md`) that lives in the vault dir but isn't a note. Files are never deleted: exclusion only drops index rows (fully recoverable by clearing the glob and reindexing), and `cdno reindex` now reports how many files an `ignore` pattern excluded so an over-broad glob isn't a silent retrieval blackout.

### Changed

- **Weekly note: `Next Week's Focus` → `This Week's Goal`** (#245) — the weekly note's fourth section is renamed so each week's note carries *its own* anchoring goal, rather than holding next week's goal a week early (which forced cross-week reads to find the current week's anchor). Carry-forward is now explicit: weekly-review writes the goal into *next* week's note (its `This Week's Goal`), and weekly-planning sets/re-aims the planned week's goal directly — passing a `date` in that week creates the note and sets its goal in one call, so the upcoming week's daily notes get an umbrella. `cdno review weekly` now writes the three retrospective sections into the ending week's note and the forward goal into next week's. `upsert_weekly_section` still accepts the former `Next Week's Focus` name as a deprecated alias (it maps to `This Week's Goal`) so pre-rename callers don't hard-fail. Template, MCP tool docs, and design §5.2 updated; existing notes keep the old heading until rewritten or migrated. Note: writing the goal into a pre-rename note (one that still has `## Next Week's Focus`) adds a fresh `## This Week's Goal` section rather than replacing the old heading, leaving the stale `## Next Week's Focus` to delete by hand — there is no automatic section-heading migration (`cdno normalise` reorders frontmatter keys, not section headings).

### Fixed

- **Daily `{{weekday}}` template variable** (#244) — a custom `.cuaderno/templates/daily.md` titled `# {{weekday}}` rendered the literal placeholder, because the daily scaffold supplied only `date` and `heading` and the engine leaves unknown placeholders verbatim. The scaffold now also supplies `weekday` (the full weekday name, e.g. `Sunday`). The design §9 variable table's unimplemented `{{day_name}}` is corrected to the implemented `{{weekday}}`, and a contradictory precedence note (it claimed later tiers override earlier) is fixed to match the code (earlier tiers win).

## [0.1.15] - 2026-06-21

Note structure & custom templates: notes keep a consistent shape, and the long-promised custom-template system (design §9) is finally wired so `.cuaderno/templates/` overrides take effect. No new MCP tools; one new CLI command (`cdno normalise`). Tool count unchanged (40).

### Added

- **Custom templates are live** (#212) — note creation now resolves through the template engine, so a custom `.cuaderno/templates/<type>.md` overrides the built-in default for every note type (project, action, question, stewardship, portfolio, evidence, commitment, tracking + variants, and daily/weekly/inbox). Custom templates are read through the `VaultStore` (same abstraction as every other vault file). They render against the built-in variable set each operation supplies; a custom template referencing a *new* variable (e.g. a vault-level `{{author}}`) leaves it literal until config variables land (#238).
- **`cdno normalise [--check]`** (#233) — reorder a note's frontmatter into canonical key order, derived from its *effective* template (custom override or built-in). Line-based and value-preserving (quoting, `null`s, unknown keys, multi-line values all survive); idempotent. `--check` reports out-of-order notes and exits non-zero without writing (CI / pre-commit). Notes cdno creates are already canonical, so a clean vault is a no-op.
- **Frontmatter canonical order** (#233) — `NoteType::frontmatter_order` defines the per-type key order (`type` first), pinned to the templates and code scaffolds by tests so they can't drift.

### Changed

- **The daily `## Logs` section stays at the bottom** (#232) — new sections (a mid-day `## Meeting`, etc.) no longer push the running history up; `## Logs` is pinned last and a note where it had drifted is self-healed on the next write. The "keep-last" anchor is the daily template's final section, so a custom daily template can pin a different trailing section (#212).
- **Daily/weekly/inbox notes are template-driven** (#212) — their in-code scaffolds became template files (`templates/{daily,weekly,inbox}.md`), so they're customisable too. The first daily-log write now inserts into `## Logs` via the section manipulator rather than appending raw text.
- **Enforcement follows the effective template** (#212) — `cdno normalise`'s key order and the daily Logs anchor both derive from whatever template a note is created from, so customisation and enforcement agree instead of the enforcement fighting a custom layout. Output is unchanged on a vault with no custom templates.

### Notes

- Follow-ups filed: config `[variables]` + interactive `[variables.prompt]` (#238), surface frontmatter-order drift as a lint rule (#236).

## [0.1.14] - 2026-06-21

Pre-UI hardening release: the largest tool-surface jump yet (29 → 40 MCP tools) plus integrity, lint, and read-surface work that closes the gaps found auditing the CLI / domain / MCP layers before starting the Tauri UI. Adds MCP read parity (the CLI's daily-driver reads are now callable over MCP), milestone / waiting-on write tools, a broken-wikilink lint, an inbox-triage loop, a guided weekly-review ritual, `--json` output, and index self-healing.

### Added

- **MCP read-parity tools** (#204) — `list_projects`, `get_commitments`, `lint`, and `capture`, so an assistant can reach the same daily-driver reads the CLI exposes without shelling out (29 → 34).
- **Milestone & waiting-on MCP tools** (#213) — `add_milestone`, `complete_milestone`, `add_waiting_on`, `resolve_waiting_on` (→ 38), completing MCP parity for the project-board write verbs.
- **Inbox triage** (#208) — `cdno triage` drains uncategorised `inbox/` captures (keep-as-action / discard / skip; non-interactive runs list what's pending), plus `triage_inbox` and `discard_inbox_item` MCP tools (→ 40). Discard is a deliberate hard delete but preserves the captured text in the daily log, so it stays recoverable.
- **Question ↔ portfolio backlinks** (#200) — `create_portfolio` writes bidirectional links to a `core_question`, the `link_portfolio_to_question` MCP tool and `cdno portfolio link` retrofit existing portfolios, and links resolve to the folder note's `_index`.
- **Broken-wikilink lint + severities** (#205) — `LintReport` gains `LintSeverity` (`Error` / `Warning`); a body-only broken-wikilink check reports dangling links as warnings without aborting on a single unreadable note.
- **`cdno reindex`** (#206) — rebuilds the SQLite index from the markdown source of truth; the recovery path for a corrupt or stale index. `SqliteIndex::open` also self-heals a corruption-shaped error on an existing index by rebuilding once.
- **`cdno review weekly`** (#209) — the guided weekly-review ritual: walks the four weekly-note sections (Wins, Challenges, One Improvement, Next Week's Focus) and composes each interactively over the existing `upsert_weekly_section` seam. Non-interactive runs print the current note. (`review monthly` tracked in #228.)
- **`--json` output** (#210) — a global `--json` flag emits machine-readable JSON on the daily-driver read verbs (`commitments`, `questions`, `status`, `orient`) for scripts and skills. `CommitmentSource` serialises as a homogeneous `{kind, slug}` shape matching the MCP DTO.
- **`cdno lint --strict`** (#217) — warnings are non-fatal by default; `--strict` exits non-zero on any issue (errors always fail).

### Changed

- **Wikilink resolver: last-segment fallback** (#215) — a `[[actions/<slug>]]` reference now resolves after the note relocates within its tree (archived to `actions/_done/<year>/`, parked, or completed), via a unique last-path-segment stem match. Keeps resolution sound (a stem collision degrades to unresolved, never the wrong note) and removes the pervasive broken-wikilink noise that #205 surfaced.
- **Surface reconciliation errors** (#207) — `open_vault` now prints startup-reconciliation errors to stderr instead of swallowing them, so a note that fails to index is visible rather than silently missing.

### Fixed

- **Commitments "4 sources" doc drift** (#214) — aligned the docs with the actual four-source aggregation (project milestones, stewardship periodic commitments, standalone commitments, and action `due:` dates).

### Notes

- MCP tool count: 29 → 40 (#200 +1, #204 +4, #213 +4, #208 +2).
- `cdno lint --fix` (#211) was assessed and deferred: no current lint category is safely auto-fixable, so it would be a no-op today. Follow-ups filed for deferred scope: monthly note type + `review monthly` (#228), `--json` for list / show / search (#227), slug uniqueness (#225), `inquire::Editor` for review prose (#230).

## [0.1.13] - 2026-06-15

Minor release: commitments can now record where they came from. `create_commitment` persists the optional `project` / `stewardship` origin links it previously dropped, and a project or stewardship can list the dated commitments that point at it. No new tools or commands; tool count unchanged (29).

### Added

- **Commitment origin links** (#199) — `create_commitment` now persists the `project` and `stewardship` fields instead of writing them as `null`. Inputs are canonicalised through the filename slugifier (so `Health` resolves to the `health` stewardship) and stored as quoted YAML scalars, which keeps an arbitrary input from injecting YAML or being read back as a non-string scalar. The links are bare slugs (matching `action.project` / `tracking.stewardship`), not wikilinks — frontmatter wikilinks aren't indexed as backlinks. They are loose pointers: the target's existence isn't validated. The MCP `create_commitment` tool gains working `project` / `stewardship` arguments and the CLI gains `--project` / `--stewardship` flags.
- **Commitment backlink queries** (#199) — `Vault::commitments_for_project` and `Vault::commitments_for_stewardship` return the standalone commitments linked to a project or stewardship, sorted by due date (active and completed). They use the type-scan-and-filter idiom rather than the link index, so a stewardship dashboard can surface its related dated items. Not yet exposed through a dedicated MCP/UI read surface.

## [0.1.12] - 2026-06-13

Patch release: concurrent-write safety. With several agents or processes sharing one vault, writes to the same note — most acutely the daily log — could silently clobber each other; this serialises them. No new tools or commands; tool count unchanged (29).

### Fixed

- **Cross-process vault write lock** (#196) — every write now holds an exclusive advisory lock (std `File::lock` on `.cuaderno/.lock`) across its whole read-modify-write, so two cdno processes writing the same note serialise instead of one overwriting the other's change. The mutating ops are read-modify-write full rewrites (read the note, edit, rewrite the file); the SQLite index was already protected (WAL + busy_timeout) but the markdown files — the source of truth — were not, so a concurrent `append_to_log` or section write could drop a line. The lock is taken at transaction construction (before the read, since the lost update is born at the read), released on commit, and freed by the OS on process death; readers don't lock. Zero new dependencies (std native file locking). Proven by concurrency regression tests over `log_to_daily_note` and `add_action`.

## [0.1.11] - 2026-06-13

Minor release: the weekly note becomes a first-class, writable artefact, completing the weekly loop. The MCP server gains a weekly-note read/write pair (27 → 29 tools) and the CLI gains `cdno weekly`.

### Added — weekly note

- **Weekly-note write surface** (#193) — `read_weekly_note` and `upsert_weekly_section` MCP tools (27 → 29) persist the design §5.2 weekly note (`journal/<iso-year>/weekly/<YYYY>-Www.md`): ISO-week frontmatter (`week`, `date_start` = Monday, `date_end` = Sunday) plus the four sections Wins, Challenges, One Improvement, and Next Week's Focus (the forward plan). Keyed by ISO week — any day in the week writes the same note; `append: false` (default) composes the section, `append: true` accrues. Mirrors the daily-section seam (#158); the review and the plan share one artefact per week, so cdno keeps no separate weekly-plan note.
- **`cdno weekly`** (#194) — prints the week's note with the frontmatter stripped; `--date <YYYY-MM-DD>` selects another ISO week (any day in it); a week with no note shows a placeholder pointing at the review/planning skills. The terminal window onto the weekly note, completing CLI/MCP parity for weekly content.

## [0.1.10] - 2026-06-13

Minor release: prettier CLI output. Every list-style command now renders through a shared borderless table that wraps long text to the terminal width instead of running off the edge. No new tools or commands; presentation stays in the CLI (the MCP server and domain are untouched). Tool count unchanged (27).

### Changed — CLI output (#153)

- **Tabular CLI output** (#153) — list-style commands render through one `comfy-table`-backed formatting helper in `cdno-cli`: borderless, terminal-width-aware, wrapping long free-text columns rather than letting them overflow, with identifier and badge columns pinned to content width so only the prose column reflows. Migrated `cdno questions` (#188); `portfolio list` and `stewardship list` (#189); the `orient`, `status`, and `commitments` dashboards (#190, which also refactored the shared `commitment_line` into `commitment_cells`); and `portfolio show` evidence plus `search` hits (#191). `comfy-table` is confined to `cdno-cli` — no presentation traits are derived on `cdno-domain` types, and the MCP stdout JSON-RPC channel never touches table code by construction.

## [0.1.9] - 2026-06-13

Patch release: sharper agent-facing slug guidance in three tool descriptions. No behaviour, API, tool, or command changes; tool count unchanged (27).

### Changed

- **Slug guidance in tool descriptions** (#186) — the `create_portfolio`, `file_to_portfolio`, and `create_tracking_entry` MCP tool descriptions now state that slug-typed arguments are not validated (an unknown `project`/`origin` slug is written as a dangling wikilink rather than rejected, and an invented stewardship slug only fails at lookup) and tell the caller to resolve the real slug — via `get_orientation` or the listed valid set — instead of guessing. This is the description-level complement to the self-correcting not-found errors (#180, #181), aimed at the failure mode where an agent invents a slug (e.g. `fitness` for the real `gym`). Adds a regression test pinning the stewardship not-found error to enumerate the valid slugs.

## [0.1.8] - 2026-06-13

Minor release: file non-markdown artefacts — PDFs, images, figures, recordings — as portfolio evidence. The artefact is imported beside a linked markdown stub whose body is an abstract that stands in for it everywhere the bytes can't be read directly. No new tools (the existing `file_to_portfolio` gains an `attach` parameter); tool count unchanged (27).

### Added — non-markdown evidence (#154)

- **File attachments** (#154, #183) — `cdno file --attach <path>` (and the `file_to_portfolio` `attach` parameter) file a non-markdown artefact as evidence: the file is imported into the portfolio at `portfolios/<slug>/<stem>/<filename>` beside a linked markdown stub `portfolios/<slug>/<stem>.md`, and the stub's body becomes the artefact's abstract. The bytes are imported but never indexed — only the stub is — so that abstract is the sole thing search and other agents ever see of the artefact. The media `kind` (`pdf`/`image`/`video`/`audio`/`typst`/`latex`/`file`) is detected from the extension and recorded on the stub. `--move` removes the source after a successful import (the default copies). Import is a create-only, atomic transaction op (`FileOp::Import`) that rolls back on failure; YAML-unsafe sources and angle-bracket filenames are escaped into the stub.

### Changed

- **Attachment-aware portfolio retrieval and lint** (#154, #184) — `get_portfolio_contents` (and `cdno portfolio show`) now surface each evidence note's media `kind`, so a retrieving agent can tell media evidence from prose and knows to dereference the linked artefact; it is omitted for plain prose evidence. `cdno lint` gained a stub-to-artefact-folder pairing check in both directions: an attachment stub whose sibling folder is missing or empty, and an artefact folder under `portfolios/` with no evidence stub (an orphaned attachment, hedged in the message since shape alone can't tell an artefact folder from a hand-made grouping folder).

## [0.1.7] - 2026-06-12

Minor release: self-correcting slug errors and a faster reconcile. No new tools or commands — quality and ergonomics over the existing surface. Tool count unchanged (27).

### Changed

- **Self-correcting slug not-found errors** (#180, #181) — when a slug doesn't resolve, the error now names the valid set, e.g. `file not found: projects/srrogate-model.md — available projects: nfm, surrogate-model, wedding (parked)`. So a caller that guessed wrong — most often an agent driving the MCP server — sees the options and self-corrects instead of retrying blind (the failure that motivated this: a client invented `fitness` when the real stewardship was `gym`). Covers every slug-keyed lookup — projects, portfolios, questions, commitments, and stewardships — through one shared helper, with parked/expanded/done variants flagged and fulfilled commitments excluded. The hint flows out unchanged through both the MCP and CLI surfaces.

### Performance

- **Reconcile mtime fast-path** (#94) — startup reconciliation now skips the read + content-hash for files whose `mtime` and `size` are unchanged since the last index write, instead of reading and re-hashing every `.md` file on every pass. The steady-state win is for CLI verbs that re-reconcile on every invocation (`cdno log`, `cdno capture`). To make cdno's own writes eligible, `VaultTransaction::commit` now stamps the file's real mtime into the index row (it previously stored the entry-build instant, which never matched the file). mtime is a pre-filter only — the content hash stays the source of truth, and a touched-but-identical file is re-stamped so it fast-paths next pass rather than re-reading forever.

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
