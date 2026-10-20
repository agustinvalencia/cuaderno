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

The suggested start is drawn from the projects that count toward the cap first (active, and naming
no one else as holder; parked projects are not listed at all): a project handed to
someone else ([`cdno project hand-off`](project.md#cdno-project-hand-off)) is not your move, so it is
suggested only when no project of yours has an open action, and then with its holder named. Its card
in the project list carries a `held by:` line.

Projects that share a [core question](project.md#cdno-project-core-question) are one undertaking,
so two or more of them read as **one card**: the question as its title, then one line naming each
project with its next action (`held by <name>` beside one someone else is moving). A project with no
question, or the only one under its question, keeps its own card. Under `--json` the same
arrangement is the `groups` array (see [JSON output](../json-output.md)).

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
cdno orient --json | jq '.slots'   # active, counted and cap
```

## Related MCP tool

[`get_orientation`](../mcp/reads.md).

## See also

- [The daily loop](../../tutorials/daily-loop.md).
- [`status`](status.md) — projects only, without the commitments digest.
