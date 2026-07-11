# Changelog

All notable changes to Cuaderno are recorded here. The project is pre-release; entries are grouped by phase milestone rather than version.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Each entry links to the merged PR.

## [Unreleased]

### Changed

- **Note slugs are now globally unique, so a relocated note keeps its backlinks** (#225) — when you
  create a project, action, commitment, question, or custom note whose slug is already taken
  anywhere in the vault, it gets a `-2` (`-3`, …) suffix instead of erroring on the collision. This
  keeps the last-segment wikilink fallback unambiguous: a note that later relocates (an action
  archived to `_done/`, a project parked) no longer loses its `[[type/slug]]` backlinks to a stem
  shared with another note. Note the behaviour shift — creating a second note with the same title now
  succeeds with a suffixed slug rather than raising an "already exists" error. Stewardships are
  deliberately excluded (they don't relocate, and their flat-vs-expanded exclusivity error is a
  useful guard a suffix would mask); the relocating creators still check stewardship stems, so they
  can't collide with one.
- **Backlinks now catch frontmatter wikilinks, not just body ones** (#395) — the index scans a note's
  frontmatter string values for `[[wikilinks]]` alongside its body, so `find_backlinks` (and thus
  `project_backlinks` / `question_backlinks`, `cdno`'s backlink surfaces) now includes a project's
  `core_question:`, a portfolio's `project:`, and an evidence note's `origin:`. In particular, the
  Strategic questions grid's project chips (#354) now populate for the common case — a project that
  answers a question via `core_question:` — instead of only hand-written body references. The scan is
  domain-agnostic (any `[[…]]` in any frontmatter string), so non-link fields contribute nothing.
  Existing vaults pick this up on their next reconcile (every launch). Lint's broken-link scan stays
  deliberately body-only.

## [0.21.0] - 2026-07-11

### Added

- **Backlink chips on the Strategic questions grid** (#354) — each question card now shows chips for
  the notes that reference it: projects that wikilink the question (routing to the project detail
  page), portfolios (routed — both those sharing the question's slug and those linked with a
  different slug via `link_portfolio_to_question`), and evidence (opening in the reader). Backed by a
  new `question_backlinks` domain method (the question-side mirror of `project_backlinks`), surfaced
  on the strategic bundle's question rows. Frontmatter links like a project's `core_question:` aren't
  indexed, so only body-level references appear (see #395); daily-note/action backlinks are kept off
  the card to keep it calm.

## [0.20.1] - 2026-07-11

### Added

- **A "busy, will apply later" note for a deferred config reload** (#384) — when an external
  `config.toml` edit couldn't be applied because the vault was momentarily busy (the #372 case),
  the app had kept the last good config *silently*, so a valid edit could appear to do nothing. The
  desktop app now shows a distinct, calm banner ("the vault was busy, so this config change hasn't
  applied yet — it'll be re-applied on the next change to config.toml") separate from the
  invalid-config banner, so the deferral is visible rather than silent. A successful in-app config
  save (or any successful reload) clears the banner, so it never lingers after the config is fixed.
  Backed by a new `deferred` state on the `config:status` event.

## [0.20.0] - 2026-07-10

Token-cap safety pass over the MCP context tools, with one breaking payload-shape change.

### Changed

- **BREAKING: `get_weekly_context` state changes drop `old_state`** (#351) — each `state_changes`
  entry previously carried a ~200-char gist of *both* the before and after `Current State` bodies.
  The two are ~90% identical, so shipping both discarded exactly the delta a weekly review wants. The
  entry now carries only the `new_state` gist; the previous state is reconstructable from the daily
  log (it is auto-logged before every overwrite). Consumers reading `old_state` must switch to the
  daily log / `read_daily_note`.
- **`get_project_context` payload is bounded** (#352, #388) — the tool's growable fields are now
  capped for token-cap safety, each drop observable and the full data one `read_note` /
  `read_daily_note` away: `recent_mentions` to the 50 most-recent lines, `body_markdown` to a
  generous 20k-char safety valve (a normal map never reaches it; when it does, the cut is marked with
  a trailing `…`), and each `backlinks` group to 100. The CLI and desktop app, which read the same
  domain queries directly, keep the full, uncapped data.

## [0.19.1] - 2026-07-10

### Fixed

- **A busy vault no longer looks like a broken config** (#372) — when the desktop app rebuilt the
  live vault after an external `config.toml` edit, a transiently held vault write lock (a long write
  in flight elsewhere) made `Vault::new` fail, and the app showed the calm "config.toml has an error"
  banner even though the config was perfectly valid. The watcher now distinguishes a genuinely
  invalid config (bad TOML, glob, note-type, or schema) from transient contention: an invalid config
  still shows the banner, while contention retries the reload once and, if still blocked, keeps the
  last good config silently and applies the change on the next config edit — never a false "invalid"
  notice.

### Documentation

- **Document the desktop Config editor** (#377) — a new Getting-started page walks through the
  Config view (Raw/Form, the validate-before-save gate, surgical comment-preserving Form edits,
  conflict detection, and live external reload), and the desktop-app tour and configuration
  reference no longer describe Config as a read-only inspector.

## [0.19.0] - 2026-07-10

Edit your note types and schemas from the desktop app: the Config view gains a structured, editable Form alongside the raw editor, and external `config.toml` edits now apply live.

### Added

- **Edit note types and schemas from a desktop Config form** (#365, PR5a + PR5b) — the Config view
  gains a **Raw / Form** toggle. Raw is the existing `config.toml` editor; the Form side renders the
  parsed config as calm cards and tables (vault meta, each custom `[note_types.<name>]`, each
  `[schemas.<type>]`'s fields) and lets you **add, edit, and remove** custom note types (folder,
  template, append-only, required/optional fields, title/date field) and their schema field
  declarations (type, default, required, allowed values — the allowed-values editor is enabled only
  for a `string` field, mirroring the server rule). Every edit is a **surgical** `toml_edit` rewrite
  of just the one table it touches: comments, key order, the `[variables]` block, and every untouched
  note type/schema are preserved byte-for-byte — the form never re-serialises the whole config. It
  produces a candidate string that flows through the exact same validate -> compare-and-swap ->
  write -> live-reload gate as the raw editor, so an edit from the form can no more brick the vault
  than one from Raw; client-side pre-checks (reserved folders, built-in-name shadowing) keep the UX
  calm while server validation stays authoritative. Backed by `read_config_model` /
  `parse_config_model` projections, a `config_edit` surgical writer in `cdno-core`, the thin
  `config_set_note_type` / `config_remove_note_type` / `config_set_schema_field` /
  `config_remove_schema_field` commands, and `Serialize` + TypeScript bindings on the config field
  model, with the draft/validate/save machinery hoisted into a shared `useConfigDraft` hook.
- **External `config.toml` edits apply live in the desktop app** (#365, PR4) — hand-edit
  `.cuaderno/config.toml` in nvim, the `cdno` CLI, or via sync and the running app now rebuilds its
  live vault from the new config on the fly: added note types, schemas, and folders take effect with
  no restart, and changed `ignore` globs are honoured on the next reconcile. If the edited config is
  invalid, the app keeps the last good config live (it is never left vault-less) and shows a calm,
  non-red banner explaining the edit was not applied; a later valid edit clears it. The rebuild is a
  shared `rebuild_and_swap` core that builds the fresh vault and ignore set BEFORE swapping either
  handle, so a broken edit can never brick the session — the same never-brick guarantee as the
  in-app editor. Template edits under `.cuaderno/templates/` still refresh the affected views but do
  not trigger a registry rebuild.

### Fixed

- **Stuck-project staleness is now counted in local days, not UTC** (#379) — `stuck_projects` and
  the weekly review's "state untouched for N days" compared a note's UTC-based modification date
  against the local `today`, so for any positive-offset timezone in the hours after local midnight a
  project touched "today" was reported as one stale day too many (and, at a zero-day threshold,
  wrongly flagged). The mtime is now converted to its local calendar date before the day
  subtraction, and the stuck-scan cutoff is taken in the machine's local zone — matching the
  machine-local dates the rest of the app uses.

## [0.18.0] - 2026-07-09

Edit your `config.toml` from the desktop app: a Config view that validates before it writes (so it can never brick the vault) and applies the change live via a hot-swappable vault.

### Added

- **Edit `config.toml` from the desktop app** (#365, PR3) — the **Config** view is now an editor,
  not just an inspector: an editable raw pane, on-demand and debounced validation shown inline, and
  a **Save** that runs a hard server-side gate before anything is written. The gate order is
  validate -> compare-and-swap -> write -> live-reload: the candidate is validated FIRST with the
  exact check the app runs when it opens a vault, so a config that would not reopen is refused and
  the file is left byte-identical (the vault can never be bricked from the editor); then a
  content-hash compare-and-swap refuses to clobber a concurrent hand-edit (a distinct "changed on
  disk — reload" notice); then the buffer is written verbatim (comments, ordering, and
  `[variables]` preserved) and the vault is reloaded live, so the edit applies with no restart.
  Backed by a new `save_config(content, expectedHash)` command over a domain
  `Vault::save_config_raw`, and a tagged `ConfigSaveError` (validation / conflict / internal) the
  UI reacts to. The structured form editor remains a later PR.
- **Read-only config inspector in the desktop app** (#365, PR1) — a new **Config** view (Browse
  group) shows the vault's `.cuaderno/config.toml` verbatim in a read-only pane, with a **Check**
  button that dry-runs the exact validation the app runs when it opens a vault
  (`toml::from_str` → `ignore_set` → `TypeRegistry::validate`) and reports a calm "valid" or the
  precise error inline (with the line/column for a TOML syntax error). No editing yet — this is an
  inspector; the raw editor + save (with a hard validate gate and a compare-and-swap against
  concurrent hand-edits) and the structured form land in later PRs. Backed by two pure-read
  commands, `read_config` (content + content hash) and `validate_config(content)`, plus a domain
  `Vault::read_config_raw` and a shared `validate_config_str` dry-run function that the eventual
  save gate reuses, so the inspector's check and the future write can never drift.
- **Live config reload plumbing in the desktop app** (#365, PR2) — an internal `reload_config`
  command re-reads `.cuaderno/config.toml` from disk, rebuilds the vault on the SAME store and
  index (no SQLite reopen), and atomically swaps it in, then emits an all-areas `vault:changed` so
  the UI refetches. This is the plumbing a later config save (PR3) will call to apply an edit
  live, with no restart. No config editing or UI surface ships in this PR.

### Changed

- **The desktop app's managed vault is now hot-swappable** (#365, PR2) — `AppState.vault` became an
  `arc_swap::ArcSwap<Vault>` (from a bare `Arc<Vault>`), with an `AppState::vault()` accessor that
  hands each command an owned `Arc` snapshot. A command already running against the old vault
  finishes cleanly even as a reload swaps a new one in (correct by construction — the loaded `Arc`
  keeps the old vault alive). The reload rebuilds via `Vault::new`, which re-runs the same
  validation the app performs at open, and on any error it keeps the previous vault live rather
  than leaving the session vault-less. `AppState` now also retains the store/index `Arc`s so the
  rebuild reuses them. (A config change to the `ignore` globs does not yet refresh the file
  watcher's compiled matcher — deferred to PR4.)

## [0.17.0] - 2026-07-09

Set a typed frontmatter field without a hand-edit: `cdno frontmatter set` (and the `set_frontmatter` MCP tool) writes the field and the SQLite index atomically, with optional auto-logging. Completes #301.

### Added

- **Generic `set_frontmatter` setter (`cdno frontmatter set`, MCP `set_frontmatter`)** (#301, Phase 2)
  — set a declared, typed frontmatter field on a note *through the index*, so a toggle like the daily
  `meds: true` no longer forces a hand-edit that desyncs `.cuaderno/index.db`. `cdno frontmatter set
  <note> <key> <value>` (and the matching MCP tool, bringing the catalogue to 45) writes the field in
  the same single-transaction read-modify-write the lifecycle tools use, keeping the file and its
  index row consistent. The write is driven purely by the `[schemas.<type>.fields.<key>]` spec:
  the field must be **declared** and marked **`settable = true`** (default-deny — an absent or `false`
  `settable` rejects), its value is **coerced and type-checked** against the declared `type`/`values`,
  and engine-owned keys (`type`, `status`, and a calendar type's period key `date`/`week`/`month`)
  are **hard-blocked** regardless of config so the lifecycle tools stay their sole writers. When the
  field declares `log_on_change = true`, a real change stamps a `key: old → new` line into today's
  daily note in the same commit; an unchanged value is a silent no-op (no write, no log). `note`
  resolves as `today`, a `YYYY-MM-DD` date (both → the daily note), or a vault-relative note path;
  slug resolution for projects/questions and ordered-insert of an absent key are noted follow-ups (v1
  requires the key to already exist). Documented in the CLI reference, the MCP writes reference, and
  the configuration reference (`settable`/`log_on_change` are now live).

## [0.16.0] - 2026-07-09

Config-driven frontmatter (Phase 1): declare typed `[schemas.<type>.fields]`, and their defaults populate at note creation, are recognised by the Templates editor, and are type-checked by lint.

### Added

- **Typed schema fields (`[schemas.<type>.fields]`)** (#301, PR-A) — a built-in note type can now
  declare typed frontmatter fields in `.cuaderno/config.toml`, e.g. `[schemas.daily.fields.meds]`
  with `type = "bool"` (`bool` | `int` | `string` | `date`), an optional static `default`, an
  optional `required` flag, and an optional `values` allowed-set on a `string`. This slice is
  read-only: the fields are **recognised by the desktop Templates editor** (a custom template
  referencing `{{meds}}` no longer warns "renders literally") and **type-checked by `cdno lint`** (a
  present field whose value doesn't match its declared type — or isn't one of `values` — warns; the
  check is opt-in per type). The legacy `[schemas.<type>].extra_required` list keeps working
  unchanged and desugars into the same view as an untyped, non-required string field, so it stays
  lint-only and never becomes a create-time error; on a name clash an explicit `fields` block wins.
  Malformed declarations fail at vault-open: an unknown `type`, a mistyped key, `values` on a
  non-string field, a `default` that doesn't type-check, `list = true` (reserved but unimplemented),
  or a field that shadows an engine-owned key (`type`, or a calendar type's period key). The
  undeclared-key lint is deferred (the correct allowed-set isn't exposed yet). Documented in the
  configuration reference and the templates tutorial.
- **Create-time population of declared field defaults** (#301, PR-B) — a declared field's `default`
  now populates the new note's frontmatter at creation. When a custom `.cuaderno/templates/<type>.md`
  references `{{<field>}}`, the token renders the declared default (e.g. `[schemas.daily.fields.meds]`
  with `default = false` → `meds: false`); a declared field with no default renders `null` rather
  than a literal `{{<field>}}`. Defaults are injected below the note's own create-path values, so an
  engine-supplied value (and a `[variables]` static var of the same name) still wins over a declared
  default — no create surface changes. A name that is also a `[variables.prompt]` variable is left to
  the prompt path, so a supplied prompted answer is never discarded by a schema default. `required`
  remains **inert** in this slice: with no way yet to
  supply a caller value for a built-in's schema field, it does not block creation, so the first
  checkpoint-log of a day cannot fail. The `set_frontmatter` setter and create-time
  `required`-enforcement (the `meds: true` toggle) remain for the next phase of #301.

## [0.15.0] - 2026-07-09

A desktop Templates view: browse, edit, and create note templates, with placeholder help and a calm unknown-placeholder warning.

### Added

- **Desktop Templates view** (#357) — a new `/templates` surface (in the sidebar's Browse group)
  to browse, edit, and create note templates. It lists every note type — the built-ins plus any
  config-defined custom types — with a badge showing whether its template is a custom override, the
  built-in default, or (for a custom type) not yet created. Selecting a type shows its effective
  content in an in-app editor (the app's first free-text file editor); editing and saving writes
  `.cuaderno/templates/<type>.md`, transparently creating a custom override for a built-in-backed
  type on first save (no separate `eject` step). A config-defined custom type with no template yet
  offers **Create**, which scaffolds a starter from the type's declared fields. A side panel lists
  the type's available placeholders grouped by source (supplied, schema fields, config variables,
  prompted), and the editor flags any unrecognised `{{token}}` with a calm, non-blocking notice —
  saving is never blocked. For a config custom type the known set includes its
  `[note_types.<name>]` required and optional fields, so its own schema fields never false-warn. An
  external edit to a template file refreshes the view. New domain methods (`list_templates`,
  `read_template`, `save_template`, `create_template`, and a `Schema` placeholder source) back the
  matching Tauri commands.

## [0.14.0] - 2026-07-09

The MCP server now logs its resolved local timezone at startup, so a silent UTC fallback is visible immediately.

### Added

- **MCP server logs the resolved local timezone at startup** (#309) — `cdno-mcp-server` (and the
  stdio `cdno-mcp`) now log the process's resolved local UTC offset and a sample `Local::now()`
  next to the "vault opened" line, e.g. `local_offset=+02:00 sample_local_now=…`. The server
  timestamps daily/tracking entries in process-local time, so a silent UTC fallback on a host with
  no `tzdata`/`TZ` (the 2026-07-06 incident: remote logs landed two hours behind the wall clock)
  is now visible from the boot logs instead of surfacing days later as wrong timestamps. The line
  is factual — a host legitimately in UTC does not raise a warning. The HTTP-server reference page
  documents the `TZ`/`tzdata` deployment requirement.

## [0.13.0] - 2026-07-08

Two daily-use fixes: custom daily templates render `day_name`/`week`, and `get_weekly_context` stays under the MCP token cap.

### Fixed

- **Daily scaffold now supplies `day_name` and `week`** (#300) — a customised
  `.cuaderno/templates/daily.md` referencing `{{day_name}}` or `{{week}}` used to render those
  placeholders literally, because the daily scaffold supplied only `date`, `heading`, and
  `weekday`. The scaffold (shared by `append_to_log` and `upsert_daily_section`) now also fills
  `day_name` — an alias of `weekday`, the weekday name — and `week`, the ISO-week label `YYYY-Www`
  reusing the exact formatting the weekly scaffold uses, so a daily note's `week:` frontmatter
  matches the weekly note it points at (correct across a year boundary — e.g. 2025-12-29 is
  `2026-W01`). `cdno templates vars daily` and the template docs now list the full daily set:
  `date`, `heading`, `weekday`, `day_name`, `week`.

- **`get_weekly_context` is now bounded to fit the MCP client's token cap** (#298) — the payload
  could overflow the client's max-token limit. Each project state change now carries a ~200-char
  gist of its before/after `## Current State` rather than the full bodies, and `logs` is capped to
  the 100 most-recent lines. Non-breaking — the DTO field shape is unchanged.

## [0.12.0] - 2026-07-08

Desktop polish: portfolio chips on questions, column charts for count-style trends, and a disambiguation picker instead of a dead-end toast.

### Added

- **Desktop: portfolio link chips on the Strategic questions grid** (#339) — each question card
  on `/strategic` now surfaces a chip row for its linked portfolio(s), so a question links out to
  the evidence gathered against it. Portfolios are correlated to a question entirely client-side:
  a portfolio shares its question's slug, and the strategic bundle already returns the portfolio
  list, so a question's chips are the portfolios whose `slug` matches the question's `slug` — no
  backend change. Clicking a chip navigates to the portfolio detail route, mirroring the
  commitments timeline's `OriginChip`. The question card, previously a single full-card button,
  is restructured into a plain container so the title stays the reader-opening control and the
  chips are accessible sibling links (interactive elements must not nest inside a button). v1 is
  portfolio chips only; project-link/backlink chips need a new `question_backlinks` domain method
  and are deferred to a follow-up.

- **Desktop: column chart variant for stewardship trends** (#337) — Stewardship Detail now draws
  count/volume series (reps, laps, sessions) as calm columns rather than lines, while continuous
  measures (a weight, a pace) keep the line. The mark is chosen per series from a robust,
  non-semantic signal — a series whose every tracking value is integer-valued reads as a column,
  any fractional value keeps the line — so no domain change is needed (`TrackingSeries` still
  infers no column semantics) and a misclassification is purely cosmetic. The column obeys the
  same design laws as the line: same context hue, no target lines, no red, reduced-motion aware,
  same height and axes.

- **Desktop: milestone/action disambiguation picker** (#338) — a free-text selector that matches
  more than one candidate (completing a milestone or action, resolving a waiting-on blocker) now
  opens a calm picker of the candidates instead of a dead-end "be more specific" toast. Choosing
  one re-invokes the same command with that exact string, so the write completes in place. A new
  shared `useAmbiguityResolver` hook centralises the branch-on-`kind` + re-invoke logic (each write
  site hands it a `mutateAsync` re-invoke, reusing that mutation's own success/rollback/toast
  path), and an `AmbiguityPicker` renders over the same centred Radix `Dialog` as the project-cap
  modal — focus trap, Esc, and return-focus for free, reduced-motion honoured, no red. Wired into
  the Commitments timeline, Project Detail, and the cross-project Actions list.
  An ambiguous *slug* (no candidates to choose between) falls through to the normal toast. UI-only;
  the backend already carried `CmdError::Ambiguous { query, candidates }`.

## [0.11.0] - 2026-07-08

The journal grows a month view: a monthly note type and a desktop calendar to browse daily notes.

### Added

- **Desktop: calendar view for daily notes** (#340) — a new `/calendar` surface (sidebar entry
  between Actions and Commitments). A from-scratch, seven-column, Monday-first month grid marks
  the days that have a daily note with a calm dot (never red), is keyboard-navigable with a
  roving tabindex (arrow keys step by day and week, clamped within the month), and pages by
  month. Clicking a day loads its note into an **embedded panel** (deliberately not the shared
  NoteReader overlay, which has no navigation chrome) that renders the markdown read-only via
  the shared renderer, with quick jumps to the **previous day**, **next day**, the day's
  **week**, and its **month** — each landing note read at a date the *backend* stamped, so the
  frontend never computes a domain date (§3.7). A day, week, or month with no note shows a calm
  empty state carrying its open-in-editor path, not an error. New Tauri commands `read_daily`
  (the note plus `prev_date` / `next_date` / `week_of` / `month`), `read_weekly` and
  `read_monthly` (raw note reads, distinct from the composed `get_weekly_bundle`), and
  `list_daily_dates` over a new `Vault::daily_dates_in_month` domain query that scans only the
  requested month's daily directory. The watcher's path classifier now maps
  `journal/<year>/monthly/…` to a new `VaultArea::Monthly` (daily and weekly already had their
  own areas), so a calendar edit or an external nvim/CLI edit to any daily, weekly, or monthly
  note refreshes the calendar live.
- **Monthly note type + `cdno review monthly`** (#228) — a new first-class `monthly` note
  type, mirroring the weekly-note seam throughout. One artefact per calendar month at
  `journal/<year>/monthly/<YYYY-MM>.md`, keyed by month so any day resolves to the same note.
  The `MonthlySection` set is celebration-first and lean — `Wins`, `Themes`, `Next Month's
  Focus` — with **no Metrics section** (quantitative metrics stay behind the desktop "show
  metrics" toggle, a design law). The scaffold's `## Weeks` block **links, doesn't copy**: one
  wikilink per Monday falling within the calendar month, pointing at that Monday's weekly note,
  so the weekly notes stay the source of truth. New domain seam `Vault::read_monthly_note` +
  `Vault::upsert_monthly_section` (mirroring the weekly read/section-write pair, absence is
  non-error), core path helpers `journal_monthly_dir` / `monthly_note_relpath`, `cdno monthly`
  (read the note) and `cdno review monthly` (interactive compose; lists sections when
  non-interactive), and the `read_monthly_note` + `upsert_monthly_section` MCP tools (44 tools
  total). Monthly participates in reconcile, index, search, backlinks, normalise, and lint
  exactly as weekly does, recognised by its `type: monthly` frontmatter. This brings the vault
  to twelve built-in note types.

## [0.10.0] - 2026-07-08

Quieter internals - lint now catches malformed stewardship-dashboard bullets that used to vanish silently, and the desktop watcher stops echoing the app's own writes back as external edits.

### Added

- **Lint surfaces malformed stewardship-dashboard bullets** (#312) — `Vault::lapsed_habits`
  and the periodic-commitment parser both skip dashboard lines they cannot parse *by design*,
  so a hand-typed near-miss (an ASCII hyphen or en-dash where the em-dash belongs, a missing
  `next:` marker, an unparseable date) used to vanish from the lapse scan and the commitments
  aggregation with no diagnostic anywhere. `cdno lint` (and the `lint` MCP tool) now scans each
  stewardship's `## Active Habits` and `## Periodic Commitments` sections and emits a `warning`
  for every bullet the canonical parser rejects, pointing at the stewardship and the offending
  line with a cheap hint at the likely typo. Acceptance is delegated to the same parsers the
  scans use — never a parallel regex — so the grammar stays canonical and lint can never drift
  from it; only list bullets are checked, so section prose and headings are untouched.

### Fixed

- **Desktop write journal now suppresses exactly the paths a write touched** (#315) — the
  Tauri layer's `WriteJournal` records the paths this process wrote so the file-watcher can tell
  its own echoes from external edits. It used to reconstruct that set by hand from a single
  primary path the domain returned, which leaked three ways: a `complete_action` that archived an
  attached note (`actions/<slug>.md` -> `actions/_done/<year>/<slug>.md`) never journalled those
  two paths, so the watcher echoed them back and forced a redundant refetch; an
  `update_project_state` that silently no-ops on unchanged text still journalled the project and
  daily paths, planting a false suppression entry that could swallow a genuine external edit for
  the ~2s echo window; and the daily-note path was rebuilt client-side, duplicating a domain
  rule. Domain write methods `complete_action` and `update_project_state` now return a
  `WriteOutcome { primary, paths }` (`VaultTransaction::commit` reports the touched file set),
  so the desktop layer journals the exact paths the domain wrote — archival moves included — and
  skips journalling and its self-change emit entirely on a no-op. CLI and MCP surfaces are
  unchanged: they read only `outcome.primary`.

## [0.9.0] - 2026-07-08

The vault picker - the app asks for its vault instead of requiring launchctl.

### Added

- **Desktop-app vault picker for Finder launches** (#331) (#332) — a Finder-launched app
  inherits no shell environment, so the app used to abort silently (to Console.app) unless
  `launchctl setenv CUADERNO_VAULT_PATH ...` had been run. Startup now resolves the vault through
  three layers: the `CUADERNO_VAULT_PATH` override (unchanged, and still fails loudly on a bad
  path), then a persisted setting at `<app_config_dir>/vault.json`, then a native folder picker
  (`tauri-plugin-dialog`). The picker validates that the chosen folder is a cuaderno vault
  (`.cuaderno/` marker), re-asks on a non-vault, remembers a valid pick, and — on cancel —
  explains that the app needs a vault and exits gracefully instead of a silent abort. A stored
  path that fails to open — whether it fails the cheap `.cuaderno/` marker check (vault
  moved/deleted) or passes it but fails the full open (corrupt config, unopenable index, or a
  TOCTOU delete) — falls through to the picker rather than crashing; only the explicit env
  override still hard-fails. The main window stays hidden until initialisation completes, so a
  first-launch no-vault error no longer flashes on screen before the picker appears. The
  `launchctl` step in the install docs and cask caveats becomes optional. Resolution ordering
  lives in a testable `vault_locator` seam (`crates/cdno-tauri/src/vault_locator.rs`); the dialog
  loop stays in `lib.rs`.

## [0.8.1] - 2026-07-08

First fix from real-device user testing.

### Fixed

- **Action bullets no longer double their energy tag or leak wikilink syntax** (#328) -
  the stored bullet text is the verbatim matching key (it carries the `(deep)` suffix and any
  raw `[[target|label]]` link), and the views rendered it as-is next to their own energy tag.
  A display-only `actionLabel` helper strips the suffix and renders wikilinks by label or last
  path segment across Home cards, the Actions view, and Project Detail; mutations still match
  on the raw text. Spotted in v0.5.0 user testing.

## [0.8.0] - 2026-07-08

The packaging-remainder milestone (M10) - the desktop-app plan (M0-M10) is complete.

### Added

- **M10 packaging remainder (plan §5 M10 — the final desktop-app milestone)** (#326) — the
  polish slice left after the dmg/ad-hoc-signing/release pipeline shipped with v0.5.0–0.7.0.
  Phase 5/6 UI is now COMPLETE. Deliberately left out of v1: notarization, an auto-updater, an
  NSPanel capture overlay, and an Intel dmg.
  - **Menu-bar tray** (`crates/cdno-tauri/src/tray.rs`, plan §1.0): Quick capture (same handler as
    the global shortcut — show + focus the capture window, emit `capture:show`), Open cuaderno,
    Quit. Minimal and calm — no counts, no status swaps; icon reuses the codegen-embedded app icon
    (workspace `tauri` dep gains the `tray-icon` feature); a tray failure logs a warning and never
    aborts startup.
  - **Degraded-watcher pill + poll fallback** (plan §3.1): the backend's `watcher:status` events —
    emitted with every batch but consumed by nothing until now — land in a
    `ui/src/lib/watcherStatus.ts` module store; while degraded the sidebar footer shows a muted
    grey "live updates paused" pill and all queries are invalidated every 60s (event-driven refresh
    can't be trusted after a notify overflow/failed reconcile), both cleared on the next healthy
    batch.
  - **Day-change QA**: the event bridge is now under test (`ui/src/api/events.test.ts`) —
    `clock:day-changed` invalidates the date-dependent queries, `vault:changed` fans out through
    the area map, `watcher:status` reaches its callback. No Rust-side clock test: `clock.rs` is a
    sleep-loop around `Local::now()` with no extractable pure logic.
  - **A11y pass** (M2/M5 keyboard criteria): `vitest-axe` smoke tests over Home, Commitments, and
    Strategic (`ui/src/views/a11y.test.tsx`; `color-contrast` excluded — jsdom cannot paint). One
    real finding fixed: the Commitments view skipped from `h1` to the shared timeline's `h3` month
    headers — `CommitmentsTimeline` now takes a `monthHeading` level prop.
  - **Docs**: README gains the desktop-app install (cask with `--no-quarantine` + the `launchctl`
    vault-path step) and drops the long-stale "scaffolded" phrasing; new docs-site page
    "Desktop app" (`getting-started/desktop-app.md`) covers install, the two launch caveats, the
    vault env var, and a tour of the views.

## [0.7.0] - 2026-07-08

The Strategic view milestone (M9 of the desktop-app plan) - the final view; every route in the app is now real.

### Added

- **Strategic / Monthly view (M9, plan §1.5; closes #57)** (#324) — the `/strategic` route
  replaces the placeholder with the in-app monthly review: a questions grid (by domain), the
  button-based five-slot project allocator with a gentle cap modal, a portfolio-health table,
  a stewardship overview with habit sparklines, and the six-week commitments timeline — all painted
  from one composed read.
  - **Backend** (`crates/cdno-tauri/src/commands/strategic.rs`): `get_strategic_bundle()` composes
    the active questions, the portfolio-health rows (reusing `list_portfolios`), the active + parked
    project slots (a thin `ProjectSlot { slug, context }` view over the project frontmatter), the
    configured active-project cap (read from `config.vault.max_active_projects` — never hardcoded),
    a stewardship overview row per stewardship (its `StewardshipSummary` paired with a precomputed
    12-week habit sparkline), and the six-week (42-day) commitments window. The sparklines are
    **entries-per-ISO-week counts computed backend-side** from each expanded stewardship's tracking
    dates (plan §3.7: the frontend does no date maths); flat stewardships get an empty sparkline.
    The allocator's park/activate reuse the existing M5 `park_project` / `activate_project` commands,
    whose `ProjectCapReached` error already carries the active slugs the cap modal lists.
    `QuestionSummary`, `QuestionDomain`, and `QuestionStatus` gained `ts-rs` derives.
  - **Frontend** (`ui/src/views/strategic/Strategic.tsx`): questions group into a research/life
    grid, each card opening the question note in the shared reader. The allocator draws `max_active`
    slots — the filled ones first, then soft dashed "open slot" placeholders (breathing room, not
    vacancy) — with a quiet per-slot "park" button and a parked shelf of "activate" buttons below.
    An over-cap activate opens a gentle centred modal ("Room for five. Park one to make space.")
    listing the active projects with inline park buttons — no red anywhere. The portfolio-health
    table renders staleness as the same neutral emphasis tiers as the M8 browser (now a shared
    `lib/staleness.ts` helper, so the two surfaces can't drift). The stewardship overview shows a
    context-hued 24px sparkline per tracked stewardship, and the six-week commitments timeline reuses
    the shared `CommitmentsTimeline` in read-only mode.
  - **Chart extraction** (`ui/src/components/charts/TrendChart.tsx`): the `TrendChart` (and the
    `usePrefersReducedMotion` hook + `SERIES_COLORS`) moved out of `StewardshipDetail` into a shared
    charts module, joined by a new tiny `Sparkline` (line-only, no axes, animation off under reduced
    motion). `StewardshipDetail` keeps working via the new import. A minimal centred `Dialog`
    primitive was vendored under `components/ui/` following `sheet.tsx`'s Radix pattern.

## [0.6.0] - 2026-07-08

The Portfolio Browser milestone (M8 of the desktop-app plan).

### Added

- **Portfolio Browser (M8, plan §1.6; closes #58)** (#322) — the `/portfolios` and
  `/portfolios/:slug` routes replace the placeholder: a calm selector of per-question evidence
  dossiers and a per-portfolio detail with an evidence timeline, a links sidebar, and the app's only
  note-creation form (evidence quick-add, sanctioned by #58).
  - **Backend** (`crates/cdno-tauri/src/commands/portfolios.rs`): `list_portfolios()` (today stamped
    in Rust) returns one `PortfolioSummary` per portfolio (evidence count, last-updated, staleness);
    a composed `get_portfolio(slug)` bundles the unifying question, the linked project and the
    related questions (the project comes from frontmatter, the questions are scanned from the
    `_index.md` body's `[[questions/…]]` wikilinks — that link lives in the body, not the
    frontmatter), and every filed evidence note as a wire-ready row (path, created, source, origin),
    newest-first. `add_evidence(portfolio, source, origin, content)` wraps `file_evidence_with_vars`
    (mirroring the MCP `file_to_portfolio` plain-evidence path) and emits the Portfolios area.
    Stored wikilinks are lowered to bare navigable targets for the frontend. `PortfolioSummary`
    gained `ts-rs` derives.
  - **Origin validation (a deliberate tightening over the MCP tool):** the domain does not validate
    an evidence `origin`, so a wrong slug writes a dangling `[[…]]` link that only lint later
    notices. The desktop composer free-texts `origin`, so `add_evidence` resolves it first via
    `resolve_wikilink` and refuses an unresolvable target with a calm `Invalid` — the GUI cannot
    write a dangling link. The CLI/MCP surfaces keep the looser contract for scripted callers.
  - **Frontend** (`ui/src/views/portfolios/`): the selector lists each portfolio's question, an
    evidence count, and a staleness line rendered as *neutral* emphasis — fresh sits at full ink,
    ageing fades to ink-muted, long-dormant recedes to ink-faint, with a "last updated N d ago"
    hover title — never a hue (colour is identity, never urgency, and no semantic green/red token
    exists). The detail collapses the plan's three panes to two: an evidence timeline (each row opens
    the note in the shared reader; its origin chip opens the producing note) and a links sidebar
    (project → `/projects/:slug`, questions → reader). The quick-add composer is an inline (not modal)
    slide-down form — source + origin (both required, origin hinted as "must name an existing note")
    + content — that surfaces the invalid-origin message inline. Empty portfolios invite their first
    artefact. Both surfaces code-split onto navigation.

## [0.5.0] - 2026-07-08

Desktop app: M1–M7 of the Tauri UI — scaffold, Home interactions, global capture, Commitments Timeline, Project Detail / Actions / note reader / command palette, Weekly Review, and Stewardship Detail — on top of M0's UI-groundwork domain queries and file watcher. M0 (#311) and M1–M7 all landed after the v0.4.0 tag, so they ship here; the CLI and stdio/HTTP MCP surfaces are otherwise unchanged.

### Changed

- **MCP wire shape** (#317): `get_commitments` source objects of kind `standalone_commitment` now carry
  a `slug` field (additive; the `kind` tag is unchanged). Consumers that deserialise the source
  strictly should allow the new field.

### Added

- **Stewardship Detail (M7, plan §1.7; closes #59)** (#320) — the `/stewardships` and `/stewardships/:slug`
  routes replace the placeholder: a calm list of perpetual responsibilities and a per-stewardship
  dashboard with read-only trend charts, recent tracking, and an inline log form.
  - **Backend** (`crates/cdno-tauri/src/commands/stewardships.rs`): `list_stewardships()` (today
    stamped in Rust) and a composed `get_stewardship_detail(slug)` — the dashboard body, the numeric
    `series` (from the existing `tracking_series`; empty for a flat stewardship), the last five
    tracking entries (a wide-window `list_tracking` scan, newest-first) with a `tracking_count`, all
    in one invoke. `get_tracking_template_fields(activity)` reports the prompted fields the log form
    must gather — it calls `Vault::template_prompts("tracking", slugify(activity))`, which resolves
    `tracking-<activity>` with a fallback to the generic template exactly as `add_tracking_entry`
    does, so the fields match what the later create call enforces (the generic template has none).
    `log_tracking_entry(...)` wraps `add_tracking_entry_with_vars`; it touches only the new tracking
    note (no daily-log line is staged by that domain call), so that single path is journalled and the
    Stewardships area is emitted. Filing on a flat stewardship and a same-day duplicate surface as
    calm `Invalid` toasts. `TrackingSeries`/`TrackingPoint` gained `ts-rs` derives.
  - **Frontend** (`ui/src/views/stewardships/`): the list shows a context dot, name, variant chip
    (expanded/flat), and a muted staleness line ("last tracked N days ago" / "no tracking yet" /
    "dashboard only"). The detail renders the dashboard body through the shared `Markdown` component
    (wikilinks route as elsewhere), then — for an expanded stewardship with numeric tracking — a
    Trends pane of compact Recharts line charts (one per series, ~160px, colours drawn from the
    context-hue CSS variables, no grid, no target/reference lines, animation off under
    `prefers-reduced-motion`). Recent entries open in the note reader; a Log Entry button reveals an
    inline (not modal) form whose dynamic fields are fetched (debounced) from
    `get_tracking_template_fields` for the typed activity. Charts ship as core status visualisations
    (§1.7), *not* gated behind the metrics toggle; the denser metrics variant is deferred.
  - **Known limitation (from #59's comment thread, still open):** the issue named an "exercise volume
    per week" chart (sets × reps × weight). `tracking_series` sums each table column independently, so
    a cross-column product like volume is not derivable from it; it would need a computed volume
    column in the gym tracking template or a richer domain query. Not built here — the shipped charts
    are weight-over-time and per-column count trends (Sets/Reps totals).

- **Weekly Review (M6, plan §1.4; closes #55)** (#319) — the `/weekly` route is now a guided, deliberately
  anti-chore 5-step flow, backed by one composed `get_weekly_bundle` read and a single
  `save_weekly_section` write.
  - **Backend** (`crates/cdno-tauri/src/commands/weekly.rs`): `get_weekly_bundle(week_of?)` composes
    the whole review in one invoke — the existing weekly note's four sections (parsed so the UI
    prefers written content over a fresh seed), next week's Monday (`next_week_of`) and its
    existing goal (`next_week_goal`) for the Focus step, the week's completed actions and (capped,
    most-recent-first-kept) daily logs for the wins seed, the active-project scan
    (`ProjectSummary`, context riding along), the stuck-project set with each project's day count,
    the commitments lookahead (14 days forward plus the domain's 30-day overdue look-back), and
    the stewardship scan. `save_weekly_section(week_of?, section, content)` wraps
    `upsert_weekly_section` (compose/overwrite); the `section` wire string is kebab-case —
    `"wins" | "challenges" | "one-improvement" | "this-weeks-goal"` (the plan sketch's
    "Next Week's Focus" maps to the domain's `ThisWeeksGoal`) — and an unrecognised value is a
    calm `Invalid` naming the valid sections. A new `stuck_project_days` domain query returns the
    stuck set with day counts; `StewardshipSummary`/`StewardshipVariant` gained `ts-rs` derives.
    **Week-target semantics**: the Wins/Challenges/One Improvement saves target the note of the
    week under review (`week_of`); the Focus step's goal targets the note of the FOLLOWING week
    (`next_week_of`) — a Sunday review writes its "next week's focus" into next week's
    `This Week's Goal`, never overwriting the goal of the week being reviewed.
  - **Frontend** (`ui/src/views/weekly/`): a non-linear stepper — the progress dots (24px+ hit
    targets, the current one ringed, `aria-current`) jump anywhere, and all five steps stay
    mounted with visibility toggled, so unsaved drafts survive the jumps. Each step's save is
    complete in itself; the stop-anywhere reassurance is two-tier — "you can stop here — it's
    already saved" after an actual write, the softer "you can stop anytime — nothing here demands
    finishing" when only read-only looked-at marks exist. No "N of 5" counter unless the metrics
    toggle is on. Step 1 seeds an editable Wins composer from completed actions
    (`- Completed: {title} ({project})`) plus a few log lines (empty weeks get the calm "what felt
    like progress anyway?" prompt); step 2 is the inline Current State scan with a muted "state
    untouched for N days" line on stuck projects; step 3 (stewardships) and step 4 (the lookahead,
    reusing the shared `CommitmentsTimeline` with its new `readOnly` prop — origin chips stay,
    done buttons don't) are read-only with a local "looked at" affordance; step 5 saves next
    week's single focus into next week's note (seeded from `next_week_goal`), with quick-pick
    buttons from the active project slugs. The route is lazy-loaded like the other heavy views;
    `get_weekly_bundle` is invalidated from the weekly area and from the projects, daily,
    commitments, and stewardships areas the bundle composes.
- **Project Detail, Actions view, note reader, and command palette (M5, plan §1.0/§1.2/§1.8; no
  GH issue for Project Detail or the Actions view — both are plan deltas; the palette is plan §1.0;
  delivers the standalone-chip-to-reader bullet that M4 deferred when it closed #56)** (#318) — the
  desktop app's navigation targets now resolve to real views, and every note is one click from
  being read.
  - **Note reader** (`components/markdown/NoteReader.tsx`): a 380px Radix-dialog slide-in
    (focus trap, Esc-to-close, return-focus) that renders any vault note — title, flat scalar
    frontmatter chips, the markdown body, and an "Open in editor" footer. Hosted once in the shell
    behind a small `ReaderProvider`/`useReader()` context (plan §6), so the timeline, palette, and
    backlinks anywhere open it without prop drilling. Markdown is `react-markdown` + `remark-gfm`
    (tables, task lists rendered read-only) plus a hand-rolled `remarkWikilinks` plugin turning
    `[[target]]`/`[[target|label]]` into in-app links (adjacent links and aliases handled;
    unparseable syntax stays literal). A wikilink resolves via `resolve_wikilink`: a project routes
    to its detail, a stewardship to the list, anything else replaces the reader in place; an
    unresolved target renders muted and does nothing. External `http(s)` links render as muted text
    (the webview has no shell-open capability wired).
  - **Project Detail** (`/projects/:slug`): the full project map — inline Current State editor,
    Next Actions (tick + energy-tagged quick-add), Waiting On (add + text-matched resolve
    quick-rows), Milestones (tick, `hard:` chip + date), grouped backlinks and "recently in your
    logs", and the whole map body rendered below a divider so nothing is hidden. Header carries
    park/activate (the `ProjectCapReached` error surfaces as a calm toast) and open-in-editor. A
    parked project renders read-only. Home's project cards gained the quiet "open" link the M2
    deferral promised for when this route shipped.
  - **Actions view** (`/actions`): a single cross-project list grouped by project with a
    single-select energy filter (empty groups drop out); each row ticks done (optimistic), promotes
    an unattached bullet to a note, or opens an attached note in the reader. Empty state: "No open
    actions anywhere. Enjoy it."
  - **Command palette** (`⌘K`, `cmdk`): a Navigate group (static views + debounced `search_vault`
    note results, routing to project/stewardship/reader) and two verbs — "Capture…" and
    "Log to daily…" — that switch to a one-line submit into the vault. Styled from the semantic
    tokens; Esc closes and returns focus. A sidebar "Search & jump ⌘K" affordance opens it too.
  - **Commitments Timeline**: the standalone origin chip now opens the commitment note in the
    reader — the one acceptance bullet M4 deferred when it closed #56 — replacing the plain-text
    placeholder.
  - New frontend deps (Bun): `react-markdown` 10, `remark-gfm` 4, `cmdk` 1.1, `@radix-ui/react-dialog`
    1.1 (the first vendored Radix-backed primitive, a shadcn-style `Sheet` under
    `components/ui/`).
  - **Backend surface (this PR):** the Tauri command layer this milestone consumes lands here too.
    New reads — the composed `get_project` (ProjectDetail: typed frontmatter, body, actions, open
    milestones, grouped backlinks, and recent log mentions in one invoke), `list_all_actions`
    (the cross-project Actions list), `search_vault`, and `read_note`. New writes — `add_action`,
    `promote_action`, `add_waiting_on`, `resolve_waiting`, `park_project`, and `activate_project`
    (the last surfacing `ProjectCapReached` for the allocator modal). Plus a new domain method,
    `Vault::resolve_wikilink`, which resolves a single clicked `[[target]]` for UI navigation by
    delegating to the batch core resolver (`cdno_core::extractors::resolve_wikilinks`) with a
    one-element input, so the two paths can't drift.

- **Commitments Timeline (closes #56)** (#317) — the full promises view lands at `/commitments`: a
  strictly chronological vertical list of every dated commitment from all four sources (project
  milestones, stewardship periodic commitments, standalone commitment notes, and self-due action
  notes), 90 days out by default. Upcoming entries group under month headers; past-due entries
  collapse into a calm, collapsed-by-default "a few slipped past" group at the top. Colour signals
  context, never urgency — a past-due row keeps its context hue and earns only a secondary
  desaturated-amber accent (a thin left border and a `planned for …` date label; never "overdue",
  never red). A row of seven context-filter chips narrows the list client-side (multiple active,
  empty means all). Standalone commitments and project milestones carry a quiet **done** button
  (optimistic removal, rollback on error, `Done: {title}.` on success); periodic and action-note
  sources are read-only here (periodic shows cadence; actions complete from Home). Origin chips
  link project- and stewardship-backed entries to their detail; a standalone reads as plain text
  until the note reader arrives (M5). Empty state: "Nothing promised in this window." — emptiness
  is success. Three new commands — `get_commitments` (returns a `today`-stamped `CommitmentsView`),
  `complete_commitment`, `complete_milestone` — follow the established write-command pattern
  (`WriteJournal` record + `origin: self` emit after commit); the `projects` invalidation area now
  also refreshes the timeline, since a milestone edit changes both. The shared `CommitmentsTimeline`
  component is built for M6's lookahead and M9's six-week views to reuse. `CommitmentSource`'s
  standalone variant now carries the commitment's own slug (so the done button can complete it
  without re-deriving it) — a change rippling through the CLI `orient` label, the MCP
  `CommitmentSourceDto` mirror, and the aggregation site.

- **Global capture + inbox drawer (M3, no GH issue — plan delta)** (#316) — capture a thought from
  anywhere with `⌘⇧C` (registered in Rust via `tauri-plugin-global-shortcut`; SUPER+SHIFT maps to
  Cmd on macOS). The hotkey summons a dedicated undecorated, transparent capture window whose
  minimal entry never loads the SPA: a single input where **Enter** captures to `inbox/`,
  **⌘/Ctrl+Enter** logs to today's daily, and **Esc**/blur hides — a sub-second "captured" /
  "logged" flash confirms, then the window auto-hides. A right-side **inbox drawer** (toggled from
  the sidebar, with a grey — never red — count badge) is the visible landing place that makes
  capture trustworthy: each item shows its text and captured date with per-item open-in-editor and
  optimistic discard; full triage stays a CLI/Claude concern. New commands `capture_quick`,
  `log_quick`, `list_inbox`, `discard_inbox_item`, and `open_in_editor` follow the established
  write-command pattern (`WriteJournal` record + `origin: self` emit after commit); `open_in_editor`
  validates its vault-relative path through `VaultPath` before joining the vault root, the one file
  access that bypasses the domain layer. `tauri-plugin-single-instance` (registered first) focuses
  the running window instead of launching a duplicate; `tauri-plugin-opener` backs
  open-in-editor. `InboxItem` gained `ts-rs` bindings.

- **Home view interactions (closes #54)** (#314) — the desktop app is now daily-usable. Energy selector
  (deep/medium/light) filters each card's surfaced action with the no-match rule: a card never
  blanks — it keeps its best-available action behind a muted "smallest step" note. Start logs
  `started [[project]] — action` to today's daily note; done completes the bullet with an
  optimistic cache update (rolled back on error); the Current State snippet is an inline editor
  backed by `update_project_state` (previous state auto-logged). Write commands record their
  touched paths in the `WriteJournal` and emit `origin: self` change events, so the watcher
  suppresses their echoes. Arrow/`j`/`k` keys rove focus across the card grid. A calm toast
  surface (no red; amber edge for attention) reports errors and completions. The commitments
  strip is colour-coded by context — `CommitmentEntry` now carries the owning note's life
  context, resolved per source — and renders friendly short dates. The **"show
  metrics" toggle** (plan §3.11) lands default-off next to the theme switch — its first surface
  is an open-actions count pill on project cards. IPC round-trips now cover args marshalling
  (including the `new_state`→`newState` camelCase seam) and the serialised error contract.

- **Desktop app scaffold (closes #53)** (#313) — new `cdno-tauri` crate + `ui/` frontend (React 19,
  Vite 7 multi-page, TypeScript, Tailwind v4, Bun as package manager / Node as runtime). The app
  opens the vault named by `CUADERNO_VAULT_PATH`, runs startup reconciliation, registers
  `Arc<Vault>` as managed state (no wrapper lock — writes serialise on the transaction's
  cross-process lock), and serves a styled shell whose Home view renders the live orientation
  (commitments strip, project cards with context dots, lapsed-habits line). A dedicated watcher
  thread turns debounced filesystem events into `vault:changed` area events with self-echo
  suppression (`WriteJournal`), backed by focus-refetch and a day-change ticker. TypeScript
  bindings are generated from the Rust wire types via `ts-rs` (`just gen-bindings`). Commands so
  far: `get_orientation`, `get_today`; every command is async and routes domain calls through
  `tauri::async_runtime::spawn_blocking`. Vault opening was lifted into
  `cdno_domain::bootstrap::open_vault` (typed `BootstrapError`), with `cdno-mcp`'s bootstrap now
  delegating to it.

- **Lapsed habits in orientation** (#311) — `cdno orient` / `get_orientation` now surface stewardship
  habits whose `## Active Habits` line declares a lapse (e.g. `- Swimming 1x/week — lapsed since
  March`). The dashboard prose is the source of truth; no cadence inference. Previously the
  `lapsed_habits` field existed but was always empty.
- **`FileWatcher` trait + `FsFileWatcher`** in cdno-core (#311) — debounced (400ms), vault-relative
  filesystem events over `notify`, groundwork for the desktop app's live-refresh pipeline (#53).
  Events are hints (`Changed`/`Removed`/`Rescan`); consumers stay correct by re-running
  reconciliation.
- **Domain queries for UI surfaces** (#311) — `Vault::read_note` (frontmatter + body + note type for
  reader panes), `Vault::tracking_series` (numeric time series lifted from tracking-note tables
  via a new cdno-core first-table extractor, for trend charts), and `Vault::start_action` (logs
  `started [[project]] — action` to the daily note, single-sourcing the format across CLI, MCP,
  and the future desktop Start button).

## [0.4.0] - 2026-07-07

Remote serving: a Streamable HTTP transport for `cdno-mcp`, origin-side Access-JWT validation, and write safety for concurrent writers.

### Added

- **Write safety for concurrent writers** (#306) — three layers, sized for the remote-serving era:
  - **Atomic content writes in cdno-core**: `FsVaultStore`'s `write_file`/`append_to_file`/
    `import_external` now materialise content in a temp file in the target's directory, fsync it,
    and `rename(2)` it over the destination — a reader never observes a half-written note and a
    crash never truncates one. Appends become read-concat-rewrite so they inherit the same
    all-or-nothing guarantee; imports gain a no-clobber atomic persist. Benefits every writer
    (CLI, stdio MCP, HTTP server) identically.
  - **Filesystem-level path confinement**: `VaultPath` already rejected `..`/absolute paths
    lexically; the store now also canonicalises against the vault root per operation, refusing —
    fail closed, `StoreError::OutsideVault` — any path that escapes through a symlink (or whose
    confinement can't be verified, e.g. dangling symlinks). Legitimate intra-vault symlinks still
    work.
  - **Git checkpoints in `cdno-mcp-server`**: a commit-if-dirty sweep
    (`--git-checkpoint-interval-secs`, default 60, 0 disables) makes every remote mutation
    diffable and revertible — the only meaningful damage limit for prompt-injected writes.
    Deliberately a sweep rather than a per-tool hook: zero handler changes, and out-of-band edits
    join the audit trail too; per-write attribution already lives in the daily-note log lines.
    In-flight atomic-write temp files are excluded from history via `.git/info/exclude` (never
    committed, independent of any lock); the sweep holds the vault write lock so it never
    snapshots a cross-process writer's half-applied transaction; a single transient `git` failure
    (e.g. `.git/index.lock` contention with the operator's own git use) is retried, not fatal.
    Runs `git` with a scrubbed environment; warns and disables when the vault isn't a git repo
    (a `.git` file/worktree pointer is refused too) or `git` is missing.
  - `create_tracking_entry`'s tool description now warns that entries are always dated today
    (no override) so remote callers don't try to backfill past sessions through it.

- **Origin-side Access-JWT validation for `cdno-mcp-server`** (#305) — the server now verifies the
  `Cf-Access-Jwt-Assertion` header Cloudflare Access injects after Managed OAuth: RS256 against
  the team JWKS (fetched fail-closed at startup, refreshed on unknown `kid` behind an
  anti-stampede cooldown), strict `iss`/`aud`/`exp`, every failure a bare 401 with the reason
  logged server-side only. Configured via `CDNO_ACCESS_TEAM_URL` + `CDNO_ACCESS_AUD` (paired
  flags/env; setting one without the other is a startup error). **Configuring auth is exactly
  what lifts the non-loopback bind interlock** — e.g. `0.0.0.0` inside a container becomes legal
  once every request is authenticated at the origin. The middleware sits outermost, so
  unauthenticated requests never consume body-buffer or concurrency budget.

- **`cdno-mcp-server`: Streamable HTTP transport** (#304) — a second binary in the cdno-mcp
  crate serving the same tool catalogue as the stdio `cdno-mcp`, over the MCP Streamable HTTP
  transport (stateless JSON mode, mounted at `/mcp`), for remote deployment behind an
  OAuth-terminating proxy. Deliberately **implements no authentication itself** and therefore
  **refuses to bind non-loopback addresses** until the origin-auth middleware lands (#305) — an
  unauthenticated vault listener must be impossible to expose by accident. Flags: `--bind`
  (default `127.0.0.1:8787`), `--allowed-host` (extends rmcp's DNS-rebinding allowlist),
  `--smoke` (a one-tool echo server holding **no vault handle**, for proving tunnel/auth
  infrastructure with zero vault exposure), `--read-only` (advertises only the context-gathering
  read tools), and `--reconcile-interval-secs` (default 300). Because this server is long-running
  while other writers (CLI, editors, sync) mutate the markdown underneath it, it re-runs the
  index reconciliation pass on that interval as the correctness backstop — the #49 file watcher
  can later reduce latency but never replaces it. `CuadernoServer::read_only()` and the
  `SmokeServer` are exposed from the library for tests and future transports.

- **Tool handlers moved onto the blocking pool** (#307) — every MCP tool call now runs its
  synchronous domain work via `spawn_blocking` (one task per request), so slow disk/SQLite
  operations can no longer stall the HTTP server's async workers; the affordance is shared by
  stdio and HTTP transports.

## [0.3.0] - 2026-07-03

Custom note types: define your own schema-only note types in `config.toml`.

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
