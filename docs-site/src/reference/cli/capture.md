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

When an action is in [focus](../../concepts/contexts-and-energy.md#focus), the new inbox item's
frontmatter gets `captured_during: <project-slug>`, recording where your attention was when the
thought arrived. It is the one thing capture adds on its own; with nothing in focus the item is
exactly what you typed.

Capture is meant to be frictionless — no fields, no decisions. Classify later during
[triage](../../tutorials/inbox-and-triage.md).

## Related MCP tool

[`capture`](../mcp/writes.md).

## See also

- [Inbox and triage](../../tutorials/inbox-and-triage.md).
- [`triage`](triage.md) — process what you've captured.
