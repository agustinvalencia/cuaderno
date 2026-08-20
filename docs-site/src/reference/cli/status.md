# `cdno status`

A quick snapshot: your active projects and each one's top next action. Lighter than
[`orient`](orient.md) — no commitments digest, no suggestion.

```text
cdno status [OPTIONS]
```

In a terminal this then offers to open one of the projects it just listed, printing what `cdno
project show` would and asking again until you press Esc. Piped output, `--no-interactive`, and
`--json` skip the prompt. See [Colour and interactivity](../colour-and-interactivity.md).

## Options

Only the [global options](overview.md#global-options). With `--json`, emits the snapshot as
structured data.

## Examples

```bash
cdno status
cdno status --json | jq '.[].slug'
```

## Related MCP tool

[`list_projects`](../mcp/reads.md) — the projects view for AI clients.

## See also

- [`orient`](orient.md) — the fuller morning view.
- [Managing projects](../../tutorials/projects.md).
