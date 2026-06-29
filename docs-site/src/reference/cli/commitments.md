# `cdno commitments`

List aggregated commitments across the vault — project hard milestones, standalone commitment notes,
stewardship periodic commitments, and self-imposed action-note deadlines — sorted by date, with
overdue items flagged.

```text
cdno commitments [OPTIONS]
```

## Options

| Flag | Description |
|------|-------------|
| `--weeks <WEEKS>` | Lookahead window, in weeks. Default `2`. A standing 30-day overdue look-back always applies on top. |

Plus the [global options](overview.md#global-options). With `--json`, emits the list as structured
data.

## Examples

```bash
cdno commitments              # next 2 weeks + anything overdue
cdno commitments --weeks 6    # next 6 weeks
cdno commitments --json | jq '.[] | select(.overdue)'
```

## How it's computed

This is a derived view, not a single file. It merges four sources (see
[Business rules](../../concepts/business-rules.md#commitments-are-aggregated-not-stored-in-one-place)):
project milestones marked `--hard`, stewardship periodic commitments, standalone
[`commit`](commit.md) notes, and action notes with a self-imposed `due:`.

## Related MCP tool

[`get_commitments`](../mcp/reads.md).

## See also

- [Commitments and deadlines](../../tutorials/commitments.md).
- [`commit`](commit.md) — create a standalone commitment.
