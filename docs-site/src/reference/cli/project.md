# `cdno project`

Manage project maps: create, update state, add/complete milestones, manage waiting-on items,
park/activate, and list/show. Next actions have their own verb, [`cdno action`](action.md).

```text
cdno project [OPTIONS] <COMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| [`create`](#cdno-project-create) | Create a new project map |
| [`state`](#cdno-project-state) | Update the Current State (auto-logs the previous) |
| [`park`](#cdno-project-park) | Move a project to `_parked/` |
| [`activate`](#cdno-project-activate) | Bring a parked project back (enforces the cap) |
| [`list`](#cdno-project-list) | List active projects |
| [`show`](#cdno-project-show) | Show one project |
| [`milestone`](#cdno-project-milestone) | Add / complete milestones |
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

List active projects with a state snippet. Honours `--json`.

```bash
cdno project list
cdno project list --json | jq '.[].slug'
```

## `cdno project show`

Show a compact summary of a single project (any status). Takes the slug as a positional argument.
Honours `--json` (emits the project summary object).

```bash
cdno project show surrogate-model
cdno project show surrogate-model --json
```

## `cdno project milestone`

Manage milestones — dated markers of progress. A `--hard` milestone is a real deadline counted in
[`cdno commitments`](commitments.md).

**`add`** — `--slug`, `--title`, `--date <YYYY-MM-DD>`, `--hard`
**`done`** — `--slug`, `--query` (case-insensitive substring of the milestone title)

```bash
cdno project milestone add --slug surrogate-model --title "Submit to ICML" --date 2026-01-22 --hard
cdno project milestone done --slug surrogate-model --query "submit to icml"
```

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
[`complete_milestone`](../mcp/writes.md), [`add_waiting_on`](../mcp/writes.md),
[`resolve_waiting_on`](../mcp/writes.md).

## See also

- [Managing projects](../../tutorials/projects.md).
- [`action`](action.md) — the next-action list.
