# `cdno weekly`

Show the weekly review/plan note: Wins, Challenges, One Improvement, and This Week's Goal.

```text
cdno weekly [OPTIONS]
```

## Options

| Flag | Description |
|------|-------------|
| `--date <DATE>` | Any day in the target ISO week (`YYYY-MM-DD`). Defaults to this week. |

Plus the [global options](overview.md#global-options). `weekly` ignores `--json`.

## Examples

```bash
cdno weekly                     # this ISO week
cdno weekly --date 2026-04-20   # the week containing 20 Apr 2026
```

To *write* the weekly sections rather than just view them, use the guided
[`review weekly`](review.md).

## Related MCP tools

[`read_weekly_note`](../mcp/reads.md) and [`get_weekly_context`](../mcp/reads.md).

## See also

- [Weekly review](../../tutorials/weekly-review.md).
- [`review`](review.md).
