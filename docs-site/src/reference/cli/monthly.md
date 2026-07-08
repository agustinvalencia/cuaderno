# `cdno monthly`

Show the monthly review note: Wins, Themes, Next Month's Focus, and the month's linked weeks.

```text
cdno monthly [OPTIONS]
```

## Options

| Flag | Description |
|------|-------------|
| `--date <DATE>` | Any day in the target calendar month (`YYYY-MM-DD`). Defaults to this month. |

Plus the [global options](overview.md#global-options). `monthly` ignores `--json`.

## Examples

```bash
cdno monthly                     # this calendar month
cdno monthly --date 2026-04-20   # the month containing 20 Apr 2026
```

To *write* the monthly sections rather than just view them, use the guided
[`review monthly`](review.md).

## Related MCP tools

[`read_monthly_note`](../mcp/reads.md) and [`get_monthly_context`](../mcp/reads.md).

## See also

- [Note types](../../concepts/note-types.md).
- [`review`](review.md).
