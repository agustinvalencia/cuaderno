# RFC 0004 — Closing a project: `complete`, `drop`, and one closure model

| | |
|---|---|
| **Status** | Draft, amended after review — 2026-09-29 (three-seat review, three rounds, on the RFC PR; all seats approve for amendment; §9 D7 is the maintainer's to confirm). Implementation shipped: #692 (T0), #693 (T1), #694 (T2), #695 (T3), #697 (T4), #696 (T5), #698 (T6), #700 (T7), #701 (T8), #702 (T9), #703 (T10), #704 (T11), #705 (T12), #706 (T13), #707 (T14), #708 (T15), #709 (T16), #710 (T17), #711 (T18), #712 (T19), #713 (T20), #714 (T21). Follow-up shipped: #715 (`cdno templates sync`, #699). The status moves to Accepted once D7 is confirmed. |
| **Tracked by** | #667 (a project can only be parked, never completed); #611 (a parked project's milestones and the commitments register) |
| **Affects** | `cdno-core` (`paths.rs`, one index query), `cdno-domain` (project lifecycle, register, reviews, lint), `cdno-cli` (`cdno project`), `cdno-mcp` (two tools, one rejection code, four payloads), `docs/design.md`, `docs-site` |
| **Related** | #559 (`drop_action`), #522 (`drop_milestone`), #573 (`drop_commitment`), #564 (the `reason:` continuation line), #560 (caller-actionable rejections over MCP), #225 (globally unique stems), #215 (last-segment wikilink resolution), #111 (append-only lint for archived actions), #481 (record sequences in frontmatter) |

> **Amended after review.** The first draft was reviewed on the PR by three seats (CLI and MCP
> surface; method and user; safety, tests and maintenance), three rounds, seats replying to each
> other until they agreed. The review changed §3.3 G4 (a bug reproduced: moving a project map
> deletes its milestone rows from the index), §5.1 (closing accepts a parked project and never
> cap-checks), §5.2 (one `closed:` date, no `completed:`), §5.3 (the report parses the map; hard
> milestones and linked commitments are named; a hash guards the MCP cascade), §5.5 (the locator
> reads the disk, not the index; every move restages index rows), §5.6 (`parked_projects` in the
> monthly context), §5.7 (a direct switch between outcomes), §6 throughout, and §9. The text
> below is the amended version; the record is on the PR.

> **Amended by the maintainer, 2026-09-30** (during implementation, #701). Two decisions, §9
> D11 and D12: **`complete` never cascades.** A project with an open action or milestone
> refuses to be completed, with or without any flag; each item is completed or dropped first.
> Only `drop` offers to let open items go with the project, still refusing by default. And the
> cascade drops **every attached action note that is not already closed**, `blocked` included,
> rather than only `active` ones. §1.1, §5.1, §5.3, §6.2 to §6.4 and §10 are amended in place.

> **Authorship.** Drafted by Claude (Anthropic) from #667, which the maintainer filed on
> 2026-09-29, and from a survey of every closing verb in the codebase, done for this RFC. The
> problem statement and the expected behaviour in §2 are the maintainer's; the survey in §3, the
> closure model in §5 and the staging in §8 are Claude's, amended by the review. Where the text
> says "the maintainer confirms", the decision is open and §9 names it.

---

## 1. Summary

A project has one exit today, `park`, and it is the wrong word for two of the three ways a
project ends. This RFC adds the other two:

```bash
cdno project complete --slug surrogate-model              # the work is finished
cdno project drop --slug surrogate-model --reason "…"     # the work is abandoned
```

with `complete_project` and `drop_project` as the MCP tools. Both close the project the way
every other closable thing in the vault is already closed: stamp `status`, stamp `closed:`, move
the note to `projects/_done/<year>/`, swap the index rows, and write one line to today's daily
log — in one transaction. A closed project frees its slot, leaves the commitments register, the
orientation and the stuck-project scan, and shows up in the weekly and monthly review as a win
(completed) or a decision (dropped). `activate` brings a closed project back, cap-checked, exactly
as it brings a parked one back. Closing accepts an active *or a parked* project and never checks
the cap, because closing never adds an active project.

Three things make this more than "add a verb":

- **A project is a container.** Its open actions, attached action notes and open milestones are
  what the register, the orientation and the wins list read. Closing the container while leaving
  them open is how a finished project keeps nagging. §5.3 makes closing refuse when open items
  exist and list them; a drop may then let them go on explicit request, never silently, and a
  completion never does.
- **Moving a project map loses its dates today.** Parking or activating a project deletes its
  milestone rows from the index and never writes them back, so a hard deadline vanishes from
  `cdno commitments` until the next `cdno reindex`. The review reproduced this with the real
  binary. §5.5 fixes every move, closing included, and is the actual fix for #611.
- **Reads must ask the container's status.** The register aggregates milestones and action
  dues without checking whether the owning project is active. §5.5 fixes it once, for parked and
  closed alike.

### 1.1 How you will use it

You submitted the paper. You run `cdno project complete --slug surrogate-model`. It stops:

```
surrogate-model has 3 open items:
  - [ ] Run feature set B on full geometry mesh (deep)
  - [ ] [[actions/characterise-kan-ppo-sample-efficiency]] (deep)
  - [ ] ICML paper submitted — hard: 2026-05-22
Still open, not touched: [[commitments/reviewer-report]] (due 2026-10-15)
Complete or drop each of them (or add it to the project that now owns it), then run again.
```

You tick the milestone (`cdno project milestone done`), decide the two actions are not
happening and drop them (`cdno action drop`, each with its own reason), and run the same
command again. Nothing is open now, so the map moves to
`projects/_done/2026/surrogate-model.md` with `status: completed` and `closed: 2026-09-29`, and
the project is logged as completed:

```
Completed surrogate-model. 4 of 5 slots in use.
```

A completion is a claim that the work is done, so it never lets open work go on its own (§9
D11); `drop` is the verb that may.

`cdno orient` no longer shows it, `cdno commitments` no longer lists its milestone, and Sunday's
`get_weekly_context` carries it under `closed_projects` so the review can celebrate it.

A month later the reviewers want a revision. `cdno project activate --slug surrogate-model`
brings it back from `_done/`, cap permitting, and the log says so.

At the monthly review, with five projects active, you decide a parked project is not coming back.
`cdno project drop --slug bayesian-opt-survey --reason "superseded by the ICML work"` closes it
from `_parked/` directly: no slot is needed, and no `activated` line is written for a resumption
that never happened. If it still has open items, the drop lists them and asks
`Let these N go and drop bayesian-opt-survey? [y/N]` (or takes `--drop-open`); each is logged as
dropped with `reason: project dropped (superseded by the ICML work)`.

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
- **It keeps coming back.** Parked projects are, by design, revisited at the monthly review
  (`docs/design.md`, structural conventions) — though the review found that `get_monthly_context`
  carries no parked list today, so the design intent has no surface (§5.6).
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
| Action note | archived by the bullet's completion: `status: completed`, `completed: <date>` | archived by the bullet's drop: `status: dropped`, `completed` cleared | `Blocked` exists in the enum; no verb writes it | no | `actions/_done/<year>/`, prefix frozen for either outcome (#111) | same line as the bullet |
| Milestone | `complete_milestone` (`- [x] title — date`) | `drop_milestone` (+ reason) | — | no | in place | `milestone done on [[p]] — title` / `milestone dropped on [[p]] — title` |
| Standalone commitment | `complete_commitment`: `status: completed`, `completed: <date>` | `drop_commitment` (+ reason): `status: dropped`, `completed` cleared | — (`reschedule_commitment` moves a date; it is not a pause) | no | `commitments/_done/<year>/` | `commitment completed [[s]] — title` / `commitment dropped on [[s]] — title` |
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
   frontmatter is the truth (the commitment verbs check this; `stage_action_archival` checks only
   that the note exists, since its caller has already resolved the bullet);
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

**G2 — Nowhere for a closed project to go, and "where is project X" is answered in eight
places.** `paths.rs` has `PROJECTS` and `PROJECTS_PARKED` and no `_done/`. Every project locator
probes those two paths by hand: `resolve_active_project` and `resolve_any_project`
(`projects/mod.rs`), `update_project_state` (`projects/state.rs`), `get_project_full`
(`context.rs`), `resolve_project_path` (`commitments.rs`, for an action note's owning map),
`available_projects_hint`, `activate_project`, the active-beats-parked rule in
`note_ref::narrow` (`note_ref.rs`), and in core `project_path_candidates` behind
`milestones_for_project`. A third location cannot be added by touching one of them.

**G3 — Closing or parking a container ignores its children.** `park_project` rewrites one
field and moves one file. Its open bullets stay open; its attached action notes stay `status:
active` under `actions/` and keep appearing in every `list_by_type(action)` scan. And because
every closing verb goes through `resolve_active_project`, none of them can be closed while the
project is parked — the only route is `activate`, which the cap can refuse (#611, first half).

**G4 — Moving a project map deletes its dates, and the register never asks the parent's
status.** Two defects that #611 saw as one. First, the `milestones` and `deadlines` tables
reference `notes(path)` with `ON DELETE CASCADE` (`migrations/001_initial.sql`), so the
`remove_note(old)` in `park_project` and `activate_project` (`lifecycle.rs`) deletes the rows,
and neither verb restages them at the destination. The commit records the file's real mtime, so
reconciliation's fast path (`reconcile.rs`, #94) classifies the moved file as unchanged and never
heals it; only `cdno reindex` does. Reproduced with the binary on a temp vault: add a hard
milestone, `park`, `activate` — it is gone from `cdno commitments` until a reindex. So a parked
project's dates do not nag today; they silently vanish, and stay vanished after reactivation.
Second, and dormant behind the first: `commitments()` source 1 resolves each project map only
for its `context` (`cached_project_context` discards the parsed status), and source 4 resolves an
action note's project through `resolve_project_path`, which returns parked maps happily. Fix the
first defect alone and a parked project's dates *would* nag; the two fixes ship together (§5.5).

**G5 — Reviews never see a project end.** `get_weekly_context` composes the week's logs,
`completed_actions_between` and `project_state_changes_between`; `get_monthly_context` composes
completed actions, questions, portfolios, stuck projects, stewardships, the lookahead and slot
allocation — and no parked list, despite the design's "revisited at the monthly review". There
is no project-closure source. Today a finished project's only trace is a `parked` line and its
last `state on` entry.

**G6 — The project log lines are the odd ones out.** `project [[s]] parked` and `project [[s]]
activated` put the subject first; every closing line elsewhere reads `<type> completed [[…]] —
<title>` or `<type> dropped on [[…]] — <title>` with an optional `reason:`. A reader grepping
the log for what ended should find projects in the same shape.

**G7 — No closure date on a project.** `ProjectFrontmatter` is `context, status, created,
core_question`. Commitments and actions carry `completed:`, and `completed_actions_between`
selects wins by it. `rewrite_field_in_frontmatter` deliberately refuses to *insert* an absent key
(`MissingFrontmatterField`), so adding one to maps that already exist needs a helper that can
insert as well as rewrite. One already exists: `merge_fields_into_frontmatter`
(`vault/frontmatter_edit.rs`) replaces in place, appends before the closing `---`, skips
continuation lines (#481) and handles CRLF.

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
  (§9, D4). Standalone commitments whose `project:` names the project are **linked**, not
  children: a promise to someone else outlives the project (§5.3).
- **Open item** — a child that is still open at closure time.
- **A move** — any verb that relocates a project map: `park`, `activate`, `complete`, `drop`.
- **The `_done/` tree** — `<type>/_done/<year>/`, year of closure, for actions and commitments
  today and projects after this RFC.

---

## 5. Proposal

### 5.1 Two verbs, from active or parked

`Vault::complete_project(at, slug)` and `Vault::drop_project(at, slug, reason, open_items)`,
surfaced as `cdno project complete` / `cdno project drop` and `complete_project` /
`drop_project`.

Both accept a project whose status is `active` **or `parked`**, and neither checks the cap:
closing never adds an active project, so the cap has nothing to guard, and the write lock
serialises a close against a concurrent `activate`. The commonest drop is a parked project let
go at the monthly review; on a full vault, routing it through `activate` would mean park A,
activate X, drop X, activate A — four commands and two `activated` lines recording a resumption
that never happened, the "record lies" problem this RFC exists to fix.

The rule becomes: **verbs that change a project's plan need it active; verbs that end it accept
active or parked.** `resolve_active_project` stays the gate for every body verb (§9, Q1).

### 5.2 One closure model

Promote `ActionClosure` to a vault-wide `Closure { Completed, Dropped }` (`vault/closure.rs`) and
write the six steps of §3.2 for projects against it. The type carries what the outcome stamps.
Commitments are refactored onto it only if that stays a mechanical change; the RFC does not
require it (§8, S2).

The project's frontmatter grows **one** date, `closed: <date> | null`, stamped on both outcomes;
`status` carries which outcome it was. A project does not get `completed:`: on a completion the
two dates would always be equal, so the second is redundant state that `activate` would have to
clear and lint would have to police, for no reader (§9, D7; the maintainer may still rule for
both). `ProjectStatus` grows `Dropped`. Both are additive (§7).

### 5.3 Children: refuse, list, cascade on request

Closing a project with open items is refused with `DomainError::ProjectHasOpenItems { slug,
actions, milestones, untouched_commitments, open_items_hash }`, where `actions` carries each open
bullet's text and, when attached, the note slug and status; `milestones` each open milestone's
title, date and whether it is hard (hard milestones are the only ones the register reads, so the
user should see which drops would retire a deadline); and `untouched_commitments` the active
standalone commitments whose `project:` names the project — **for information only, never
cascaded**, because a promise made to someone else does not end with the project. The error is
data, in the `ProjectCapReached` and `AmbiguousAction` tradition: the CLI renders it as a list
(§6.3), the MCP returns it as a caller-actionable rejection (§6.4).

**The report parses the map itself** — `## Next Actions` and `## Milestones` from the
`MarkdownDocument`, via the existing bullet parser and `extract_milestones_from_body`
(`cdno-core/src/markdown.rs`) — and never `list_actions` (which gates on an active project) or
`open_milestones` (which reads index rows that, on a parked map, G4 has already deleted). An
index-backed report on a parked map would see no milestones, pass the refusal, and archive open
`- [ ]` lines unlisted.

`complete_project` stops there: a project with an open action or milestone is not completed
until each is completed or dropped (D11). `drop_project` with `open_items = OpenItems::Drop {
expected }` closes the children first, in the same transaction, through the same section edits
`drop_action` and `drop_milestone` make: each open bullet is removed, each attached note that is
not already closed (`blocked` included, D12) archived as `Closure::Dropped` (status `dropped`,
`completed` cleared, snapshot recorded — the archived prefix is frozen for a drop exactly as for
a completion), each open milestone removed with its continuation lines — each with its own log
line and `reason: project dropped`, or `reason: project dropped (<reason>)` when the user gave
one, so a child line found by grep explains itself. Then the project's own
line. Children first so the daily log reads in the order things happened.

`expected` is the `open_items_hash` the refusal returned, a hash of the report in source order.
Over MCP it is **required** whenever open items exist: an agent reads the refusal, asks its
user, then calls again with `"drop"`, and anything added meanwhile — a CLI `action add`, another
client — must not be dropped unseen. The domain compares it inside the locked transaction, after
the fresh report, and re-refuses with the fresh list on mismatch, the shape `revise_note`'s
`expected_hash` / `stale_revision` already has. The interactive CLI passes it internally on the
confirm-and-retry; a scripted `--drop-open` is exempt, since a human typed the flag against a
vault only they are writing to. The user never sees the hash.

Children are dropped, never completed, by the cascade, and only a drop cascades. A completion is
a claim about work, and the cascade cannot know the work happened; if it did, the user ticks it
first, which is what the refusal is for. `OpenItems::Refuse` is the default on every surface.

### 5.4 Where a closed project lives

`projects/_done/<year>/<slug>.md`, year of closure, for both outcomes — mirroring
`commitments/_done/<year>/` and `actions/_done/<year>/`, where dropped notes also live. The
stem is globally unique (#225), so `[[surrogate-model]]` keeps resolving after the move (#215).

A closed map is not frozen. `resolve_active_project` already refuses every mutating verb on it,
as it does for a parked map; a reactivation is legitimate, and so is a late retrospective —
though the map "is not a history (the log is)" (`rlm.md`), so the docs point a retrospective at
`cdno log` with `[[<slug>]]` rather than at a hand edit of the archived map. The archived-action
freeze (#111) stays what it is.

### 5.5 Location and dates: one locator, every move restages

**One locator, from the disk.** `locate_project(slug) -> ProjectLocation { path, frontmatter }`
probes `projects/<slug>.md`, `projects/_parked/<slug>.md`, then each year under
`projects/_done/` (`list_dir`, O(years)), and reads the frontmatter from the file. It does not
consult the index: where a verb *writes* must never depend on a cache that can be stale — after
an `IndexStale` commit, or a hand move in an editor, the stdio server reconciles only at startup
and the HTTP server every 300 s (`bin/server.rs`), and an index-backed locator would mis-resolve
every write in that window. A stem found at more than one location is refused as
`DomainError::AmbiguousProject { slug, candidates }` — a dedicated error, because
`Store(AlreadyExists)` means "destination occupied" and would tell the caller to pick another
name. Writes refuse; the documented active-beats-parked rule in `note_ref::narrow` stays for
reads. Every locator in G2 is rewritten on top of it, and `commitments()` builds its stem-to-path
map once rather than per action note. In core, `milestones_for_project` takes the resolved path
instead of guessing two candidates.

**Every move restages its rows.** `park`, `activate`, `complete` and `drop` stage
`replace_milestones` and `replace_deadlines` for the destination path from the final
`## Milestones` section, after `upsert_note(dest)` (the foreign key), the way the milestone verbs
already do through `stage_milestone_index_rows`. This is the fix for the first half of G4.

**The register asks the parent.** `commitments()` source 1 skips a milestone whose project is
not active, source 4 skips an action note whose project is not active. Source 3 is untouched: a
standalone commitment linked to a closed project keeps surfacing. Together with the restaging
this is the fix for #611, and it covers parked and closed projects with the same line.
`orientation_context` and the stuck scans already filter on `Active`; the active-only scans
(`active_projects`, `parked_projects`, `stuck_projects`) additionally skip `projects/_done/`
**by path** before parsing, because they fail on one malformed map, `_done/` grows for ever and
invites hand edits, and one broken retrospective must not block `create`, `activate` or `orient`.
G8's lint row catches a misfiled active map.

### 5.6 Reviews see closures, and the shelf

`Vault::closed_projects_between(from, to) -> Vec<ClosedProjectEntry { slug, title, context,
outcome, closed_on }>`, read from the `_done/` maps' `status` and `closed:` the way
`completed_actions_between` reads archived action notes. `get_weekly_context` and
`get_monthly_context` gain a `closed_projects` array, and `get_monthly_context` also gains
`parked_projects` — the shelf is short, the design says the monthly review revisits it, and with
§5.1 the review can then offer "keep parked or drop" as one decision per project. The review
skills can put a completed project in the wins and a dropped one in the decisions without
parsing prose.

### 5.7 Reactivation, and switching outcome

`activate_project` accepts a project from `_parked/` or `_done/<year>/`. Same cap check, same
collision guard, same `status: active`; `closed` is cleared, so a reactivated project can be
closed again with a fresh date. The log line stays `project [[s]] activated`.

This is an un-drop, and `rlm.md` says there is none "and none is planned". The exception is
named there (§6.5): a closed project keeps its slug (#225), so `activate` brings its map back
rather than making it again, and the log shows the closure and the reactivation as two
decisions. The doc comment on `ProjectStatus` ("completion is terminal") changes with it.

A closed project may also be closed again **with the other outcome**, directly: a dropped
project that turns out to have been finished, or the reverse, is a new decision taken today, and
`complete` / `drop` on a closed map restamp `status` and `closed:`, move it to
`_done/<current year>/` so the folder year matches, restage the rows, and write a new log line.
No cap check, by §5.1's logic. A same-outcome re-close is refused, with the message "already
completed on <date>" (§9, Q4).

---

## 6. Detailed design

### 6.1 `cdno-core`

- `paths.rs`: `PROJECTS_DONE = "projects/_done"`, `projects_done_dir(year)`, and the current
  year's folder in `init_dirs`. The doc comment on `PROJECTS` gains the third location.
- `index.rs`: `milestones_for_project(path: &VaultPath)` replaces the slug-and-guess signature;
  `project_path_candidates` is deleted. The trait's two implementations and the test wrappers in
  `tests/unit/transaction_tests.rs` and `tests/unit/reconcile_tests.rs` follow. `milestones_between`
  is unchanged — the status filter belongs to the domain, which has the frontmatter. No
  migration: the schema does not change, and reconciliation types a note by its frontmatter
  `type:`, so `projects/_done/2026/x.md` indexes as a project with no change.

### 6.2 `cdno-domain`

**Types.** `ProjectStatus::Dropped` (kebab `dropped`); `ProjectFrontmatter.closed:
Option<NaiveDate>` (optional field, absent reads as `None`, so every existing map parses);
`Closure { Completed, Dropped }` in `vault/closure.rs`, with `ActionClosure` becoming a type
alias or being replaced outright; `OpenItems { Refuse, Drop { expected: Option<OpenItemsHash> } }`;
`ProjectLocation`; `ClosedProjectEntry`; `ProjectClosureOutcome { outcome: WriteOutcome,
dropped_actions, dropped_milestones, untouched_commitments }`. Errors:
`DomainError::ProjectHasOpenItems` (§5.3); `DomainError::AmbiguousProject { slug, candidates }`
(§5.5); `DomainError::ProjectNotActive` gains the project's actual `status`, since after this RFC
it covers parked, completed and dropped and each has a different recovery; `ProjectNotParked`
keeps its name (codes are stable on the wire) with the message widened to "not parked or
closed".

**Frontmatter.** `closed` is written through `merge_fields_into_frontmatter`
(`frontmatter_edit.rs`), which already inserts an absent key safely with record sequences
(#481) and CRLF; no new upsert helper. `templates/project.md` gains `closed: null` **last**, and
the `frontmatter_order` pin for `project` puts it last too — placed after `created:` instead,
every pre-RFC map would trip the frontmatter-order lint Warning on its first closure, since the
merge appends before the closing `---`. `is_reserved_key` (`set_frontmatter.rs`) reserves
`closed` for `project`, so a settable schema cannot forge a closure date; today it reserves only
`type` and `status`.

**`locate_project`** (`projects/mod.rs`), per §5.5. The rewrites: `resolve_active_project`
(status must be `Active`), a new `resolve_closable_project` (status `Active` or `Parked`),
`resolve_any_project`, `update_project_state`, `get_project_full`, `resolve_project_path` in
`commitments.rs`, `available_projects_hint` (displays `(parked)`, `(completed)`, `(dropped)`),
`activate_project`, `project_summary` through `resolve_any_project`, and `note_ref::narrow`'s
project-only branch.

**`complete_project` / `drop_project`** (`projects/lifecycle.rs`), one transaction each, the
write lock taken before any read:

1. `resolve_closable_project(slug)` (active or parked); a map already closed is handled per §5.7,
   a same-outcome re-close refused;
2. the open-items report from the document (§5.3): open bullets from `## Next Actions`, each
   attached note's existence and status read from `actions/<slug>.md`, open milestones from
   `extract_milestones_from_body`, linked commitments from `commitments_for_project` filtered to
   `active`; hash it; if non-empty and completing, or dropping with `OpenItems::Refuse`, or
   `Drop { expected }` with a mismatching or absent hash where the caller must supply one,
   return `ProjectHasOpenItems`;
3. dropping with `OpenItems::Drop`: check **every** `_done` destination (each attached note's, then the
   project's) before staging anything, so a collision writes nothing; dedupe attached slugs, since
   two bullets linking one note would stage two deletes and the commit would roll back; skip an
   attached note already `completed` or `dropped` rather than restamp it, and drop every other,
   `blocked` included (D12);
   then, on the one `MarkdownDocument`, remove each open bullet and each open milestone with its
   continuation lines, using section helpers extracted from `drop_action` and `drop_milestone`
   (the public verbs open their own transaction, and the lock is not re-entrant), and
   `stage_action_archival(Closure::Dropped)` for each attached note;
4. stamp `status` and `closed` **on the rendered document**, not on a fresh `store.read_file`
   as `park_project` does — copying `park` would silently discard the section edits;
5. destination `projects/_done/<year>/<slug>.md` (checked in step 3);
6. `write_file` new, `delete_file` old, `upsert_note`, `remove_note`, then
   `replace_milestones` / `replace_deadlines` for the destination from the final section (§5.5);
7. one `stage_daily_logs` call with all N+1 entries, children first, then
   `project completed [[<slug>]] — <title>` or `project dropped on [[<slug>]] — <title>` with
   the `reason:` continuation when given. `<title>` is the body H1, as `body_title_or_slug` gives
   it. Both link the bare slug, so `mentions_project` and `daily_log_mentions` find them; a test
   pins that neither `project_state_changes_between` nor `current_focus` claims the new lines.

Returns `ProjectClosureOutcome`.

**`park_project` / `activate_project`**: locate through the new resolver; `activate` accepts
`Parked`, `Completed` or `Dropped` and clears `closed`; both restage milestone and deadline rows
at the destination (§5.5).

**Queries.** `parked_projects()` unchanged; `closed_projects()` added on the same pattern for
the CLI picker and `list_projects`; `closed_projects_between(from, to)` for the reviews;
`commitments()` gains the two status checks of §5.5, with `cached_project_context` widened to
cache `(Context, ProjectStatus)` and the stem-to-path map built once; the active-only scans skip
`_done/` by path.

**Lint** (`vault/lint.rs`). Rows: a project whose `status` disagrees with its folder is an
`Error` ("downstream code can trip over it": the locator trusts the frontmatter, so the file is
unreachable by the verb its folder suggests) — `active` outside `projects/`, `parked` outside
`projects/_parked/`, `completed` or `dropped` outside `projects/_done/<year>/`; a closed status
without `closed:`, and `active` or `parked` with one, are `Warning`s (nothing trips; the review
query skips the row). The mismatch message **names the manual fix**, because no verb repairs a
hand-closed map under `projects/` and none should: only the user knows when it ended, and a verb
would have to invent the date and write a closure line dated today for an event it never saw.
The message: "move the map to `projects/_done/<year>/` and set `closed: <date>`, or set
`status: active` and close it with `cdno project complete`".

### 6.3 `cdno-cli` (flags-and-prompts, `docs/cli-ergonomics.md`)

```
cdno project complete [--slug S]
cdno project drop     [--slug S] [--reason R] [--drop-open]
```

(`--json` is global.)

- `--slug` gathers through `gather_or_error` with a picker over active **and parked** projects,
  labelled; `--reason` is optional and never prompted for on `drop`, matching `cdno action drop`
  and `cdno commit drop`.
- On `ProjectHasOpenItems`, the refusal leads with what is there, not with an error (§1.1
  shows the text): the items, hard milestones marked with their date, linked commitments as
  "still open, not touched", then what to do. `complete` stops there (D11). `drop` without
  `--drop-open`, interactive — after the domain refusal, as `resolving_ambiguity` in `action.rs`
  already does for an ambiguous match — asks "Let these N go and drop <slug>? [y/N]" through
  `prompt_confirm(…, false)`: pressing Enter must never drop work. On `y`, retry with
  `OpenItems::Drop { expected: Some(hash) }`.
  Non-interactive — print the list, exit non-zero, and under `--json` emit the same object the
  MCP rejection carries (§6.4) on stdout, following `config validate`. One shape for both
  surfaces.
- Success leads with the outcome and the slot: "Completed <slug>. 4 of 5 slots in use." A drop
  reads "Dropped <slug>. 4 of 5 slots in use. Let go: 2 actions, 1 milestone." when it cascaded. The word "abandoned" appears in no
  user-facing text.
- Confirm only when something was prompted, per the convention.
- `cdno project activate`'s picker and a new `complete_reactivatable_project` completion cover
  parked and closed projects, labelled. `cdno project list` gains `--closed` (closed projects,
  newest first, with outcome and date; under `--json` it emits `ClosedProjectEntry`, since
  `ProjectSummary` carries no date); the default stays active-only. `cdno project show` already
  renders the status badge and needs the `Dropped` arm.

### 6.4 `cdno-mcp`

- `complete_project { project }` and `drop_project { project, reason?, open_items?: "refuse" |
  "drop", expected_open_items?: string }` in `lifecycle.rs`, `verified_write` with
  `WriteShape::Rewritten` on the destination path. Default `refuse`; `complete_project` always
  refuses while items are open (D11). The result of a drop names what the cascade dropped.
- The refusal is a `RejectionCode::ProjectHasOpenItems` classified in `rejection.rs`, which
  derives the wire code `project_has_open_items` from the variant name — `classify` has no
  wildcard arm, so the variant is added there:

  ```json
  {"code": "project_has_open_items", "message": "…",
   "details": {"slug": "…", "open_items_hash": "…",
               "actions": [{"text": "…", "note": "<slug>|null", "note_status": "active|null"}],
               "milestones": [{"title": "…", "date": "YYYY-MM-DD|null", "hard": true}],
               "untouched_commitments": [{"slug": "…", "due": "YYYY-MM-DD"}]}}
  ```

  A call with `"drop"` and no `expected_open_items` while items exist gets this rejection too —
  that is D2 anyway. `AmbiguousProject` classifies as `ambiguous_project` with
  `details: {slug, candidates}`, the shape `read_note`'s `ambiguous_slug` already uses, so an
  agent's recovery code is shared. `project_not_active` gains `status` (and `closed` when set) in
  `details`, so the message can read "already completed on 2026-09-29" and an agent can branch.
- `list_projects { include_closed?: bool }`, default false: the closed archive grows for years
  and this is "the lightweight enumeration tool". `ProjectListDto` gains `closed` when asked;
  `ProjectFrontmatterDto` gains `closed`.
- `get_weekly_context` gains `closed_projects`; `get_monthly_context` gains `closed_projects`
  (bounded by its 30-day window) and `parked_projects`, the latter noting that it inherits
  `parked_projects()`'s fail-on-one-bad-map behaviour.
- `get_project_context` resolves through `locate_project`, so a closed project is readable.
- Descriptions are the only instruction surface, so the stale ones change with the code:
  `activate_project` ("Activate a parked project"), `list_projects` ("active and parked"),
  `get_project_context` ("Resolves the slug against both `projects/` and `projects/_parked/`"),
  `get_weekly_context` (which enumerates its slices), and `get_monthly_context`. The new tools'
  descriptions state the rules: closing is refused while items are open; only `drop` may cascade
  them, as drops with a recorded reason; a completion is a claim about work and a drop is not; closing accepts
  a parked project and never needs a slot.
- The catalogue pins in `tests/server.rs` and `tests/e2e_stdio.rs` go to 60.

### 6.5 Documentation

`docs/design.md` §5.3 (status values, `closed:`, the `_done/` folder, the lifecycle paragraph),
the structural-conventions list, the note-type table's `project` row, and the CLI and MCP
surface tables. `docs-site`: `reference/cli/project.md` (two subcommands, `list --closed`,
`activate` from `_done/`), `reference/mcp/creation-and-lifecycle.md`,
`concepts/business-rules.md` (the cap paragraph), `concepts/vault-structure.md`,
`concepts/rlm.md` (line 85, "projects park" becomes "projects park, complete or drop"; and the
no-un-drop paragraph gains the exception of §5.7), and the projects tutorial. The `ProjectStatus`
doc comment. CLAUDE.md's history-preservation list gains the two prefixes. `CHANGELOG.md` and
`STATUS.md` with the shipping PRs.

---

## 7. Compatibility

- **Existing vaults need no migration.** `closed` is optional and absent on every existing map;
  the key is inserted, last, on first closure. `projects/_done/` is created on first use
  (`create_dir_all`) and by `cdno init` for new vaults. `cdno reindex` picks up `_done/` maps by
  their frontmatter type.
- **Existing behaviour changes in one place on purpose:** `park` and `activate` now keep a
  project's milestone rows in the index (§5.5). Everything else about `park`, `activate` from
  `_parked/`, `create`, and every project-body verb is unchanged. A project that is `Active`
  behaves exactly as before.
- **A hand-made `status: completed` map under `projects/`** (the shape
  `project_summary_returns_summary_for_completed_project` seeds) keeps parsing and rendering;
  `locate_project` finds it and reports it as not active; lint flags the folder mismatch and
  names the fix (§6.2).
- **Wikilinks keep resolving** across the move by the last-segment rule (#215), as they do for
  parked projects and archived actions.
- **Log-line family.** Two new fixed prefixes, `project completed ` and `project dropped on `,
  join the family; the `parked`/`activated` lines are left as written — a change would break
  nothing but would also buy nothing.
- **MCP clients** see two new tools, one new rejection code, one new optional input on
  `list_projects`, and widened payloads; nothing is removed or renamed.
- **`rlm.md`'s "no un-drop" rule** gains a named exception for projects (§5.7).

---

## 8. Implementation plan — staged

One PR per stage; each leaves `just ci` green and ships on its own.

- **S0 — Location.** `PROJECTS_DONE` and `projects_done_dir` in core; `locate_project` (from the
  disk) and `AmbiguousProject` in the domain, and the G2 rewrites onto it; `milestones_for_project`
  by path, with the core test wrappers. No behaviour change; the tests that pin the two-path
  probes move to the locator.
- **S1 — Moves keep their dates, and the register asks the parent.** `park` and `activate`
  restage milestone and deadline rows; the status filter in `commitments()` sources 1 and 4; the
  active-only scans skip `_done/` by path. Closes #611 and the reproduced bug. The two halves
  ship together: restaging alone would make a parked project's dates nag reliably.
- **S2 — The verbs.** `Closure`, `ProjectStatus::Dropped` (with the exhaustive-match arms in
  `cdno-cli/src/commands/project.rs` and `cdno-mcp/src/dto.rs`, or S2 does not compile), `closed`
  on the frontmatter and template (last), the `is_reserved_key` reservation, `complete_project`,
  `drop_project`, the open-items report, hash and cascade, `activate` from `_done/`, the outcome
  switch, `closed_projects()`. Domain tests in `tests/unit/projects_tests.rs`.
- **S3 — CLI.** The two subcommands, the refusal and success wording, `list --closed`, the
  widened pickers and completions. `assert_cmd` wiring tests only.
- **S4 — MCP.** The two tools, the rejection code and shape, `expected_open_items`,
  `list_projects.include_closed`, `get_project_context` on a closed project, the description
  updates. Handler tests plus both catalogue pins.
- **S5 — Reviews.** `closed_projects_between`, `closed_projects` in both contexts,
  `parked_projects` in the monthly one.
- **S6 — Lint and docs.** The status/folder and `closed:` rows with the repair message;
  `docs/design.md`; `docs-site`; CLAUDE.md; CHANGELOG and STATUS. Closes #667.

S0 and S1 are safe to merge before the RFC is accepted: they change no user-visible behaviour
except fixing #611 and the lost-rows bug.

---

## 9. Decisions and open questions

Settled by the three-seat review unless marked otherwise.

**D1 — Both outcomes archive to `_done/<year>/`.** Not `_dropped/`. Commitments and actions
already put dropped notes under `_done/`; the outcome is in the frontmatter and the log, not the
folder name. One tree to reconcile, one tree to `activate` from.

**D2 — Refuse by default, cascade on request.** The alternative — always cascade and report —
would make an MCP call with a typo in the slug irreversible before anyone read the result. The
refusal costs one round trip and makes the drop an explicit decision. Interactive CLI folds the
round trip into a confirm that defaults to No.

**D3 — The cascade drops, never completes.** A completion is a claim about work; the tool
cannot make it on the user's behalf. Open items that were in fact done are ticked first, which
the listing prompts for. A child's `reason:` carries the user's own reason on a drop.

**D4 — Waiting-on lines are not children.** Nothing downstream reads them; they stay in the
closed map as part of its record. Linked standalone commitments are named but never touched.

**D5 — A closed map is not frozen.** Reactivation is real; the mutating verbs already refuse a
non-active map; retrospectives go to the log. The active-only scans skip `_done/` by path so a
hand-edited archive cannot block the live vault.

**D6 — Log-line shapes** are `project completed [[<slug>]] — <title>` and `project dropped on
[[<slug>]] — <title>` (+ `reason:`), matching commitments. The bare slug is linked so the
existing project-mention matcher finds them.

**D7 — One `closed:` date, no `completed:` on projects.** All three seats ended here: nothing
reads a project's `completed:`, on a completion it would always equal `closed:`, and the second
key is an invariant for `activate` to clear and lint to police, for no reader. **The maintainer
confirms**: the seats would each accept `closed:` plus `completed:` for symmetry with actions and
commitments, with an equality lint row as an `Error`.

**D8 — Closing accepts a parked project and never cap-checks.** Ending a shelved plan must not
cost a slot or a false `activated` line (§5.1).

**D9 — The locator reads the disk.** The index is a cache, and a write's destination may not
depend on it (§5.5).

**D10 — An MCP cascade carries the hash of the list it was shown.** The user never sees it; what
it prevents is an item added between the refusal and the cascade being dropped unseen (§5.3).

**D11 — `complete` never cascades** (the maintainer, 2026-09-30). A project with an open action
or milestone refuses to be completed, whatever the caller passes; each item is completed or
dropped first, with its own verb and its own reason. `drop` keeps the refusal by default and the
explicit cascade (`--drop-open`, the confirm, MCP `"drop"` with the hash). A completion that
silently drops open work would record the project as done while its log says part of it was not.

**D12 — The cascade drops every attached note that is not closed** (the maintainer,
2026-09-30). Children that are not completed inherit the drop, so an attached action note in
any status but `completed` or `dropped` — `blocked` included — is archived as dropped with the
project. Skipping only non-`active` notes would leave a blocked note live in `actions/` under a
closed project. Linked standalone commitments are still never touched (D4).

**Q1 — Should `drop_action` / `drop_milestone` accept a parked project?** (#611, first
candidate.) **No.** Once S1 lands a parked project's dates stop nagging, so the pruning case is
gone, and D8 covers the ending case. The rule is one line: verbs that change a project's plan need
it active; verbs that end it accept active or parked.

**Q2 — Carrying an open action forward to another project.** #667 mentions "explicitly carried
forward". There is no move-action verb, and `rlm.md` settles that a drop is re-decided rather than
undone: the action is re-added on the project that now owns it, which the refusal text says.
**Not in this RFC**; open an issue if the `reason:` line proves insufficient as the pointer.

**Q3 — `ActionStatus::Blocked`.** Same class of gap as G1, different concern. **A separate
issue**, except for the cascade, which D12 settles.

**Q4 — Closing a project that is already closed.** A same-outcome re-close is refused with
"already completed on <date>" and `status` in the rejection's `details`; the other outcome is
allowed directly as a new decision (§5.7). A hand-closed map under `projects/` is repaired by
hand, with lint naming the steps (§6.2): a verb that filed an old closure would write a log line
dated today for something that happened earlier.

---

## 10. Verification

Per crate, the tests each stage adds, in already-registered files; the names follow the suite's
style.

- `cdno-core` (`tests/unit/vault_index_tests.rs`, `memory_index_tests.rs`, `paths` tests):
  `projects_done_dir_is_year_partitioned`; `init_dirs_includes_projects_done_for_the_current_year`;
  `milestones_for_project_reads_the_given_path`.
- `cdno-domain` (`projects_tests.rs`): `locate_project_finds_active_parked_and_closed`;
  `locate_project_resolves_by_disk_when_index_is_stale`;
  `locate_project_refuses_a_stem_at_two_locations_as_ambiguous_project`;
  `park_then_activate_keeps_milestones_in_register`;
  `complete_project_moves_stamps_and_logs`; `complete_project_restages_milestone_rows_at_destination`;
  `complete_project_refuses_with_open_items_listed`; `complete_project_lists_hard_milestones_and_untouched_commitments`;
  `drop_project_drop_open_cascades_as_drops_with_reason`;
  `drop_project_cascade_archives_attached_notes_as_dropped_and_frozen`;
  `drop_project_cascade_dedupes_attached_note`; `drop_project_cascade_skips_closed_attached_note`;
  `drop_project_cascade_drops_blocked_attached_note`;
  `drop_project_cascade_collision_writes_nothing`;
  `drop_project_logs_children_then_project_in_one_write`;
  `drop_project_refuses_stale_open_items_hash`; `drop_project_clears_nothing_and_stamps_closed`;
  `drop_project_closes_a_parked_project_at_cap`; `drop_project_at_cap_lists_open_milestone_of_parked_project`;
  `drop_project_carries_reason_to_children`; `complete_project_refuses_an_occupied_destination`;
  `complete_project_on_dropped_project_switches_outcome`; `complete_project_on_completed_project_is_refused`;
  `activate_project_from_done_clears_closed_and_cap_checks`;
  `closing_pre_rfc_map_leaves_frontmatter_in_canonical_order`;
  `closed_projects_between_returns_both_outcomes`; `active_projects_excludes_closed`;
  `active_projects_ignores_malformed_done_map`;
  `project_closure_lines_not_parsed_as_state_or_focus`.
  (`commitments_tests.rs`): `commitments_skips_milestones_of_parked_projects`;
  `commitments_skips_milestones_of_closed_projects`; `commitments_skips_action_dues_of_non_active_projects`;
  `commitments_keeps_standalone_commitment_of_closed_project`.
  (`frontmatter_edit_tests.rs`): `merge_inserts_closed_into_map_with_record_sequence`.
  (`set_frontmatter_tests.rs`): `set_frontmatter_refuses_closed_on_project`.
  (`lint_tests.rs`): `lint_errors_on_project_status_folder_mismatch`;
  `lint_names_fix_for_closed_map_outside_done`; `lint_warns_on_closed_status_without_closed_date`;
  `lint_warns_on_active_project_with_closed_date`.
- `cdno-cli` (`project.rs` target): `complete_non_interactive_lists_open_items_and_fails`;
  `complete_json_refusal_matches_mcp_shape`; `drop_drop_open_succeeds_and_prints_destination`;
  `drop_with_reason_writes_reason_line`; `drop_parked_project_needs_no_slot`;
  `list_closed_shows_outcome_and_date`; `activate_from_done_works`.
- `cdno-mcp`: `complete_project_rejection_carries_open_items_and_hash`;
  `drop_project_with_stale_hash_is_re_refused`; `drop_project_with_open_items_drop_cascades`;
  `ambiguous_project_rejection_carries_candidates`; `project_not_active_details_carry_status`;
  `list_projects_includes_closed_only_when_asked`; `get_weekly_context_includes_closed_projects`;
  `get_monthly_context_includes_parked_and_closed_projects`; both tool-catalogue pins at 60.
