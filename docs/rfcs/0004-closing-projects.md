# RFC 0004 — Closing a project: `complete`, `drop`, and one closure model

| | |
|---|---|
| **Status** | Draft — 2026-09-29 |
| **Tracked by** | #667 (a project can only be parked, never completed); #611 (a parked project's milestones stay in the commitments register) |
| **Affects** | `cdno-core` (`paths.rs`, one index query), `cdno-domain` (project lifecycle, register, reviews, lint), `cdno-cli` (`cdno project`), `cdno-mcp` (two tools, three payloads), `docs/design.md`, `docs-site` |
| **Related** | #559 (`drop_action`), #522 (`drop_milestone`), #573 (`drop_commitment`), #564 (the `reason:` continuation line), #560 (caller-actionable rejections over MCP), #225 (globally unique stems), #215 (last-segment wikilink resolution), #111 (append-only lint for archived actions) |

> **Authorship.** Drafted by Claude (Anthropic) from #667, which the maintainer filed on
> 2026-09-29, and from a survey of every closing verb in the codebase, done for this RFC. The
> problem statement and the expected behaviour in §2 are the maintainer's; the survey in §3, the
> closure model in §5 and the staging in §8 are Claude's. Where the text says "recommended", the
> decision is open and §9 names it.

---

## 1. Summary

A project has one exit today, `park`, and it is the wrong word for two of the three ways a
project ends. This RFC adds the other two:

```bash
cdno project complete --slug surrogate-model              # the work is finished
cdno project drop --slug surrogate-model --reason "…"     # the work is abandoned
```

with `complete_project` and `drop_project` as the MCP tools. Both close the project the way
every other closable thing in the vault is already closed: stamp `status`, date `completed` (or
clear it on a drop), move the note to `projects/_done/<year>/`, swap the index rows, and write
one line to today's daily log — in one transaction. A closed project frees its slot, leaves the
commitments register, the orientation and the stuck-project scan, and shows up in the weekly and
monthly review as a win (completed) or a decision (dropped). `activate` brings a closed project
back, cap-checked, exactly as it brings a parked one back.

Two things make this more than "add a verb":

- **A project is a container.** Its open actions, attached action notes and open milestones are
  what the register, the orientation and the wins list read. Closing the container while leaving
  them open is how a finished project keeps nagging. §5.3 makes closing refuse when open items
  exist, list them, and cascade on explicit request — never silently.
- **Reads must ask the container's status.** The register aggregates milestones and action
  dues without checking whether the owning project is active; that is #611, and it applies to a
  completed project too. §5.5 fixes it once, for parked and closed alike.

### 1.1 How you will use it

You submitted the paper. You run `cdno project complete --slug surrogate-model`. It stops: two
open actions and one open milestone, listed. You tick the milestone (`cdno project milestone
done`), decide the two actions are not happening, and run the same command with `--drop-open`.
The map moves to `projects/_done/2026/surrogate-model.md` with `status: completed` and
`completed: 2026-09-29`; the two actions are logged as dropped with `reason: project completed`,
then the project as completed. `cdno orient` no longer shows it, `cdno commitments` no longer
lists its milestone, `cdno project list` shows four of five slots used, and Sunday's
`get_weekly_context` carries it under `closed_projects` so the review can celebrate it.

A month later the reviewers want a revision. `cdno project activate --slug surrogate-model`
brings it back from `_done/`, cap permitting, and the log says so.

---

## 2. Motivation

From #667. `ProjectStatus` in `crates/cdno-domain/src/frontmatter/project.rs` is
`Active | Parked | Completed`; the parser accepts `status: completed`; `project_summary` and
`cdno project show` render it; a test seeds it by writing the file by hand. But nothing writes
it. The CLI has `create`, `state`, `core-question`, `park`, `activate`, `list`, `show`,
`milestone` and `waiting`; MCP has `park_project` and `activate_project`; `set_frontmatter`
refuses `status` for every type so that lifecycle stays in the verbs. Finishing a project today
means parking it, and that has three costs:

- **The record lies.** The log says `project [[surrogate-model]] parked`, and the weekly and
  monthly reviews read the log back.
- **It keeps coming back.** Parked projects are, by design, revisited at the monthly review.
- **It is the only lifecycle without a done/abandoned split.** Actions, milestones and
  commitments each have a `complete_*` and a `drop_*` verb since #559, #522 and #573. For a
  project, paused, finished and abandoned all collapse into `park`.

`docs-site/src/concepts/rlm.md` promises "permission to park or drop … all first-class, and
none of it is a failure state". For projects only the first half is true.

---

## 3. Background — what closing looks like today

This section is the survey the RFC rests on. It answers the question behind #667: which pieces
of closing machinery exist, which are missing, and whether the gap is specific to projects.

### 3.1 The closure ledger

Every thing in the vault that can end, and how:

| Thing | Finished (work happened) | Ended without the work | Paused | Comes back? | Note moves to | Log line |
|---|---|---|---|---|---|---|
| Action bullet | `complete_action` | `drop_action` (+ reason) | — | no (re-decided, never undone) | bullet removed | `action done on [[p]] — text` / `action dropped on [[p]] — text` |
| Action note | archived by the bullet's completion: `status: completed`, `completed: <date>`, frozen prefix (#111) | archived by the bullet's drop: `status: dropped`, `completed` cleared | `Blocked` exists in the enum; no verb writes it | no | `actions/_done/<year>/` | same line as the bullet |
| Milestone | `complete_milestone` (`- [x] title — date`) | `drop_milestone` (+ reason) | — | no | in place | `milestone done on [[p]] — title` / `milestone dropped on [[p]] — title` |
| Standalone commitment | `complete_commitment`: `status: completed`, `completed: <date>` | `drop_commitment` (+ reason): `status: dropped`, `completed` cleared | `reschedule_commitment` | no | `commitments/_done/<year>/` | `commitment completed [[s]] — title` / `commitment dropped on [[s]] — title` |
| Periodic commitment | `complete_periodic` (advances `next:`) | — (hand-edit the dashboard line) | — | — | in place | `periodic done on [[s]] — title` |
| Waiting-on item | `resolve_waiting_on` | — | — | — | line removed | `waiting resolved on [[p]] — text` |
| Question | `set_question_status(Answered)` | `set_question_status(Retired)` | `Parked` | yes, from any status | in place | `status on [[questions/<domain>/<slug>]]` + `was:` / `now:` |
| Inbox item | triaged elsewhere | `discard_inbox_item` | — | — | file deleted | `triaged inbox item \`<slug>\` -- discarded` |
| **Project** | **none** | **none** | `park_project` | yes, from parked | `projects/_parked/` | `project [[s]] parked` / `project [[s]] activated` |
| Portfolio | — | — | — | — | — | — |
| Stewardship | — (by definition never completes) | — | — | — | — | — |
| Concept (custom) | — (refined in place, never closed) | — | — | — | — | — |

Three note types have no lifecycle by design: a stewardship is "a thing that cannot complete"
(`rlm.md`), a concept is refined rather than closed (RFC 0002 §5.3), and a portfolio accumulates
evidence against a question — the question carries the lifecycle (`answered`, `retired`) and
the portfolio is its record. The RFC leaves those alone.

That leaves one type with a lifecycle and no closure: the project.

### 3.2 The recipe every closing verb follows

`complete_commitment`, `drop_commitment` (`vault/commitments.rs`) and `stage_action_archival`
(`vault/actions.rs`) do the same six things, in the same order, in one transaction:

1. resolve the note and **refuse unless its `status` is `active`** — the folder is a hint, the
   frontmatter is the truth;
2. stamp `status: completed` or `status: dropped`;
3. write `completed: <today>` on a completion, **clear it** on a drop, so an archived note can
   never read "dropped" while carrying a date that says it was kept;
4. move the file to `<type>/_done/<year>/<slug>.md`, refusing if the destination is taken;
5. swap the index rows (`upsert_note` new path, `remove_note` old path);
6. write one daily-log line — `<type> completed [[…]] — <title>` or `<type> dropped on [[…]] —
   <title>`, the latter with an optional indented `reason:` continuation (#564).

Actions carry the outcome as a named type, `ActionClosure { Completed, Dropped }`, whose doc
comment says why: "the two outcomes are not opposites of one degree — they are different claims
about what happened". Commitments inline the same distinction in two functions. There is no
shared type, but there is a shared recipe, and it is the one this RFC extends to projects.

### 3.3 The gaps

**G1 — No closure verb for a project.** The enum has `Completed` and no `Dropped`; nothing
writes either. (`project.rs`; the CLI and MCP surfaces listed in §2.)

**G2 — Nowhere for a closed project to go, and "where is project X" is answered in six
places.** `paths.rs` has `PROJECTS` and `PROJECTS_PARKED` and no `_done/`. Every project locator
probes those two paths by hand: `resolve_active_project` and `resolve_any_project`
(`projects/mod.rs`), `get_project_full` (`context.rs`), `resolve_project_path` (`commitments.rs`,
for an action note's owning map), `available_projects_hint`, `activate_project`, and in core
`project_path_candidates` behind `milestones_for_project`. A third location cannot be added by
touching one of them.

**G3 — Closing or parking a container ignores its children.** `park_project` rewrites one
field and moves one file. Its open bullets stay open; its attached action notes stay `status:
active` under `actions/` and keep appearing in every `list_by_type(action)` scan; its milestones
stay open in the `milestones` table. And because every closing verb goes through
`resolve_active_project`, none of them can be closed while the project is parked — the only
route is `activate`, which the cap can refuse (#611, first half).

**G4 — The register reads children without asking the parent's status.** `commitments()`
source 1 reads `milestones_between` and resolves each project map only for its `context`
(`cached_project_context` discards the parsed status); source 4 resolves an action note's project
through `resolve_project_path`, which returns parked maps happily. A parked project's dates keep
surfacing in orientation and the lookahead (#611, second half). A completed project's would too.

**G5 — Reviews never see a project end.** `get_weekly_context` composes the week's logs,
`completed_actions_between` and `project_state_changes_between`; `get_monthly_context` composes
completed actions, questions, portfolios, stuck projects, stewardships, the lookahead and slot
allocation. There is no project-closure source. Today a finished project's only trace is a
`parked` line and its last `state on` entry.

**G6 — The project log lines are the odd ones out.** `project [[s]] parked` and `project [[s]]
activated` put the subject first; every closing line elsewhere reads `<type> completed [[…]] —
<title>` or `<type> dropped on [[…]] — <title>` with an optional `reason:`. A reader grepping
the log for what ended should find projects in the same shape.

**G7 — No completion date on a project.** `ProjectFrontmatter` is `context, status, created,
core_question`. Commitments and actions carry `completed:`, and `completed_actions_between`
selects wins by it. `rewrite_field_in_frontmatter` deliberately refuses to *insert* an absent key
(`MissingFrontmatterField`), so adding one to maps that already exist needs a helper that can
insert as well as rewrite — the same trap `drop_action` hit with `completed` on an ejected
template, handled there by a fallback.

**G8 — Lint does not check status against folder.** A `status: completed` map under
`projects/` (the test fixture's shape) or a `status: active` map under `_parked/` passes lint. The
resolvers already trust frontmatter over folder; lint should say when the two disagree.

**Adjacent, not closure — out of scope, recorded so nobody rediscovers them:**

- `ActionStatus::Blocked` has no verb either. Same shape as G1 (an enum state no verb writes),
  a different concern (pausing, not ending). Should be its own issue.
- A waiting-on item can be resolved but not dropped. The register does not read `## Waiting On`,
  so a moot item costs nothing; a `drop` would only be for the record.
- A periodic commitment can be completed but not retired. The stewardship dashboard is mutable
  by design; deleting the line is the supported edit.

The answer to "is this only about projects", then: the *verbs* are missing only for projects, but
the *container* problems (G3, G4) are project problems that show up whenever a project changes
state — parking today, closing tomorrow — and the fix for them is the same fix.

---

## 4. Terminology

- **Closure** — ending a thing's active life. Two **outcomes**: `completed` (the work happened)
  and `dropped` (it did not). Parking is not a closure: it is reversible by design and asserts
  nothing about the work.
- **Container** — a project. **Children** — its open next-action bullets, the action notes those
  bullets link, and its open milestones. Waiting-on lines are not children for closure purposes
  (§9, D4).
- **Open item** — a child that is still open at closure time.
- **The `_done/` tree** — `<type>/_done/<year>/`, year of closure, for actions and commitments
  today and projects after this RFC.

---

## 5. Proposal

### 5.1 Two verbs

`Vault::complete_project(at, slug, open_items)` and `Vault::drop_project(at, slug, reason,
open_items)`, surfaced as `cdno project complete` / `cdno project drop` and `complete_project` /
`drop_project`. Both require the project to be active — a parked project is activated first, as
for every other mutating verb, so the one rule "mutating verbs need an active project" stays one
rule (§9, Q1 discusses the exception #611 floated).

### 5.2 One closure model

Promote `ActionClosure` to a vault-wide `Closure { Completed, Dropped }` (`vault/closure.rs`) and
write the six steps of §3.2 for projects against it. The type carries what the outcome stamps:
`status()` and whether `completed` is dated or cleared. Commitments are refactored onto it only if
that stays a mechanical change; the RFC does not require it (§8, S2).

The project's frontmatter grows `completed: <date> | null`, exactly as action and commitment
frontmatter has it, and `ProjectStatus` grows `Dropped`. Both are additive (§7).

### 5.3 Children: refuse, list, cascade on request

Closing a project with open items is refused with `DomainError::ProjectHasOpenItems { slug,
actions, milestones }`, where `actions` carries each open bullet's text and, when attached, the
note slug and status, and `milestones` each open milestone's title and date. The error is data,
in the `ProjectCapReached` and `AmbiguousAction` tradition: the CLI renders it as a list, the MCP
returns it as a caller-actionable rejection (#560).

With `open_items = OpenItems::Drop` the same call closes the children first, in the same
transaction, through the existing drop logic: each open bullet is removed as `drop_action` would
remove it, each attached note archived as `Closure::Dropped` (status `dropped`, `completed`
cleared, snapshot recorded), each open milestone removed as `drop_milestone` would remove it —
each with its own log line and `reason: project completed` (or `project dropped`). Then the
project's own line. Children first so the daily log reads in the order things happened.

Children are dropped, never completed, by the cascade. A completion is a claim about work, and
the cascade cannot know the work happened; if it did, the user ticks it first, which is what the
refusal is for. `OpenItems::Refuse` is the default on every surface.

### 5.4 Where a closed project lives

`projects/_done/<year>/<slug>.md`, year of closure, for both outcomes — mirroring
`commitments/_done/<year>/` and `actions/_done/<year>/`, where dropped notes also live. The
stem is globally unique (#225), so `[[surrogate-model]]` keeps resolving after the move (#215).

A closed map is not frozen. `resolve_active_project` already refuses every mutating verb on it,
as it does for a parked map; a late retrospective appended by hand, or a reactivation, are both
legitimate. The archived-action freeze (#111) stays what it is.

### 5.5 Reads ask the container's status

One resolver, `locate_project(slug) -> ProjectLocation { path, frontmatter }`, answers "where is
project X and what state is it in" from the index (`list_by_type(project)` filtered by stem)
rather than by probing paths, and every locator in G2 is rewritten on top of it. In core,
`milestones_for_project` takes the resolved path instead of guessing two candidates.

The register then filters by status once: source 1 skips a milestone whose project is not
active, source 4 skips an action note whose project is not active. That is #611's second
candidate, and it covers parked and closed projects with the same line. `orientation_context`
and the stuck scans already filter on `Active` and need nothing.

### 5.6 Reviews see closures

`Vault::closed_projects_between(from, to) -> Vec<ClosedProjectEntry { slug, title, context,
outcome, closed_on }>`, read from the `_done/` maps' `status` and `completed:` the way
`completed_actions_between` reads archived action notes — with the difference that a dropped
project has no `completed:` date, so its `closed_on` comes from the daily-log line. Recommended
simplification: stamp `closed: <date>` on both outcomes and keep `completed:` for the
completion only (§9, D7). `get_weekly_context` and `get_monthly_context` gain a
`closed_projects` array; the review skills can then put a completed project in the wins and a
dropped one in the decisions without parsing prose.

### 5.7 Reactivation

`activate_project` accepts a project from `_parked/` or `_done/<year>/`. Same cap check, same
collision guard, same `status: active`; `completed` (and `closed`, if adopted) are cleared, so a
reactivated project can be closed again with a fresh date. The log line stays `project [[s]]
activated`; the `was:` folder is not worth a continuation line, since the previous closure line
is in the log already.

### 5.8 Parking parks the dates

§5.5 does this without a new verb: once the register filters on the container's status, a parked
project's milestones and action dues stop surfacing, which was the second half of #611. The
first half — letting `drop_milestone` act on a parked project — becomes unnecessary and is
recommended against (§9, Q1).

---

## 6. Detailed design

### 6.1 `cdno-core`

- `paths.rs`: `PROJECTS_DONE = "projects/_done"`, `projects_done_dir(year)`, and the current
  year's folder in `init_dirs`. The doc comment on `PROJECTS` gains the third location.
- `index.rs`: `milestones_for_project(path: &VaultPath)` replaces the slug-and-guess signature;
  `project_path_candidates` is deleted. `milestones_between` is unchanged — the status filter
  belongs to the domain, which has the frontmatter. No migration: the schema does not change,
  and reconciliation types a note by its frontmatter `type:`, so `projects/_done/2026/x.md`
  indexes as a project with no change.

### 6.2 `cdno-domain`

**Types.** `ProjectStatus::Dropped` (kebab `dropped`); `ProjectFrontmatter.completed:
Option<NaiveDate>` (optional field, absent reads as `None`, so every existing map parses);
`Closure { Completed, Dropped }` in `vault/closure.rs`, with `ActionClosure` becoming a type
alias or being replaced outright; `OpenItems { Refuse, Drop }`; `ProjectLocation`;
`ClosedProjectEntry`; `DomainError::ProjectHasOpenItems`; `DomainError::ProjectNotClosed` is
not needed — `activate` reports `ProjectNotParked` today and the message widens to "not parked or
closed".

**Frontmatter helper.** `upsert_field_in_frontmatter(raw, field, value)`: rewrite when the
top-level key exists, otherwise insert after the last top-level key. Used only for `completed`
(and `closed`), so a map created before this RFC gains the key on closure and never fails the
way `drop_action` once could.

**Template.** `templates/project.md` gains `completed: null` after `created:`. The
`frontmatter_order` pin for `project` updates with it; the normaliser derives order from the
effective template, so custom `.cuaderno/templates/project.md` files without the key stay valid
and get the key inserted on first closure.

**`locate_project`** (`projects/mod.rs`): index scan by type and stem; a stem present at more
than one location is `Store(AlreadyExists)` naming both, as the create/park guards do today. The
rewrites: `resolve_active_project` (status must be `Active`), `resolve_any_project`,
`get_project_full`, `resolve_project_path` in `commitments.rs`, `available_projects_hint`
(displays `(parked)` and `(completed)`/`(dropped)`), `activate_project`, and `project_summary`
through `resolve_any_project`.

**`complete_project` / `drop_project`** (`projects/lifecycle.rs`), one transaction each:

1. `resolve_active_project(slug)`;
2. `open_items_report(&doc, slug)`: open bullets from `## Next Actions` (via the existing
   parser), each attached note's status via `list_actions`, open milestones from the index; if
   non-empty and `OpenItems::Refuse`, return `ProjectHasOpenItems`;
3. with `OpenItems::Drop`: for each open bullet, the bullet removal and (when attached)
   `stage_action_archival(Closure::Dropped)`, with `format_action_dropped_log_entry(slug, text,
   Some("project completed"))`; for each open milestone, the removal with continuation lines
   and `format_milestone_dropped_log_entry`, then `stage_milestone_index_rows`. The section
   edits are made on the one `MarkdownDocument` and rendered once;
4. `status` stamped `completed` / `dropped`; `completed` upserted to today / `null`;
5. destination `projects/_done/<year>/<slug>.md`, `AlreadyExists` if occupied;
6. `write_file` new, `delete_file` old, `upsert_note`, `remove_note`;
7. log lines, children first, via `stage_daily_logs`: `project completed [[<slug>]] — <title>`
   or `project dropped on [[<slug>]] — <title>` with the `reason:` continuation when given.
   `<title>` is the body H1, as `body_title_or_slug` gives it. Both link the bare slug, so
   `mentions_project` and `daily_log_mentions` find them.

Returns `WriteOutcome` (primary: the new map path; `paths`: everything the commit touched, so an
MCP verification can read the destination) plus the list of children it dropped, in a
`ProjectClosureOutcome`.

**`activate_project`**: locate; accept `Parked` or `Completed`/`Dropped`; clear `completed`;
otherwise as today.

**Queries.** `parked_projects()` unchanged; `closed_projects()` added on the same pattern for
the CLI picker and `list_projects`; `closed_projects_between(from, to)` for the reviews;
`commitments()` gains the two status checks of §5.5, with `cached_project_context` widened to
cache `(Context, ProjectStatus)`.

**Lint** (`vault/lint.rs`): a project whose `status` disagrees with its folder is an `Error`
("downstream code can trip over it": the locator trusts the frontmatter, so the file is
unreachable by the verb its folder suggests). Rows: `active` outside `projects/`; `parked`
outside `projects/_parked/`; `completed` or `dropped` outside `projects/_done/<year>/`;
`completed` without a `completed:` date; `dropped` with one.

### 6.3 `cdno-cli` (flags-and-prompts, `docs/cli-ergonomics.md`)

```
cdno project complete [--slug S] [--drop-open] [--json]
cdno project drop     [--slug S] [--reason R] [--drop-open] [--json]
```

- `--slug` gathers through `gather_or_error` with the active-project picker (`prompt_project`),
  as `park` does. `--reason` is optional and never prompted for on `drop`, matching `cdno action
  drop` and `cdno commit drop`.
- On `ProjectHasOpenItems` without `--drop-open`: interactive — print the list and ask "Drop
  these N open items and complete the project?", retry with `OpenItems::Drop` on yes; otherwise
  — print the list, exit non-zero, and under `--json` emit `{"error": "open_items", "actions":
  […], "milestones": […]}` so a script can decide.
- Confirm only when something was prompted, per the convention.
- `cdno project activate` picker and `complete_parked_project` completion widen to parked and
  closed projects, labelled. `cdno project list` gains `--closed` (closed projects, newest first,
  with outcome and date); the default stays active-only. `cdno project show` already renders the
  status badge and needs only the `Dropped` arm.

### 6.4 `cdno-mcp`

- `complete_project { project, open_items?: "refuse" | "drop" }` and `drop_project { project,
  reason?, open_items?: … }` in `lifecycle.rs`, `verified_write` with `WriteShape::Rewritten` on
  the destination path. Default `refuse`. The refusal returns as a tool result carrying
  `{ "rejection": "open_items", "actions": […], "milestones": […] }` in the #560 shape, so an
  agent can show the list and ask before calling again with `"drop"`. The descriptions carry the
  rules, since they are the only instruction surface: that closing is refused while items are
  open, that `drop` cascades as drops with a recorded reason, that a completion is a claim about
  work and a drop is not.
- `list_projects` gains `closed: [ … ]` beside `active` and `parked`; entries already carry the
  full frontmatter, so the outcome is `frontmatter.status`.
- `get_weekly_context` and `get_monthly_context` gain `closed_projects`.
- `get_project_context` resolves through `locate_project`, so a closed project is readable.
- The tool count pin in `crates/cdno-mcp/tests/server.rs` goes to 60.

### 6.5 Documentation

`docs/design.md` §5.3 (status values, `completed:`, the `_done/` folder, the lifecycle
paragraph), the structural-conventions list, the note-type table's `project` row, and the CLI
and MCP surface tables. `docs-site`: `reference/cli/project.md` (two subcommands, `list
--closed`, `activate` from `_done/`), `reference/mcp/creation-and-lifecycle.md`,
`concepts/business-rules.md` (the cap paragraph), `concepts/vault-structure.md`,
`concepts/rlm.md` ("projects park" becomes "projects park, complete or drop"), and the projects
tutorial. `CHANGELOG.md` and `STATUS.md` with the shipping PR.

---

## 7. Compatibility

- **Existing vaults need no migration.** `completed` is optional and absent on every existing
  map; the key is inserted on first closure. `projects/_done/` is created on first use
  (`create_dir_all`) and by `cdno init` for new vaults. `cdno reindex` picks up `_done/` maps by
  their frontmatter type.
- **Existing behaviour is unchanged** for `park`, `activate` from `_parked/`, `create`, and
  every project-body verb. A project that is `Active` behaves exactly as before.
- **A hand-made `status: completed` map under `projects/`** (the shape
  `project_summary_returns_summary_for_completed_project` seeds) keeps parsing and rendering;
  `locate_project` finds it and reports it as not active; lint now flags the folder mismatch.
- **Wikilinks keep resolving** across the move by the last-segment rule (#215), as they do for
  parked projects and archived actions.
- **Log-line family.** Two new fixed prefixes, `project completed ` and `project dropped on `,
  join the family in CLAUDE.md's "history preservation" list; the `parked`/`activated` lines are
  left as written — a change would break nothing but would also buy nothing.
- **MCP clients** see two new tools and three widened payloads; nothing is removed or renamed.

---

## 8. Implementation plan — staged

One PR per stage; each leaves `just ci` green and ships on its own.

- **S0 — Location.** `PROJECTS_DONE` and `projects_done_dir` in core; `locate_project` in the
  domain and the G2 rewrites onto it; `milestones_for_project` by path. No behaviour change;
  the tests that pin the two-path probes move to the locator.
- **S1 — The register asks the parent.** Status filter in `commitments()` sources 1 and 4.
  Closes #611 (second candidate). Independent of S2 and worth shipping first.
- **S2 — The verbs.** `Closure`, `ProjectStatus::Dropped`, `completed` on the frontmatter and
  template, `upsert_field_in_frontmatter`, `complete_project`, `drop_project`, the open-items
  report and cascade, `activate` from `_done/`, `closed_projects()`. Domain tests in
  `tests/unit/projects_tests.rs`.
- **S3 — CLI.** The two subcommands, `list --closed`, the widened `activate` picker and
  completions. `assert_cmd` wiring tests only.
- **S4 — MCP.** The two tools, the rejection shape, `list_projects.closed`,
  `get_project_context` on a closed project. Handler tests plus the `e2e_*` catalogue pin.
- **S5 — Reviews.** `closed_projects_between` and the two context payloads.
- **S6 — Lint and docs.** The status/folder rows; `docs/design.md`; `docs-site`; CHANGELOG and
  STATUS. Closes #667.

S0 and S1 are safe to merge before the RFC is accepted: they change no user-visible behaviour
except fixing #611.

---

## 9. Decisions and open questions

**D1 — Both outcomes archive to `_done/<year>/`.** Not `_dropped/`. Commitments and actions
already put dropped notes under `_done/`; the outcome is in the frontmatter and the log, not the
folder name. One tree to reconcile, one tree to `activate` from.

**D2 — Refuse by default, cascade on request.** The alternative — always cascade and report —
would make an MCP call with a typo in the slug irreversible before anyone read the result. The
refusal costs one round trip and makes the drop an explicit decision. Interactive CLI folds the
round trip into a confirm.

**D3 — The cascade drops, never completes.** A completion is a claim about work; the tool
cannot make it on the user's behalf. Open items that were in fact done are ticked first, which
the listing prompts for.

**D4 — Waiting-on lines are not children.** Nothing downstream reads them; they stay in the
closed map as part of its record. Adding a `drop_waiting_on` is a separate, small issue if wanted.

**D5 — A closed map is not frozen.** Reactivation and late retrospectives are both real; the
mutating verbs already refuse a non-active map. The archived-action freeze is about a note that
is genuinely finished; a project map is a dashboard, and a dashboard that may come back is not.

**D6 — Log-line shapes** are `project completed [[<slug>]] — <title>` and `project dropped on
[[<slug>]] — <title>` (+ `reason:`), matching commitments. The bare slug is linked so the
existing project-mention matcher finds them.

**D7 — Recommended: a `closed:` date on both outcomes.** `completed:` alone leaves a dropped
project without a machine-readable date in its frontmatter, so `closed_projects_between` would
have to read the log for half its rows. Stamping `closed: <date>` on both outcomes and
`completed: <date>` on completions only keeps the "dropped never carries a completion date"
invariant and gives the review query one field to read. Cost: one more frontmatter key on
projects that actions and commitments do not have. The alternative is to accept the log read.
**Open — the maintainer decides.**

**Q1 — Should `drop_action` / `drop_milestone` accept a parked project?** (#611, first
candidate.) Recommended **no**: once S1 lands, a parked project's dates stop nagging, so the
motivating case is gone, and keeping "mutating verbs need an active project" as one rule is
worth more than the shortcut. A project that needs its plan pruned while parked is activated,
pruned and parked again — three commands, all of them a record.

**Q2 — Carrying an open action forward to another project.** #667 mentions "explicitly carried
forward". There is no move-action verb, and `rlm.md` settles that a drop is re-decided rather than
undone: the action is re-added on the project that now owns it. Recommended: not in this RFC;
open an issue if the cascade's `reason:` line proves insufficient as the pointer.

**Q3 — `ActionStatus::Blocked`.** Same class of gap as G1, different concern. Recommended: a
separate issue, not this RFC.

**Q4 — Closing a project that is already closed.** Refused as `ProjectNotActive`, as for parked.
A dropped project that turns out to have been finished is activated and completed; the log then
shows both decisions, which is the point.

---

## 10. Verification

Per crate, the tests each stage adds; the names follow the suite's style.

- `cdno-core`: `projects_done_dir_is_year_partitioned`; `init_dirs_includes_projects_done_for_the_current_year`; `milestones_for_project_reads_the_given_path`.
- `cdno-domain` (`projects_tests.rs`): `locate_project_finds_active_parked_and_closed`;
  `locate_project_refuses_a_stem_at_two_locations`; `complete_project_moves_stamps_and_logs`;
  `complete_project_refuses_with_open_items_listed`; `complete_project_drop_open_cascades_as_drops_with_reason`;
  `complete_project_cascade_archives_attached_notes_as_dropped`; `drop_project_clears_completed_and_logs_reason`;
  `complete_project_refuses_a_parked_project`; `complete_project_refuses_an_occupied_destination`;
  `activate_project_from_done_clears_completed_and_cap_checks`; `closed_projects_between_returns_both_outcomes`;
  `active_projects_excludes_closed`. (`commitments_tests.rs`): `commitments_skips_milestones_of_parked_projects`;
  `commitments_skips_milestones_of_closed_projects`; `commitments_skips_action_dues_of_non_active_projects`.
  (`lint_tests.rs`): `lint_errors_on_project_status_folder_mismatch`; `lint_errors_on_dropped_project_with_completed_date`.
- `cdno-cli` (`project.rs` target): `complete_non_interactive_lists_open_items_and_fails`;
  `complete_drop_open_succeeds_and_prints_destination`; `drop_with_reason_writes_reason_line`;
  `list_closed_shows_outcome_and_date`; `activate_from_done_works`.
- `cdno-mcp`: `complete_project_rejection_carries_open_items`; `drop_project_with_open_items_drop_cascades`;
  `list_projects_includes_closed`; `get_weekly_context_includes_closed_projects`; the tool-catalogue pin at 60.
