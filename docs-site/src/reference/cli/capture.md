# `cdno capture`

Drop a quick note into `inbox/` with a slug-based filename, to be processed later with
[`triage`](triage.md).

```text
cdno capture [OPTIONS] <TEXT>
```

## Arguments

| Argument | Description |
|----------|-------------|
| `<TEXT>` | The note text. Quote it if it contains spaces. |

## Options

Only the [global options](overview.md#global-options). With `--json`, emits a `{path, message}`
result.

## Examples

```bash
cdno capture "does Chen 2025 use the same preconditioner?"
cdno capture "ask IT about the cluster quota" --json
```

Capture is meant to be frictionless — no fields, no decisions. Classify later during
[triage](../../tutorials/inbox-and-triage.md).

## Related MCP tool

[`capture`](../mcp/writes.md).

## See also

- [Inbox and triage](../../tutorials/inbox-and-triage.md).
- [`triage`](triage.md) — process what you've captured.
