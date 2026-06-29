# Recurrence syntax

Periodic commitments on a stewardship (`--every` on
[`cdno stewardship add-periodic`](cli/stewardship.md), or `recurrence` on the MCP
`add_periodic_commitment` tool) use a small canonical vocabulary.

| Value | Meaning |
|-------|---------|
| `daily` | Every day |
| `weekly` | Every week |
| `monthly` | Every month |
| `every N months` | Every N months (e.g. `every 3 months` for quarterly) |
| `yearly` | Every year |

Quote any value containing a space on the command line.

## Examples

```bash
cdno stewardship add-periodic --stewardship health   --title "Dental check-up"     --every "every 6 months" --next 2026-09-01
cdno stewardship add-periodic --stewardship finances --title "File quarterly taxes" --every "every 3 months" --next 2026-07-15
cdno stewardship add-periodic --stewardship health   --title "Annual physical"      --every yearly           --next 2026-11-01
```

Each line becomes a row in the dashboard's `## Periodic Commitments` section and feeds the aggregated
[`cdno commitments`](cli/commitments.md) view, advancing to its next occurrence as dates pass.

## See also

- [Stewardships and tracking](../tutorials/stewardships-and-tracking.md).
- [Commitments and deadlines](../tutorials/commitments.md).
