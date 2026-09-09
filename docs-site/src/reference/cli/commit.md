# `cdno commit`

Manage standalone commitments — dated promises kept as their own notes. To *view* all commitments
(from every source), use [`cdno commitments`](commitments.md).

```text
cdno commit [OPTIONS] <COMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| [`create`](#cdno-commit-create) | Create an active commitment at `commitments/<slug>.md` |
| [`done`](#cdno-commit-done) | Mark a commitment completed and archive it |
| [`reschedule`](#cdno-commit-reschedule) | Move an active commitment's due date |

All three honour `--json` (`{path, message}`, non-interactive).

---

## `cdno commit create`

Create an active commitment note. Optionally attribute it to a project or stewardship.

| Flag | Description |
|------|-------------|
| `--title <TITLE>` | Commitment title (the slug derives from it). |
| `--due <YYYY-MM-DD>` | Deadline. |
| `--context <CONTEXT>` | Life domain (`work`, `personal`, …). |
| `--project <SLUG>` | Optional associated project. |
| `--stewardship <SLUG>` | Optional associated stewardship. |
| `--var <NAME=VALUE>` | Value for a custom template's prompted variable ([`[variables.prompt]`](../configuration.md)). Repeatable. See [Prompted variables](../../tutorials/templates-and-frontmatter.md#prompted-variables). |

```bash
cdno commit create --title "Pay rent" --due 2026-06-01 --context personal
cdno commit create --title "Review Erik's draft" --due 2026-05-20 --context work --project projects/icml-paper
```

## `cdno commit done`

Mark a commitment completed: stamps `status` and `completed`, and moves the note to
`commitments/_done/<year>/<slug>.md`.

| Flag | Description |
|------|-------------|
| `--slug <SLUG>` | Commitment slug. |

```bash
cdno commit done --slug pay-rent
```

## Related MCP tools

[`create_commitment`](../mcp/writes.md), [`complete_commitment`](../mcp/writes.md). (View via
[`get_commitments`](../mcp/reads.md).)

## See also

- [Commitments and deadlines](../../tutorials/commitments.md).
- [`commitments`](commitments.md) — the aggregated view.

## `cdno commit reschedule`

Move an active commitment's `due` date, recording the move in today's daily note.

| Flag | Description |
|------|-------------|
| `--slug <SLUG>` | Slug of the active commitment. |
| `--due <YYYY-MM-DD>` | The new due date. Must differ from the current one. |

```bash
cdno commit reschedule --slug quarterly-report --due 2026-06-15
```

Commitments slip, and that is normal rather than an error state. Use this instead of deleting and
recreating the note: recreating destroys the body — the notes on who was chased and what was
promised — and resets `created`, the one field that shows how long something has been slipping.

The log entry carries **both** dates:

```text
- **09:30**: commitment rescheduled on [[quarterly-report]] — Quarterly report
  was: 2026-06-01
  now: 2026-06-15
```

That is what makes repeated slippage visible instead of silently rewritten: a commitment moving
once is a checkpoint, a commitment moving for the fourth time is a signal. Moving a date earlier is
allowed; moving it to the date it already carries is refused, since the entry would assert a slip
that never happened.
