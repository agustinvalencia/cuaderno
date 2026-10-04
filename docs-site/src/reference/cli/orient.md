# `cdno orient`

Daily orientation: the commitments due soon, your active projects, a single suggested starting
point, and any stewardship habits that have lapsed.

A habit counts as lapsed when its line in a stewardship dashboard's `## Active Habits` section
declares it so — a status starting with "lapsed" after the em-dash, e.g.
`- Swimming 1x/week — lapsed since March`. The dashboard is the source of truth (updated
during reviews); orientation only surfaces what it says, without judgement.

```text
cdno orient [OPTIONS]
```

In a terminal this then offers to open one of the projects it just listed, printing what `cdno
project show` would and asking again until you press Esc. Piped output, `--no-interactive`, and
`--json` skip the prompt. See [Colour and interactivity](../colour-and-interactivity.md).

`orient` does not show what is in [focus](../../concepts/contexts-and-energy.md#focus) or the last
thing you paused, and neither field is in its `--json`: run [`cdno now`](now.md) for those. The MCP
[`get_orientation`](../mcp/reads.md) does carry both, because an agent has no other cheap way to know.

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
