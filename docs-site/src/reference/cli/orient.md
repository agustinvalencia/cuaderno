# `cdno orient`

Daily orientation: the commitments due soon, your active projects, and a single suggested starting
point.

```text
cdno orient [OPTIONS]
```

## Options

| Flag | Description |
|------|-------------|
| `--energy <ENERGY>` | Bias the suggested starting point toward this [energy level](../../concepts/contexts-and-energy.md): `deep`, `medium`, or `light`. |

Plus the [global options](overview.md#global-options). With `--json`, emits the orientation as a
structured object.

## Examples

```bash
cdno orient                 # neutral suggestion
cdno orient --energy deep   # bias toward a heavy-focus action
cdno orient --energy light  # bias toward something quick on a low day
cdno orient --json | jq '.suggested_start'
```

## Related MCP tool

[`get_orientation`](../mcp/reads.md).

## See also

- [The daily loop](../../tutorials/daily-loop.md).
- [`status`](status.md) — projects only, without the commitments digest.
