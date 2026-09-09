# `cdno project`

Manage project maps: create, update state, set the core question, add/complete/drop milestones, manage
waiting-on items, park/activate, and list/show. Next actions have their own verb, [`cdno action`](action.md).

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
| [`activate`](#cdno-project-activate) | Bring a parked project back (enforces the cap) |
| [`list`](#cdno-project-list) | List active projects |
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

## `cdno project activate`

Bring a parked project back. Fails if it would exceed the active cap — park another first.

| Flag | Description |
|------|-------------|
| `--slug <SLUG>` | Parked project slug. |

```bash
cdno project activate --slug surrogate-model
```

## `cdno project list`

List active projects with a state snippet. Each project renders as a card — a coloured bar keyed to
its context, the slug as a title, and the state wrapped underneath. Honours `--json`.

In a terminal this then offers to open one of the projects it just listed, printing the same thing
`cdno project show` would and asking again until you press Esc. Piped output, `--no-interactive`, and
`--json` skip the prompt. See [Colour and interactivity](../colour-and-interactivity.md).

```bash
cdno project list
cdno project list --json | jq '.[].slug'
cdno project list --no-interactive     # listing only, never a prompt
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
[`activate_project`](../mcp/creation-and-lifecycle.md), [`list_projects`](../mcp/reads.md),
[`get_project_context`](../mcp/reads.md), [`add_milestone`](../mcp/writes.md),
[`complete_milestone`](../mcp/writes.md), [`set_core_question`](../mcp/writes.md),
[`add_waiting_on`](../mcp/writes.md),
[`resolve_waiting_on`](../mcp/writes.md).

## See also

- [Managing projects](../../tutorials/projects.md).
- [`action`](action.md) — the next-action list.
