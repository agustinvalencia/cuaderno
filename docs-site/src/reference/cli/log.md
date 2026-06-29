# `cdno log`

Append a log entry to today's daily note (creating the note if it doesn't exist yet).

```text
cdno log [OPTIONS] <MESSAGE>
```

## Arguments

| Argument | Description |
|----------|-------------|
| `<MESSAGE>` | The log message. Quote it if it contains spaces. |

## Options

| Flag | Description |
|------|-------------|
| `--at <TIMESTAMP>` | Override the timestamp. Accepts `YYYY-MM-DDTHH:MM:SS` or `YYYY-MM-DDTHH:MM`. Defaults to now. |

Plus the [global options](overview.md#global-options). With `--json`, emits a `{path, message}`
result.

## Examples

```bash
cdno log "scaled the mesh to 2M cells; 4x runtime, still stable"

# Backdate an entry:
cdno log "forgot to record: fixed the sampler seed" --at 2026-04-24T18:30

# Scripted:
cdno log "nightly run complete" --json
# -> { "path": "journal/2026/daily/2026-04-25.md", "message": "Logged to ..." }
```

Daily notes are [append-only](../../concepts/business-rules.md) — `log` only ever adds.

## Related MCP tool

[`append_to_log`](../mcp/writes.md) — the same operation for AI clients.

## See also

- [The daily loop](../../tutorials/daily-loop.md).
