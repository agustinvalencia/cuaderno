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
| [`complete-periodic`](#cdno-stewardship-complete-periodic) | Complete one occurrence, rolling `next:` forward |

`create`/`add-periodic`/`complete-periodic` honour `--json` (`{path, message}`); `list`/`show` emit their data under
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

In a terminal this then offers to open one of the stewardships it just listed, printing what `cdno
stewardship show` would and asking again until you press Esc. Piped output, `--no-interactive`, and
`--json` skip the prompt. See [Colour and interactivity](../colour-and-interactivity.md).

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

## `cdno stewardship complete-periodic`

Complete one occurrence of a periodic commitment, rolling its `next:` date forward by that line's
own recurrence. Named to pair with `add-periodic`: it completes an *occurrence*, not the
stewardship, which is perpetual and never completes.

| Flag | Description |
|------|-------------|
| `--stewardship <SLUG>` | Stewardship slug. |
| `--title <SUBSTRING>` | Case-insensitive substring of the commitment's title. |
| `--at <YYYY-MM-DD>` | Date the work was actually done. Defaults to today. |

```bash
cdno stewardship complete-periodic --stewardship health --title "dental"
cdno stewardship complete-periodic --stewardship health --title "dental" --at 2026-08-25
```

Before this verb there was no way to mark a periodic commitment done: it is a bullet, not a note, so
[`cdno commit done`](commit.md) has nothing to act on, and the reminder kept firing until the file
was hand-edited — the one edit the vault asks you not to make.

**The next date is computed from the due date, never from `--at`.** A check-up every 6 months, done
a week early each time, would creep a week earlier every cycle if the schedule followed the work.
Anchored to the due date, a run of early completions leaves the schedule exactly where it was. A
late completion advances until `next:` is in the future, so one neglected commitment comes back on
schedule rather than several reminders deep.

The entry records the completion and where the schedule moved to:

```text
- **09:30**: periodic done on [[health]] — Dental check-up
  was: 2026-09-01
  now: 2027-03-01
```

A line whose recurrence the parser cannot read is refused rather than guessed — see
[Recurrence syntax](../recurrence.md) for the accepted forms. Such a line is otherwise unaffected:
it still appears in `cdno commitments` and lint still accepts it.

## Related MCP tools

[`create_stewardship`](../mcp/creation-and-lifecycle.md),
[`get_stewardship_tracking`](../mcp/reads.md),
[`add_periodic_commitment`](../mcp/creation-and-lifecycle.md),
[`complete_periodic`](../mcp/writes.md).

## See also

- [Stewardships and tracking](../../tutorials/stewardships-and-tracking.md).
- [`track`](track.md) — file a tracking entry.
