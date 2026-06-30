# `cdno stewardship`

Manage stewardship dashboards: create (flat or expanded), list, show, and append a periodic
commitment line. Filing a tracking entry is the separate [`cdno track`](track.md) verb.

```text
cdno stewardship [OPTIONS] <COMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| [`create`](#cdno-stewardship-create) | Create a stewardship (flat, or expanded with `--tracking`) |
| [`list`](#cdno-stewardship-list) | List stewardships with variant, tracking count, staleness |
| [`show`](#cdno-stewardship-show) | Show a stewardship's frontmatter + dashboard excerpt |
| [`add-periodic`](#cdno-stewardship-add-periodic) | Append a periodic commitment line |

`create`/`add-periodic` honour `--json` (`{path, message}`); `list`/`show` emit their data under
`--json`.

---

## `cdno stewardship create`

Create a stewardship dashboard. `--tracking` makes it **expanded** (a `stewardships/<slug>/` folder
with room for `tracking/` and `routines/`); without it, the dashboard is a single flat file.

| Flag | Description |
|------|-------------|
| `--name <NAME>` | Human-readable name (the slug derives from it). |
| `--context <CONTEXT>` | Life domain (`work`, `household`, `personal`, …). |
| `--tracking` | Create the expanded variant with a `tracking/` folder. |
| `--var <NAME=VALUE>` | Value for a custom template's prompted variable ([`[variables.prompt]`](../configuration.md)). Repeatable. See [Prompted variables](../../tutorials/templates-and-frontmatter.md#prompted-variables). |

```bash
cdno stewardship create --name "Finances" --context household           # flat
cdno stewardship create --name "Health" --context personal --tracking   # expanded
```

## `cdno stewardship list`

List every stewardship with its variant, tracking count, and staleness badge. Honours `--json`.

```bash
cdno stewardship list
cdno stewardship list --json | jq '.[] | {slug, variant}'
```

## `cdno stewardship show`

Show a stewardship's frontmatter and an excerpt of the dashboard body. Honours `--json` (a detail
object including `variant` and `body_markdown`).

| Flag | Description |
|------|-------------|
| `--slug <SLUG>` | Stewardship slug. |

```bash
cdno stewardship show --slug health
```

## `cdno stewardship add-periodic`

Append a periodic commitment line to the dashboard's `## Periodic Commitments` section. The line
becomes a row in the aggregated [`cdno commitments`](commitments.md) view.

| Flag | Description |
|------|-------------|
| `--stewardship <SLUG>` | Stewardship slug. |
| `--title <TITLE>` | Commitment title (e.g. "Dental check-up"). |
| `--every <RECURRENCE>` | Recurrence: `daily`, `weekly`, `monthly`, `yearly`, or `every N months`. See [Recurrence syntax](../recurrence.md). |
| `--next <YYYY-MM-DD>` | Next due date. |

```bash
cdno stewardship add-periodic --stewardship health --title "Dental check-up" \
     --every "every 6 months" --next 2026-09-01
```

## Related MCP tools

[`create_stewardship`](../mcp/creation-and-lifecycle.md),
[`get_stewardship_tracking`](../mcp/reads.md),
[`add_periodic_commitment`](../mcp/creation-and-lifecycle.md).

## See also

- [Stewardships and tracking](../../tutorials/stewardships-and-tracking.md).
- [`track`](track.md) — file a tracking entry.
