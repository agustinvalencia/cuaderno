# `cdno project`

Manage project maps: create, update state, set the core question, add/complete/drop milestones, manage
waiting-on items, park/activate, complete/drop the project itself, and list/show. Next actions have their own verb, [`cdno action`](action.md).

```text
cdno project [OPTIONS] <COMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| [`create`](#cdno-project-create) | Create a new project map |
| [`state`](#cdno-project-state) | Update the Current State (auto-logs the previous) |
| [`core-question`](#cdno-project-core-question) | Set or clear the core question (auto-logs the previous) |
| [`park`](#cdno-project-park) | Move a project to `_parked/` |
| [`complete`](#cdno-project-complete) | Close a project whose work is done (moves it to `_done/<year>/`) |
| [`drop`](#cdno-project-drop) | Close a project that is not going to happen (moves it to `_done/<year>/`) |
| [`activate`](#cdno-project-activate) | Bring a parked or closed project back (enforces the cap) |
| [`list`](#cdno-project-list) | List active projects, or closed ones with `--closed` |
| [`show`](#cdno-project-show) | Show one project |
| [`milestone`](#cdno-project-milestone) | Add / complete / drop milestones |
| [`waiting`](#cdno-project-waiting) | Add / resolve waiting-on items |

Write subcommands honour `--json` (a `{path, message}` result, run non-interactively); `list`/`show`
emit their data under `--json`.

---

## `cdno project create`

Create a new project map. Created **parked** if you're already at the active cap.

| Flag | Description |
|------|-------------|
| `--title <TITLE>` | Project title (the slug derives from it). |
| `--context <CONTEXT>` | Life domain: `work`, `side-project`, `university`, `family`, `household`, `legal`, `personal`. |
| `--question <QUESTION>` | Vault-relative core-question wikilink target (e.g. `questions/research/foo`). Optional. |
| `--var <NAME=VALUE>` | Value for a custom template's prompted variable ([`[variables.prompt]`](../configuration.md)). Repeatable. See [Prompted variables](../../tutorials/templates-and-frontmatter.md#prompted-variables). |

```bash
cdno project create --title "Surrogate model" --context work
cdno project create --title "Thesis" --context university --var ticket=ABC-123
cdno project create --title "Thesis" --context university --question questions/research/surrogate-cost
```

## `cdno project state`

Update the Current State section. The previous state is auto-logged to today's daily note first (see
[Business rules](../../concepts/business-rules.md#project-state-history-is-preserved)).

| Flag | Description |
|------|-------------|
| `--slug <SLUG>` | Project slug. |
| `--text <TEXT>` | The new state text. |

```bash
cdno project state --slug surrogate-model --text "Mesh scaling works; assembly is the bottleneck"
```

## `cdno project core-question`

Set or clear the project's core question after creation, auto-logging the previous value to today's
daily note in the same `was:` / `now:` shape [`state`](#cdno-project-state) uses.

**`core-question`** — `--slug`, and either `--question <target>` or `--clear`

```bash
cdno project core-question --slug surrogate-model --question questions/research/does-it-scale
cdno project core-question --slug surrogate-model --clear
```

`--question` takes the **bare** wikilink target, the same form
[`create --question`](#cdno-project-create) takes — `questions/research/foo`, not `[[…]]`, which is
rejected rather than double-wrapped.

Passing neither flag non-interactively is an error, not a silent detach: dropping a project's
question is a decision and has to be asked for.

## `cdno project park`

Move an active project to `projects/_parked/`, freeing a slot against the five-project cap.

| Flag | Description |
|------|-------------|
| `--slug <SLUG>` | Project slug. |

```bash
cdno project park --slug surrogate-model
```

## `cdno project complete`

Close a project because its work is done. The map moves to `projects/_done/<year>/<slug>.md` with
`status: completed` and `closed:` set to today, and the daily log gets
`project completed [[<slug>]] — <title>`. It works on an active or a parked project and never needs a
slot, since closing adds nothing to the active count.

| Flag | Description |
|------|-------------|
| `--slug <SLUG>` | Active or parked project slug. |

A completion is a claim that the work was done, so it is **refused while any action or milestone is
still open**, and there is no flag to force it. The refusal lists what is open:

```text
surrogate-model has 3 open items:
  - [ ] Run feature set B on full geometry mesh (deep)
  - [ ] [[actions/characterise-kan-ppo-sample-efficiency]] (deep)
  - [ ] ICML paper submitted — hard: 2026-05-22
Still open, not touched: [[commitments/reviewer-report]] (due 2026-10-15)
Complete or drop each of them (or add it to the project that now owns it), then run again.
```

Tick what was done ([`cdno action done`](action.md), [`milestone done`](#cdno-project-milestone)), drop
what is not happening ([`cdno action drop`](action.md), [`milestone drop`](#cdno-project-milestone),
each with its own reason), and run the command again:

```bash
cdno project complete --slug surrogate-model
# Completed surrogate-model. 4 of 5 slots in use.
```

Standalone commitments linked to the project are never touched: a promise to someone else does not
end with the project. The success message names any that are still open.

## `cdno project drop`

Close a project that is not going to happen. The map moves to `projects/_done/<year>/<slug>.md` with
`status: dropped` and `closed:` set to today, and the daily log gets
`project dropped on [[<slug>]] — <title>`, with the reason on an indented `reason:` line. Like
`complete`, it works on an active or a parked project and never needs a slot: ending a shelved plan
does not bring it back first.

| Flag | Description |
|------|-------------|
| `--slug <SLUG>` | Active or parked project slug. |
| `--reason <REASON>` | Why it is being dropped. Optional and never prompted for, but worth giving. |
| `--drop-open` | Drop the project's open actions and milestones with it, without asking. |

With open actions or milestones, `drop` lists them as `complete` does. In a terminal it then asks
`Let these N go and drop <slug>? [y/N]`, and pressing Enter keeps everything. On `y` each open item
is dropped with the project, logged as its own `action dropped on` / `milestone dropped on` line
with `reason: project dropped (<your reason>)`, and an attached action note is archived as dropped.
If the list changed while you were answering, nothing is dropped and the new list is shown.
Non-interactively the list is printed and the command exits 1, unless you pass `--drop-open`.

```bash
cdno project drop --slug bayesian-opt-survey --reason "superseded by the ICML work"
cdno project drop --slug bayesian-opt-survey --reason "superseded" --drop-open
# Dropped bayesian-opt-survey. 4 of 5 slots in use. Let go: 2 actions, 1 milestone.
```

Items that were in fact done should be ticked first: a drop records that work was let go, not that
it was finished.

Under `--json` a refusal from either verb prints the same `project_has_open_items` object the MCP
server returns (the items plus an `open_items_hash`) and exits 1; a success prints `path`,
`message`, `dropped_actions`, `dropped_milestones` and `untouched_commitments`.

A closed project can be closed again with the other outcome, as a new decision: a dropped project
that turns out to have been finished can be completed, and the reverse. It is re-filed under this
year's folder with today's `closed:` date and a new log line. Closing it again with the same outcome
is refused, naming the date it already has.

## `cdno project activate`

Bring a parked, completed or dropped project back to `projects/`. A closed project also has its
`closed:` date cleared, so it can be closed again later with a fresh one. The log says
`project [[<slug>]] activated`. Fails if it would exceed the active cap — park another first. The
interactive picker and shell completion offer parked and closed projects, labelled.

| Flag | Description |
|------|-------------|
| `--slug <SLUG>` | Parked or closed project slug. |

```bash
cdno project activate --slug surrogate-model
```

## `cdno project list`

List active projects with a state snippet. Each project renders as a card — a coloured bar keyed to
its context, the slug as a title, and the state wrapped underneath. Honours `--json`.

In a terminal this then offers to open one of the projects it just listed, printing the same thing
`cdno project show` would and asking again until you press Esc. Piped output, `--no-interactive`, and
`--json` skip the prompt. See [Colour and interactivity](../colour-and-interactivity.md).

With `--closed` it lists completed and dropped projects instead, newest first, each with its outcome
and closing date. Under `--json` each row carries `slug`, `title`, `context`, `outcome` and
`closed_on`.

```bash
cdno project list
cdno project list --json | jq '.[].slug'
cdno project list --no-interactive     # listing only, never a prompt
cdno project list --closed
```

## `cdno project show`

Show a compact summary of a single project (any status). The slug is an optional positional: omit it
in a terminal and `cdno` offers a picker covering active and parked projects. Honours `--json`
(emits the project summary object).

```bash
cdno project show surrogate-model
cdno project show                      # pick from a list
cdno project show surrogate-model --json
```

## `cdno project milestone`

Manage milestones — markers of progress. A `--hard` milestone is a real deadline counted in
[`cdno commitments`](commitments.md).

**`add`** — `--slug`, `--title`, `--date <YYYY-MM-DD>` (optional), `--hard`
**`done`** — `--slug`, `--query` (case-insensitive substring of the milestone title)

**`drop`** — `--slug`, `--query`, `--reason` (optional)

```bash
cdno project milestone add --slug surrogate-model --title "Submit to ICML" --date 2026-01-22 --hard
cdno project milestone add --slug surrogate-model --title "All Round-1 replies received"
cdno project milestone done --slug surrogate-model --query "submit to icml"
cdno project milestone drop --slug surrogate-model --query "book the venue" --reason "the funder withdrew"
```

Use `drop` rather than `done` when the milestone is not going to happen — superseded, mis-typed, or
overtaken by events. `done` ticks the bullet and writes `milestone done on ...` to the daily log,
asserting a milestone that was met; `drop` removes the bullet and writes `milestone dropped on ...`
instead, so a later reader can tell a plan that changed from a plan that was kept. `--reason` is
optional and never prompted for: a correction is simply a drop with no reason. Only open `- [ ]`
bullets are matched — a completed milestone is a record of what happened, not a plan to revise.

`--date` is optional. Some milestones are gated by a condition rather than a date — "all Round-1
replies received" on a correspondence-driven project — and omitting the flag records the milestone
as `target: TBD` rather than making you invent an estimate. An undated milestone does **not** appear
in [`cdno commitments`](commitments.md), which is the point: a date you made up reads later like a
commitment somebody made. It completes with `done` exactly like a dated one.

Interactively, the calendar is offered behind a yes/no so the undated case is reachable without
knowing the flag can be omitted.

`--hard` requires `--date`: a hard deadline with no date is rejected rather than quietly downgraded
to a soft target.

## `cdno project waiting`

Track external blockers.

**`add`** — `--slug`, `--description`
**`resolve`** — `--slug`, `--query` (substring of the item)

```bash
cdno project waiting add --slug surrogate-model --description "Cluster quota from IT"
cdno project waiting resolve --slug surrogate-model --query "cluster quota"
```

## Related MCP tools

[`create_project`](../mcp/creation-and-lifecycle.md), [`update_project_state`](../mcp/writes.md),
[`park_project`](../mcp/creation-and-lifecycle.md),
[`complete_project`](../mcp/creation-and-lifecycle.md),
[`drop_project`](../mcp/creation-and-lifecycle.md),
[`activate_project`](../mcp/creation-and-lifecycle.md), [`list_projects`](../mcp/reads.md),
[`get_project_context`](../mcp/reads.md), [`add_milestone`](../mcp/writes.md),
[`complete_milestone`](../mcp/writes.md), [`set_core_question`](../mcp/writes.md),
[`add_waiting_on`](../mcp/writes.md),
[`resolve_waiting_on`](../mcp/writes.md).

## See also

- [Managing projects](../../tutorials/projects.md).
- [`action`](action.md) — the next-action list.
