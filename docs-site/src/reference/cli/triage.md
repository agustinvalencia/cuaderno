# `cdno triage`

Process uncategorised `inbox/` captures. For each one, keep it as a project action, discard it, or
skip it.

```text
cdno triage [OPTIONS]
```

## Options

Only the [global options](overview.md#global-options). `triage` ignores `--json`.

## Behaviour

- **Interactive** — walks each pending capture and prompts you to keep (turn into a project action),
  discard, or skip.
- **Non-interactive** (piped or `--no-interactive`) — just **lists** what's pending, without changing
  anything.

## Examples

```bash
# Work through the inbox:
cdno triage

# Just see the backlog (no changes):
cdno triage --no-interactive
```

## Related MCP tools

[`triage_inbox`](../mcp/reads.md) (lists pending items) and
[`discard_inbox_item`](../mcp/writes.md) (clears one).

## See also

- [Inbox and triage](../../tutorials/inbox-and-triage.md).
- [`capture`](capture.md).
