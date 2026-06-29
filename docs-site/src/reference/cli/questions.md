# `cdno questions`

List active questions grouped by domain (research, life) — the orientation surface against the
question system. For lifecycle changes (park / answer / retire / activate), use
[`cdno question`](question.md).

```text
cdno questions [OPTIONS]
```

## Options

Only the [global options](overview.md#global-options). With `--json`, emits the list as structured
data.

## Examples

```bash
cdno questions
cdno questions --json | jq '.[].slug'
```

## Related MCP tool

[`get_active_questions`](../mcp/reads.md) — which additionally accepts a `domain` filter.

## See also

- [Research and evidence](../../tutorials/research-and-evidence.md).
- [`question`](question.md) — create questions and transition their status.
